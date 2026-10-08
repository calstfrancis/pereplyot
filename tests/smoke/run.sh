#!/usr/bin/env bash
# Headless smoke test: drives the real binary under Xvfb and checks the sidecar it writes.
# Usage: [SMOKE_CASES="plain epub"] [SMOKE_KEEP=1] tests/smoke/run.sh [path/to/pereplyot]   (needs Xvfb, xdotool, ImageMagick, python3)
# PDFIUM_LIB_PATH must point at a directory containing libpdfium.so.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
BIN="${1:-$ROOT/target/debug/pereplyot}"
WORK="$(mktemp -d)"
FAILS=0
APP_PGID=""

cleanup() {
    stop_app
    [ -n "${XVFB_PID:-}" ] && kill "$XVFB_PID" 2>/dev/null
    if [ -n "${SMOKE_KEEP:-}" ]; then echo "kept $WORK"; else rm -rf "$WORK"; fi
}
trap cleanup EXIT

[ -x "$BIN" ] || { echo "no binary at $BIN (cargo build -p pereplyot-ui-gtk first)"; exit 2; }
: "${PDFIUM_LIB_PATH:?set PDFIUM_LIB_PATH to a directory containing libpdfium.so}"
for tool in Xvfb xdotool import python3 dbus-run-session; do
    command -v "$tool" >/dev/null || { echo "missing tool: $tool"; exit 2; }
done

FX="$ROOT/tests/fixtures"

cat >"$WORK/session.conf" <<'CONF'
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig><type>session</type><keep_umask/><listen>unix:tmpdir=/tmp</listen>
<policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>
CONF

Xvfb -displayfd 3 -screen 0 1400x900x24 3>"$WORK/display" >/dev/null 2>&1 &
XVFB_PID=$!
for _ in $(seq 50); do [ -s "$WORK/display" ] && break; sleep 0.1; done
[ -s "$WORK/display" ] || { echo "Xvfb did not start"; exit 2; }
export DISPLAY=":$(tr -d '\n' <"$WORK/display")"

start_app() {
    local file="$1"
    rm -rf "$WORK/h"
    mkdir -p "$WORK/h"/{home,data,config,cache}
    # setsid gives the app its own process group so stop_app can take down dbus-run-session
    # and its children together; dbus-run-session does not forward SIGTERM reliably.
    env -u WAYLAND_DISPLAY GDK_BACKEND=x11 GTK_A11Y=none GSETTINGS_BACKEND=memory \
        WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 WEBKIT_DISABLE_COMPOSITING_MODE=1 \
        HOME="$WORK/h/home" XDG_DATA_HOME="$WORK/h/data" XDG_CONFIG_HOME="$WORK/h/config" \
        XDG_CACHE_HOME="$WORK/h/cache" \
        setsid dbus-run-session --config-file="$WORK/session.conf" -- "$BIN" "$file" \
        >"$WORK/app.log" 2>&1 </dev/null &
    APP_PGID=$!
}

stop_app() {
    [ -n "$APP_PGID" ] || return 0
    kill -TERM -- "-$APP_PGID" 2>/dev/null
    sleep 0.3
    kill -KILL -- "-$APP_PGID" 2>/dev/null
    APP_PGID=""
}

reader_window() {
    for _ in $(seq 100); do
        for w in $(xdotool search --onlyvisible --name '' 2>/dev/null); do
            local g
            g="$(xdotool getwindowgeometry "$w" 2>/dev/null | tail -1)"
            case "$(xdotool getwindowname "$w" 2>/dev/null)" in
                Reader*) echo "$w"; return 0 ;;
            esac
            : "$g"
        done
        sleep 0.2
    done
    return 1
}

screenshot() { import -window root "$1" 2>/dev/null; }

snippets() {
    annotations_json | python3 -c 'import sys,json
try:
    d=json.load(sys.stdin)
    print(" | ".join((a.get("snippet") or "").strip() for a in d.get("annotations",[])))
except Exception:
    print("")'
}

annotations_json() {
    local f
    f="$(find "$WORK/h/data" -path '*annotations*' -name '*.json' 2>/dev/null | head -1)"
    [ -n "$f" ] && cat "$f"
}

# check NAME OK [DETAIL] [KNOWN]: KNOWN names a tracked bug. A known failure is reported but does
# not fail the run; if it starts passing the run fails until the marker is removed.
check() {
    local name="$1" ok="$2" detail="${3:-}" known="${4:-}"
    if [ -n "$known" ]; then
        if [ "$ok" = 1 ]; then
            echo "XPASS $name  (fixed - drop the known-bug marker: $known)"; FAILS=$((FAILS + 1))
        else
            echo "XFAIL $name  [$known] $detail"
        fi
    elif [ "$ok" = 1 ]; then echo "PASS  $name"
    else echo "FAIL  $name  $detail"; FAILS=$((FAILS + 1)); fi
}

# Prints "x0 y0 x1 y1": the bounding box of the dark text inside the page area.
text_bands() {
    python3 "$HERE/text_bands.py" "$1"
}

run_pdf_case() {
    local label="$1" file="$2" axis="$3" band_idx="$4" expect="$5" known_geometry="${6:-}" known_punct="${7:-}"
    start_app "$FX/$file"
    local w
    w="$(reader_window)" || { check "$label: reader window opens" 0 "(no window; log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 2
    xdotool windowfocus "$w" 2>/dev/null
    screenshot "$WORK/$label.png"
    local geom
    geom="$(text_bands "$WORK/$label.png")"
    if [ -z "$geom" ]; then
        check "$label: text found on page" 0
        stop_app
        return
    fi
    read -r x0 y0 x1 y1 <<<"$geom"
    local sx sy ex ey
    if [ "$axis" = y ]; then
        local h=$(( (y1 - y0) / 4 ))
        sy=$(( y0 + band_idx * h + h / 2 )); ey=$sy
        sx=$((x0 - 6)); ex=$((x1 + 6))
    else
        local wd=$(( (x1 - x0) / 4 ))
        sx=$(( x0 + band_idx * wd + wd / 2 )); ex=$sx
        sy=$((y0 - 6)); ey=$((y1 + 6))
    fi
    xdotool mousemove "$sx" "$sy" mousedown 1 mousemove $(( (sx + ex) / 2 )) $(( (sy + ey) / 2 )) mousemove "$ex" "$ey" mouseup 1
    sleep 1
    screenshot "$WORK/$label-selected.png"
    xdotool key 1
    sleep 1
    screenshot "$WORK/$label-marked.png"
    local json
    json="$(annotations_json)"
    local got
    got="$(printf '%s' "$json" | python3 -c 'import sys,json
try:
    d=json.load(sys.stdin)
    print(" | ".join((a.get("snippet") or "").strip() for a in d.get("annotations",[])))
except Exception as e:
    print("")')"
    local bare_got bare_expect
    bare_got="$(printf '%s' "$got" | tr -cd '[:alnum:] ')"
    bare_expect="$(printf '%s' "$expect" | tr -cd '[:alnum:] ')"
    check "$label: dragging over a line highlights that line" "$([ "$bare_got" = "$bare_expect" ] && echo 1 || echo 0)" "(got '$got')" "$known_geometry"
    check "$label: the quote keeps its punctuation" "$([ "$got" = "$expect" ] && echo 1 || echo 0)" "(got '$got')" "${known_punct:-$known_geometry}"
    if [ "$label" = plain ]; then
        local on off
        on="$(python3 "$HERE/text_bands.py" "$WORK/$label-marked.png" --has-highlight)"
        check "$label: the highlight is drawn on the page" "$([ "${on:-0}" -gt 200 ] && echo 1 || echo 0)" "(amber pixels: $on)"
        xdotool key ctrl+z
        sleep 1
        screenshot "$WORK/$label-undone.png"
        off="$(python3 "$HERE/text_bands.py" "$WORK/$label-undone.png" --has-highlight)"
        check "$label: Ctrl+Z removes it from the page and the saved file" "$([ "${off:-1}" -lt 20 ] && [ -z "$(snippets)" ] && echo 1 || echo 0)" "(amber pixels: $off, saved: '$(snippets)')"
        xdotool key ctrl+shift+z
        sleep 1
        screenshot "$WORK/$label-redone.png"
        on="$(python3 "$HERE/text_bands.py" "$WORK/$label-redone.png" --has-highlight)"
        check "$label: Ctrl+Shift+Z brings it back" "$([ "${on:-0}" -gt 200 ] && [ -n "$(snippets)" ] && echo 1 || echo 0)" "(amber pixels: $on, saved: '$(snippets)')"
    fi
    stop_app
}

run_epub_case() {
    start_app "$FX/book.epub"
    local w
    w="$(reader_window)" || { check "epub: reader window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 4
    xdotool windowfocus "$w" 2>/dev/null
    screenshot "$WORK/epub.png"
    local geom
    geom="$(python3 "$HERE/text_bands.py" "$WORK/epub.png" --last-line)"
    if [ -z "$geom" ]; then check "epub: text found on page" 0; stop_app; return; fi
    read -r x0 y0 x1 y1 <<<"$geom"
    local y=$(( (y0 + y1) / 2 ))
    xdotool mousemove $((x0 - 4)) "$y" mousedown 1 mousemove $(( (x0 + x1) / 2 )) "$y" mousemove $((x1 + 8)) "$y" mouseup 1
    sleep 1
    xdotool key 1
    sleep 1
    local got
    got="$(annotations_json | python3 -c 'import sys,json
try:
    d=json.load(sys.stdin)
    print(" | ".join((a.get("snippet") or "").strip() for a in d.get("annotations",[])))
except Exception:
    print("")')"
    check "epub: dragging over a paragraph highlights it" "$([ "$got" = "The quick brown fox jumps over the lazy dog." ] && echo 1 || echo 0)" "(got '$got')"
    xdotool key ctrl+z
    sleep 1
    got="$(snippets)"
    check "epub: Ctrl+Z removes the highlight from the saved file" "$([ -z "$got" ] && echo 1 || echo 0)" "(got '$got')"
    xdotool key ctrl+shift+z
    sleep 1
    got="$(snippets)"
    check "epub: Ctrl+Shift+Z brings it back" "$([ "$got" = "The quick brown fox jumps over the lazy dog." ] && echo 1 || echo 0)" "(got '$got')"
    stop_app
}

# Line order in the fixtures: "Page N", fox, Pack, Sphinx.
want() { [ -z "${SMOKE_CASES:-}" ] || [[ " $SMOKE_CASES " == *" $1 "* ]]; }
want plain && run_pdf_case plain plain.pdf y 2 "Pack my box with five dozen liquor jugs."
want cropbox && run_pdf_case cropbox cropbox.pdf y 2 "Pack my box with five dozen liquor jugs."
want rotated && run_pdf_case rotated rotated.pdf y 2 "Pack my box with five dozen liquor jugs."
want epub && run_epub_case

echo
if [ "$FAILS" -eq 0 ]; then echo "smoke: all passed"; else echo "smoke: $FAILS failed"; exit 1; fi
