#!/usr/bin/env bash
# Headless smoke test: drives the real binary under Xvfb and checks the sidecar it writes.
# Usage: [SMOKE_CASES="plain epub"] [SMOKE_KEEP=1] tests/smoke/run.sh [path/to/pereplyot]   (needs Xvfb, xdotool, ImageMagick, python3)
# PDFIUM_LIB_PATH (default build-flatpak/files/lib) must point at a directory containing libpdfium.so.
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
: "${PDFIUM_LIB_PATH:=$ROOT/build-flatpak/files/lib}"
[ -e "$PDFIUM_LIB_PATH/libpdfium.so" ] || { echo "set PDFIUM_LIB_PATH to a directory containing libpdfium.so"; exit 2; }
export PDFIUM_LIB_PATH
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
    [ "$file" = - ] && file=""
    [ "${2:-}" = keep ] || rm -rf "$WORK/h"
    mkdir -p "$WORK/h"/{home,data,config,cache} "$WORK/h/data/fonts"
    cp "$ROOT"/packaging/fonts/*.otf "$WORK/h/data/fonts/" 2>/dev/null
    # setsid gives the app its own process group so stop_app can take down dbus-run-session
    # and its children together; dbus-run-session does not forward SIGTERM reliably.
    env -u WAYLAND_DISPLAY ${WELCOME_OK:-PEREPLYOT_NO_WELCOME=1} GDK_BACKEND=x11 GTK_A11Y=none GSETTINGS_BACKEND=memory \
        WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 WEBKIT_DISABLE_COMPOSITING_MODE=1 \
        HOME="$WORK/h/home" XDG_DATA_HOME="$WORK/h/data" XDG_CONFIG_HOME="$WORK/h/config" \
        XDG_CACHE_HOME="$WORK/h/cache" \
        setsid dbus-run-session --config-file="$WORK/session.conf" -- "$BIN" ${file:+"$file"} \
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
        local hx0 hy0 hx1 hy1 original
        read -r hx0 hy0 hx1 hy1 <<<"$(python3 "$HERE/text_bands.py" "$WORK/$label-redone.png" --highlight-box)"
        original="$(snippets)"
        local hmid=$(( (hy0 + hy1) / 2 ))
        xdotool mousemove $(( (hx0 + hx1) / 2 )) "$hmid" click 1
        sleep 0.5
        xdotool key Delete
        sleep 1
        check "$label: selecting a highlight and pressing Delete removes it" "$([ -z "$(snippets)" ] && echo 1 || echo 0)" "(saved: '$(snippets)')"
        xdotool key ctrl+z
        sleep 1
        check "$label: Ctrl+Z brings the deleted highlight back" "$([ "$(snippets)" = "$original" ] && echo 1 || echo 0)" "(saved: '$(snippets)')"
        xdotool mousemove $(( (hx0 + hx1) / 2 )) "$hmid" click 1
        sleep 0.5
        xdotool mousemove "$hx1" "$hy1" mousedown 1
        sleep 0.2
        xdotool mousemove $(( hx0 + (hx1 - hx0) * 8 / 10 )) "$hmid"
        sleep 0.2
        xdotool mousemove $(( hx0 + (hx1 - hx0) * 6 / 10 )) "$hmid"
        sleep 0.3
        xdotool mouseup 1
        sleep 1
        local shorter
        shorter="$(snippets)"
        check "$label: dragging the end handle shortens the highlight" "$([ -n "$shorter" ] && [ "${#shorter}" -lt "${#original}" ] && [[ "$original" == "$shorter"* ]] && echo 1 || echo 0)" "(was '$original', now '$shorter')"
        xdotool key ctrl+z
        sleep 1
        check "$label: Ctrl+Z puts the highlight back to its full length" "$([ "$(snippets)" = "$original" ] && echo 1 || echo 0)" "(saved: '$(snippets)')"
        xdotool key Escape
        xdotool key ctrl+z
        sleep 0.5
        xdotool key ctrl+f
        sleep 0.4
        xdotool type --delay 30 "sphinx"
        xdotool key Return
        sleep 2
        screenshot "$WORK/$label-search.png"
        local blue
        blue="$(python3 "$HERE/text_bands.py" "$WORK/$label-search.png" --has-search-match)"
        check "$label: searching marks the match on the page" "$([ "${blue:-0}" -gt 150 ] && echo 1 || echo 0)" "(blue pixels: $blue)"
    fi
    stop_app
}

run_thumbs_case() {
    start_app "$FX/plain.pdf"
    local w
    w="$(reader_window)" || { check "thumbs: reader window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 2
    xdotool windowfocus "$w" 2>/dev/null
    xdotool mousemove 29 28 click 1
    sleep 2.5
    screenshot "$WORK/thumbs.png"
    local ink
    ink="$(python3 "$HERE/text_bands.py" "$WORK/thumbs.png" --sidebar-ink)"
    check "thumbs: opening the sidebar fills in the page thumbnails" "$([ "${ink:-0}" -gt 100 ] && echo 1 || echo 0)" "(text pixels on the first thumbnail: $ink)"
    stop_app
}

run_reading_case() {
    start_app "$FX/monograph.pdf"
    local w
    w="$(reader_window)" || { check "reading: reader window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 2
    xdotool windowfocus "$w" 2>/dev/null
    xdotool key t
    sleep 3
    screenshot "$WORK/reading.png"
    local left right
    left="$(python3 "$HERE/text_bands.py" "$WORK/reading.png" --ink-in 36 128 70 160)"
    right="$(python3 "$HERE/text_bands.py" "$WORK/reading.png" --ink-in 770 250 960 460)"
    check "reading: the printed page number is in the left margin" "$([ "${left:-0}" -gt 6 ] && echo 1 || echo 0)" "(ink: $left)"
    check "reading: the footnote is in the right margin beside its line" "$([ "${right:-0}" -gt 80 ] && echo 1 || echo 0)" "(ink: $right)"
    xdotool mousemove 29 27 click 1
    sleep 1
    screenshot "$WORK/reading-contents.png"
    local crumb
    crumb="$(python3 "$HERE/text_bands.py" "$WORK/reading-contents.png" --ink-in 915 54 985 72)"
    check "reading: a PDF with no outline gets Contents from the headings Reading mode finds" "$([ "${crumb:-0}" -gt 20 ] && echo 1 || echo 0)" "(breadcrumb ink: $crumb)"
    xdotool mousemove 29 27 click 1
    sleep 0.5
    xdotool key ctrl+f
    sleep 0.4
    xdotool type --delay 30 "liturgy"
    xdotool key Return
    sleep 2
    screenshot "$WORK/reading-search.png"
    local blue
    blue="$(python3 "$HERE/text_bands.py" "$WORK/reading-search.png" --has-search-match)"
    check "reading: a search hit is marked in the text" "$([ "${blue:-0}" -gt 100 ] && echo 1 || echo 0)" "(blue pixels: $blue)"
    xdotool key Escape
    xdotool windowfocus "$w" 2>/dev/null
    sleep 0.3
    xdotool mousemove 110 205 mousedown 1 mousemove 400 205 mousemove 650 205 mouseup 1
    sleep 1
    xdotool key 1
    sleep 1
    local saved
    saved="$(annotations_json | python3 -c 'import sys,json
try:
    d=json.load(sys.stdin)
    a=d["annotations"][0]
    print(a.get("page"), len(a.get("quadpoints", [])), (a.get("snippet") or "")[:14])
except Exception:
    print("")')"
    check "reading: marking a selection saves its words, page and where they sit (a line of the reading text can span two lines of the page)" "$([[ "$saved" == "1 "[12]" of for at from" ]] && echo 1 || echo 0)" "(got '$saved')"
    xdotool key t
    sleep 2
    screenshot "$WORK/reading-page.png"
    local on
    on="$(python3 "$HERE/text_bands.py" "$WORK/reading-page.png" --has-highlight)"
    check "reading: the mark made in Reading mode is drawn on the page" "$([ "${on:-0}" -gt 150 ] && echo 1 || echo 0)" "(amber pixels: $on)"
    stop_app
}

same_view() { # same_view A B -> 1 when the text on the page of two screenshots is the same
    local ga gb n
    ga="$(text_bands "$1")"; gb="$(text_bands "$2")"
    [ -n "$ga" ] && [ -n "$gb" ] || { echo 0; return; }
    read -r ax0 ay0 ax1 ay1 <<<"$ga"
    read -r bx0 by0 bx1 by1 <<<"$gb"
    convert "$1" -crop "$((ax1 - ax0 + 1))x$((ay1 - ay0 + 1))+$ax0+$ay0" +repage "$WORK/cmp-a.png"
    convert "$2" -crop "$((bx1 - bx0 + 1))x$((by1 - by0 + 1))+$bx0+$by0" +repage "$WORK/cmp-b.png"
    n="$(compare -metric AE -fuzz 8% "$WORK/cmp-a.png" "$WORK/cmp-b.png" null: 2>&1 | grep -oE '^[0-9]+' | head -1)"
    [ "${n:-999999}" -lt 60 ] && echo 1 || echo 0
}

run_history_case() {
    start_app "$FX/plain.pdf"
    local w
    w="$(reader_window)" || { check "history: reader window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 2
    xdotool windowfocus "$w" 2>/dev/null
    screenshot "$WORK/hist-a.png"
    xdotool key End
    sleep 1.5
    screenshot "$WORK/hist-b.png"
    xdotool key alt+Left
    sleep 1.5
    screenshot "$WORK/hist-c.png"
    xdotool key alt+Right
    sleep 1.5
    screenshot "$WORK/hist-d.png"
    check "history: End moves to another page" "$([ "$(same_view "$WORK/hist-a.png" "$WORK/hist-b.png")" = 0 ] && echo 1 || echo 0)"
    check "history: Alt+Left goes back to the page you came from" "$(same_view "$WORK/hist-a.png" "$WORK/hist-c.png")"
    check "history: Alt+Right goes forward again" "$(same_view "$WORK/hist-b.png" "$WORK/hist-d.png")"
    stop_app
}

run_hover_case() {
    start_app "$FX/cite-authoryear.pdf"
    local w
    w="$(reader_window)" || { check "hover: reader window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 2
    xdotool windowfocus "$w" 2>/dev/null
    xdotool mousemove 300 500
    sleep 0.5
    screenshot "$WORK/hover-before.png"
    # The first rest starts reading the bibliography in the background; the second is answered.
    xdotool mousemove 600 236
    sleep 1.5
    xdotool mousemove 606 235
    sleep 0.3
    xdotool mousemove 608 236
    sleep 2
    screenshot "$WORK/hover-after.png"
    local before after
    before="$(python3 "$HERE/text_bands.py" "$WORK/hover-before.png" --ink-in 370 250 850 345)"
    after="$(python3 "$HERE/text_bands.py" "$WORK/hover-after.png" --ink-in 370 250 850 345)"
    check "hover: resting on an unlinked citation previews its bibliography entry" "$([ "${after:-0}" -gt $(( ${before:-0} + 150 )) ] && echo 1 || echo 0)" "(ink before $before, after $after)"
    xdotool mousemove 700 600
    sleep 1
    screenshot "$WORK/hover-gone.png"
    local gone
    gone="$(python3 "$HERE/text_bands.py" "$WORK/hover-gone.png" --ink-in 370 250 850 345)"
    check "hover: moving away dismisses the preview" "$([ "${gone:-999}" -lt $(( ${before:-0} + 40 )) ] && echo 1 || echo 0)" "(ink after moving away $gone)"
    stop_app
    start_app "$FX/linked.pdf"
    w="$(reader_window)" || { check "hover: linked document opens" 0; stop_app; return; }
    sleep 2
    xdotool windowfocus "$w" 2>/dev/null
    xdotool mousemove 300 500
    sleep 0.5
    screenshot "$WORK/link-before.png"
    xdotool mousemove 580 236
    sleep 0.3
    xdotool mousemove 585 235
    sleep 2
    screenshot "$WORK/link-after.png"
    before="$(python3 "$HERE/text_bands.py" "$WORK/link-before.png" --ink-in 345 250 830 530)"
    after="$(python3 "$HERE/text_bands.py" "$WORK/link-after.png" --ink-in 345 250 830 530)"
    check "hover: resting on a link previews where it leads" "$([ "${after:-0}" -gt $(( ${before:-0} + 120 )) ] && echo 1 || echo 0)" "(ink before $before, after $after)"
    stop_app
}

run_pin_case() {
    start_app "$FX/linked.pdf"
    local w
    w="$(reader_window)" || { check "pin: reader window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 2
    xdotool windowfocus "$w" 2>/dev/null
    xdotool mousemove 500 400 click 3
    sleep 1
    xdotool mousemove 435 491 click 1
    sleep 0.5
    xdotool mousemove 180 190 mousedown 1 mousemove 400 230 mousemove 680 262 mouseup 1
    sleep 2
    xdotool mousemove 500 450
    for _ in $(seq 20); do xdotool click 5; done
    sleep 1.5
    screenshot "$WORK/pin.png"
    local ink
    ink="$(python3 "$HERE/text_bands.py" "$WORK/pin.png" --ink-in 495 125 965 195)"
    check "pin: a pinned region stays on screen while you scroll to other pages" "$([ "${ink:-0}" -gt 200 ] && echo 1 || echo 0)" "(ink in the card: $ink)"
    stop_app
    start_app "$FX/linked.pdf" keep
    w="$(reader_window)" || { check "pin: reader window reopens" 0; stop_app; return; }
    sleep 3.5
    screenshot "$WORK/pin-back.png"
    local back
    back="$(python3 "$HERE/text_bands.py" "$WORK/pin-back.png" --ink-in 495 125 965 195)"
    check "pin: a pinned figure is back after reopening the document" "$([ "${back:-0}" -gt 200 ] && echo 1 || echo 0)" "(ink in the card: $back)"
    local grip_before grip_after
    grip_before="$(python3 "$HERE/text_bands.py" "$WORK/pin-back.png" --ink-in 495 93 965 121)"
    xdotool mousemove 953 107 click 1
    sleep 1
    screenshot "$WORK/pin-closed.png"
    grip_after="$(python3 "$HERE/text_bands.py" "$WORK/pin-closed.png" --ink-in 495 93 965 121)"
    check "pin: the close button unpins it" "$([ "${grip_before:-0}" -gt 20 ] && [ "${grip_after:-999}" -lt 8 ] && echo 1 || echo 0)" "(title bar ink before $grip_before, after $grip_after)"
    stop_app
}

run_resume_case() {
    start_app "$FX/plain.pdf"
    local w
    w="$(reader_window)" || { check "resume: reader window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 2
    xdotool windowfocus "$w" 2>/dev/null
    xdotool mousemove 500 400
    for _ in 1 2 3 4 5 6; do xdotool click 5; done
    sleep 7
    stop_app
    start_app "$FX/plain.pdf" keep
    w="$(reader_window)" || { check "resume: reader window reopens" 0; stop_app; return; }
    sleep 2.8
    screenshot "$WORK/resume.png"
    local line
    line="$(python3 "$HERE/text_bands.py" "$WORK/resume.png" --ink-in 200 86 900 92)"
    check "resume: reopening puts a 'Continue here' marker where you stopped" "$([ "${line:-0}" -gt 500 ] && echo 1 || echo 0)" "(ink on the marker line: $line)"
    stop_app
}

run_split_case() {
    start_app "$FX/plain.pdf"
    local w
    w="$(reader_window)" || { check "split: reader window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 2
    xdotool windowfocus "$w" 2>/dev/null
    xdotool mousemove 771 28 click 1
    sleep 1
    xdotool mousemove 757 232 click 1
    sleep 3
    screenshot "$WORK/split.png"
    local left right
    left="$(python3 "$HERE/text_bands.py" "$WORK/split.png" --ink-in 20 150 480 400)"
    right="$(python3 "$HERE/text_bands.py" "$WORK/split.png" --ink-in 520 150 980 400)"
    check "split: both halves show the document" "$([ "${left:-0}" -gt 200 ] && [ "${right:-0}" -gt 200 ] && echo 1 || echo 0)" "(ink left $left, right $right)"
    # Mark a line in the right-hand pane (its own text, found by measuring), and see it in the left.
    local geom
    geom="$(python3 "$HERE/text_bands.py" "$WORK/split.png" --ink-box 520 90 985 700)"
    read -r x0 y0 x1 y1 <<<"$geom"
    local h=$(( (y1 - y0) / 4 ))
    local y=$(( y0 + 2 * h + h / 2 ))
    xdotool mousemove $((x0 - 6)) "$y" mousedown 1 mousemove $(( (x0 + x1) / 2 )) "$y" mousemove $((x1 + 6)) "$y" mouseup 1
    sleep 1
    xdotool key 1
    sleep 1.5
    screenshot "$WORK/split-marked.png"
    local amber_left amber_right saved
    amber_left="$(python3 "$HERE/text_bands.py" "$WORK/split-marked.png" --amber-in 20 90 485 700)"
    amber_right="$(python3 "$HERE/text_bands.py" "$WORK/split-marked.png" --amber-in 520 90 985 700)"
    saved="$(snippets)"
    check "split: a mark made in one pane is saved and shows in both" "$([ -n "$saved" ] && [ "${amber_left:-0}" -gt 120 ] && [ "${amber_right:-0}" -gt 120 ] && echo 1 || echo 0)" "(saved '$saved', amber left $amber_left, right $amber_right)"
    local lbox0 rbox0 lbox1 rbox1
    lbox0="$(python3 "$HERE/text_bands.py" "$WORK/split-marked.png" --ink-box 20 90 485 700)"
    rbox0="$(python3 "$HERE/text_bands.py" "$WORK/split-marked.png" --ink-box 520 90 985 700)"
    xdotool mousemove 530 100 click 1
    sleep 0.3
    xdotool key ctrl+plus
    sleep 1.5
    screenshot "$WORK/split-zoomed.png"
    lbox1="$(python3 "$HERE/text_bands.py" "$WORK/split-zoomed.png" --ink-box 20 90 485 700)"
    rbox1="$(python3 "$HERE/text_bands.py" "$WORK/split-zoomed.png" --ink-box 520 90 985 700)"
    check "split: a key pressed in the second pane acts on that pane alone" "$([ "$rbox1" != "$rbox0" ] && [ "$lbox1" = "$lbox0" ] && echo 1 || echo 0)" "(left '$lbox0' -> '$lbox1', right '$rbox0' -> '$rbox1')"
    xdotool mousemove 771 28 click 1
    sleep 1
    xdotool mousemove 757 232 click 1
    sleep 1.5
    screenshot "$WORK/split-closed.png"
    local bar_open bar_closed
    bar_open="$(python3 "$HERE/text_bands.py" "$WORK/split-marked.png" --ink-in 600 733 985 760)"
    bar_closed="$(python3 "$HERE/text_bands.py" "$WORK/split-closed.png" --ink-in 600 733 985 760)"
    check "split: the same menu row closes it" "$([ "${bar_open:-0}" -gt 150 ] && [ "${bar_closed:-999}" -lt 30 ] && echo 1 || echo 0)" "(second status bar ink open $bar_open, closed $bar_closed)"
    stop_app
}

run_figures_case() {
    start_app "$FX/figures.pdf"
    local w
    w="$(reader_window)" || { check "figures: reader window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 2
    xdotool windowfocus "$w" 2>/dev/null
    xdotool key t
    sleep 4
    screenshot "$WORK/figures.png"
    local blue
    blue="$(python3 "$HERE/text_bands.py" "$WORK/figures.png" --has-figure-blue)"
    check "figures: the chart is drawn in the text as a picture" "$([ "${blue:-0}" -gt 1500 ] && echo 1 || echo 0)" "(bar pixels: $blue)"
    stop_app
}

run_area_case() {
    start_app "$FX/figures.pdf"
    local w
    w="$(reader_window)" || { check "area: reader window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 2
    xdotool windowfocus "$w" 2>/dev/null
    xdotool key a
    sleep 0.5
    xdotool mousemove 200 300 mousedown 1 mousemove 400 380 mousemove 600 450 mouseup 1
    sleep 1.5
    screenshot "$WORK/area.png"
    local saved outline
    saved="$(annotations_json | python3 -c 'import sys,json
try:
    a=json.load(sys.stdin)["annotations"][0]
    r=a.get("rect") or []
    print(a.get("kind"), len(r), "ok" if len(r)==4 and r[2]>r[0] and r[3]>r[1] else "bad")
except Exception:
    print("")')"
    check "area: the Area tool (A) saves a page rectangle" "$([[ "$saved" == "area 4 ok" ]] && echo 1 || echo 0)" "(got '$saved')"
    xdotool key Escape
    xdotool mousemove 812 28 click 1
    sleep 2.5
    screenshot "$WORK/area-card.png"
    xdotool mousemove 812 28 click 1
    sleep 0.5
    xdotool mousemove 400 375 click 1
    sleep 1
    local before after
    rect_of() { annotations_json | python3 -c 'import sys,json
try:
    print(" ".join(str(round(v)) for v in json.load(sys.stdin)["annotations"][0]["rect"]))
except Exception:
    print("")'; }
    before="$(rect_of)"
    xdotool mousemove 600 450 mousedown 1 mousemove 650 480 mousemove 720 520 mouseup 1
    sleep 1.5
    after="$(rect_of)"
    read -r bl bb br bt <<<"$before"
    read -r al ab ar at <<<"$after"
    check "area: dragging a corner handle resizes the clip" "$([ -n "$ar" ] && [ "$ar" -gt "$br" ] && [ "$ab" -lt "$bb" ] && [ "$al" = "$bl" ] && echo 1 || echo 0)" "(rect $before -> $after)"
    xdotool key ctrl+z
    sleep 1
    xdotool key ctrl+z
    sleep 1
    check "area: Ctrl+Z (twice, after the resize) removes the clip" "$([ -z "$(annotations_json | python3 -c 'import sys,json
try:
    print(len(json.load(sys.stdin)["annotations"]))
except Exception:
    print(0)' | grep -v '^0$')" ] && echo 1 || echo 0)" "(annotations left: $(annotations_json | head -c 80))"
    stop_app
}

run_import_case() {
    start_app "$FX/annotated.pdf"
    local w
    w="$(reader_window)" || { check "import: reader window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 3
    screenshot "$WORK/import.png"
    local bar
    bar="$(python3 "$HERE/text_bands.py" "$WORK/import.png" --ink-in 240 748 760 788)"
    check "import: a PDF carrying another app's annotations offers to bring them in" "$([ "${bar:-0}" -gt 400 ] && echo 1 || echo 0)" "(toast ink: $bar)"
    xdotool mousemove 655 767 click 1
    sleep 1.5
    local got
    got="$(annotations_json | python3 -c 'import sys,json
try:
    a=json.load(sys.stdin)["annotations"]
    print(len(a), ",".join(sorted(x["kind"] for x in a)))
except Exception:
    print("")')"
    check "import: they become ordinary annotations of this app" "$([[ "$got" == "4 highlight,note,strikeout,underline" ]] && echo 1 || echo 0)" "(got '$got')"
    stop_app
    start_app "$FX/annotated.pdf" keep
    reader_window >/dev/null
    sleep 3
    screenshot "$WORK/import-again.png"
    bar="$(python3 "$HERE/text_bands.py" "$WORK/import-again.png" --ink-in 240 748 760 788)"
    check "import: it does not ask a second time" "$([ "${bar:-999}" -lt 150 ] && echo 1 || echo 0)" "(toast ink: $bar)"
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

run_mixed_case() {
    start_app "$FX/mixed.pdf"
    local w
    w="$(reader_window)" || { check "mixed: reader window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 2
    xdotool windowfocus "$w" 2>/dev/null
    for _ in 1 2 3; do xdotool key Next; sleep 0.4; done
    sleep 1.5
    screenshot "$WORK/mixed.png"
    local geom
    geom="$(text_bands "$WORK/mixed.png")"
    if [ -z "$geom" ]; then check "mixed: text found on the page reached by paging" 0; stop_app; return; fi
    read -r x0 y0 x1 y1 <<<"$geom"
    local y=$(( (y0 + y1) / 2 ))
    xdotool mousemove $((x0 - 6)) "$y" mousedown 1 mousemove $(( (x0 + x1) / 2 )) "$y" mousemove $((x1 + 6)) "$y" mouseup 1
    sleep 1
    xdotool key 1
    sleep 1
    local got
    got="$(annotations_json | python3 "$HERE/page_snippets.py")"
    check "mixed: after paging through pages of different sizes, a highlight lands on page 4" "$([ "$got" = "4:Page 4 of the mixed document" ] && echo 1 || echo 0)" "(got '$got')"
    stop_app
}

notebook_file() { find "$WORK/h/data" -path '*notebooks*' -name '*.typ' 2>/dev/null | head -1; }

run_notebook_case() {
    start_app "$FX/plain.pdf"
    local w
    w="$(reader_window)" || { check "notebook: reader window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 2
    xdotool windowfocus "$w" 2>/dev/null
    screenshot "$WORK/nb-0.png"
    local geom x0 y0 x1 y1
    geom="$(text_bands "$WORK/nb-0.png")"
    read -r x0 y0 x1 y1 <<<"$geom"
    local h=$(( (y1 - y0) / 4 )) sy
    sy=$(( y0 + 2 * h + h / 2 ))
    xdotool mousemove $((x0 - 6)) "$sy" mousedown 1 mousemove $(( (x0 + x1) / 2 )) "$sy" mousemove $((x1 + 6)) "$sy" mouseup 1
    sleep 1
    xdotool key 1
    sleep 1
    xdotool key n
    sleep 1.5
    xdotool mousemove 812 28 click 1
    sleep 1.5
    xdotool mousemove 470 120 mousedown 1
    for step in 1 2 3 4 5 6 7 8 9 10 11 12; do
        xdotool mousemove $((470 + step * 30)) $((120 + step * 10))
        sleep 0.15
    done
    sleep 0.5
    xdotool mouseup 1
    sleep 1.5
    screenshot "$WORK/nb-dropped.png"
    local ink
    ink="$(python3 "$HERE/text_bands.py" "$WORK/nb-dropped.png" --ink-in 670 125 880 205)"
    check "notebook: dragging a note from the list drops a quote card into it" "$([ "${ink:-0}" -gt 150 ] && echo 1 || echo 0)" "(ink in the card: $ink)"
    xdotool mousemove 830 500 click 1
    sleep 0.3
    xdotool type --delay 40 "My own paragraph."
    sleep 1.5
    local body
    body="$(cat "$(notebook_file)" 2>/dev/null)"
    check "notebook: the quote is saved in the Typst file with its source, page and words" "$([[ "$body" == *'#pquote('*'page: "1"'*'text: "Pack my box with five dozen liquor jugs."'* ]] && echo 1 || echo 0)" "(file: $(tail -c 300 "$(notebook_file)" 2>/dev/null))"
    check "notebook: what is typed is saved too" "$([[ "$body" == *'My own paragraph.'* ]] && echo 1 || echo 0)"
    xdotool mousemove 546 120 click 1
    sleep 1.5
    check "notebook: the + button adds another quote" "$([ "$(grep -c '^#pquote(' "$(notebook_file)")" = 2 ] && echo 1 || echo 0)" "(quotes: $(grep -c '^#pquote(' "$(notebook_file)"))"
    stop_app
    start_app "$FX/plain.pdf" keep
    reader_window >/dev/null
    sleep 2.5
    xdotool key n
    sleep 2
    screenshot "$WORK/nb-reopened.png"
    ink="$(python3 "$HERE/text_bands.py" "$WORK/nb-reopened.png" --ink-in 670 125 880 205)"
    check "notebook: reopening shows the same notebook, cards and all" "$([ "${ink:-0}" -gt 150 ] && echo 1 || echo 0)" "(ink in the card: $ink)"
    xdotool mousemove 971 72 click 1
    sleep 1
    screenshot "$WORK/nb-more.png"
    xdotool mousemove 879 236 click 1
    sleep 1.5
    screenshot "$WORK/nb-export.png"
    xdotool mousemove 403 27 click 1
    sleep 2
    xdotool key ctrl+a
    xdotool type --delay 20 "$WORK/essay.typ"
    xdotool key Return
    sleep 2
    local essay
    essay="$(cat "$WORK/essay.typ" 2>/dev/null)"
    check "notebook: Export writes Typst with the text, and every quote cited and linked" "$([[ "$essay" == *'My own paragraph.'* && "$essay" == *'#quote(block: true, attribution: [plain, p. 1 #link("pereplyot://open?hash='*')[↗]])[Pack my box with five dozen liquor jugs.]'* ]] && echo 1 || echo 0)" "(got: $(head -c 400 "$WORK/essay.typ" 2>/dev/null))"
    if command -v typst >/dev/null; then
        check "notebook: the exported Typst compiles" "$(typst compile "$WORK/essay.typ" "$WORK/essay.pdf" >/dev/null 2>&1 && echo 1 || echo 0)"
    fi
    stop_app
    start_app "$FX/mixed.pdf" keep
    reader_window >/dev/null
    sleep 2.5
    xdotool key n
    sleep 1.5
    xdotool mousemove 690 140 click 1
    sleep 3
    screenshot "$WORK/nb-other-after.png"
    local after_geom ax0 ay0
    after_geom="$(text_bands "$WORK/nb-other-after.png")"
    read -r ax0 ay0 _ <<<"$after_geom"
    check "notebook: clicking a quote's source opens that document at the passage" "$([ "${ax0:-0}" -ge 180 ] && [ "${ax0:-0}" -le 215 ] && [ "${ay0:-0}" -ge 222 ] && [ "${ay0:-0}" -le 248 ] && echo 1 || echo 0)" "(text starts at '${after_geom:-none}', want about 200 228 in a new tab)"
    stop_app
}

run_connect_case() {
    start_app "$FX/plain.pdf"
    local w
    w="$(reader_window)" || { check "connect: reader window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 2
    xdotool windowfocus "$w" 2>/dev/null
    screenshot "$WORK/cn-0.png"
    local geom x0 y0 x1 y1
    geom="$(text_bands "$WORK/cn-0.png")"
    read -r x0 y0 x1 y1 <<<"$geom"
    local h=$(( (y1 - y0) / 4 )) sy band
    for band in 1 3; do
        sy=$(( y0 + band * h + h / 2 ))
        xdotool mousemove $((x0 - 6)) "$sy" mousedown 1 mousemove $(( (x0 + x1) / 2 )) "$sy" mousemove $((x1 + 6)) "$sy" mouseup 1
        sleep 1
        xdotool key $((band == 1 ? 1 : 2))
        sleep 1
    done
    xdotool mousemove 812 28 click 1
    sleep 1.5
    xdotool mousemove 790 175 click 1
    sleep 1.2
    screenshot "$WORK/cn-2.png"
    xdotool mousemove 790 347 click 1
    sleep 1.5
    screenshot "$WORK/cn-3.png"
    xdotool type --delay 40 "contradicts"
    xdotool key Return
    sleep 1.5
    screenshot "$WORK/cn-4.png"
    local got
    got="$(python3 -c 'import json,sys
try:
    c=json.load(open(sys.argv[1]))["connections"]
    print(len(c), c[0]["note"], c[0]["a"]["id"] != c[0]["b"]["id"])
except Exception:
    print("")' "$WORK/h/data/pereplyot/connections.json")"
    check "connect: Connect on two annotations, with a note, saves one link" "$([[ "$got" == "1 contradicts True" ]] && echo 1 || echo 0)" "(got '$got')"
    local row
    row="$(python3 "$HERE/text_bands.py" "$WORK/cn-4.png" --ink-in 745 355 985 395)"
    check "connect: the link shows on the other annotation's card too" "$([ "${row:-0}" -gt 150 ] && echo 1 || echo 0)" "(ink in the second card's link row: $row)"
    xdotool mousemove 966 173 click 1
    sleep 1.2
    got="$(python3 -c 'import json,sys
print(len(json.load(open(sys.argv[1]))["connections"]))' "$WORK/h/data/pereplyot/connections.json" 2>/dev/null)"
    check "connect: removing it from one card removes it from both" "$([ "$got" = 0 ] && echo 1 || echo 0)" "(links left: $got)"
    stop_app
}

launcher_window() {
    for _ in $(seq 100); do
        for w in $(xdotool search --onlyvisible --name '' 2>/dev/null); do
            [ "$(xdotool getwindowname "$w" 2>/dev/null)" = Pereplyot ] && { echo "$w"; return 0; }
        done
        sleep 0.2
    done
    return 1
}

run_launcher_case() {
    start_app -
    local w
    w="$(launcher_window)" || { check "launcher: window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 1.5
    xdotool windowfocus "$w" 2>/dev/null
    xdotool mousemove 348 28 click 1
    sleep 1
    screenshot "$WORK/ln-1.png"
    xdotool mousemove 640 75 click 1
    sleep 1
    screenshot "$WORK/ln-2.png"
    xdotool type --delay 40 "Chapter three"
    xdotool key Return
    sleep 2
    screenshot "$WORK/ln-3.png"
    check "launcher: New notebook (Notebooks tab) creates a Typst file under the chosen name" "$([[ "$(head -1 "$(notebook_file)" 2>/dev/null)" == *'"title":"Chapter three"'* ]] && echo 1 || echo 0)" "(file: $(head -c 120 "$(notebook_file)" 2>/dev/null))"
    local ink
    ink="$(python3 "$HERE/text_bands.py" "$WORK/ln-3.png" --ink-in 20 60 500 85)"
    check "launcher: the new notebook opens in a window of its own" "$([ "${ink:-0}" -gt 100 ] && echo 1 || echo 0)" "(ink in its title: $ink)"
    stop_app
}

run_scholar_case() {
    start_app "$FX/scholar.epub"
    local w
    w="$(reader_window)" || { check "scholar: reader window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 4
    xdotool windowfocus "$w" 2>/dev/null
    screenshot "$WORK/sc-0.png"
    xdotool mousemove 504 538 click 1
    sleep 1.5
    screenshot "$WORK/sc-note.png"
    local pop
    pop="$(python3 "$HERE/text_bands.py" "$WORK/sc-note.png" --ink-in 335 575 675 600)"
    check "scholar: a note reference opens its note in a popover beside it" "$([ "${pop:-0}" -gt 250 ] && echo 1 || echo 0)" "(ink in the popover: $pop)"
    xdotool key Escape
    sleep 0.5
    local nav
    nav="$(python3 "$HERE/text_bands.py" "$WORK/sc-0.png" --ink-in 205 15 250 40)"
    check "scholar: the printed page from the book's page list is shown (p. 41)" "$([ "${nav:-0}" -gt 60 ] && echo 1 || echo 0)" "(ink in the page label: $nav)"
    xdotool mousemove 286 201 mousedown 1 mousemove 600 201 mousemove 968 201 mouseup 1
    sleep 1
    xdotool key 1
    sleep 1
    local labels
    labels() { annotations_json | python3 -c 'import sys,json
try:
    print(" ".join(str(a.get("page_label")) for a in json.load(sys.stdin)["annotations"]))
except Exception:
    print("")'; }
    check "scholar: a highlight remembers the printed page it is on" "$([ "$(labels)" = "41" ] && echo 1 || echo 0)" "(page labels: '$(labels)')"
    for _ in $(seq 8); do xdotool mousemove 600 400 click 5; done
    sleep 1
    xdotool mousemove 575 489 click 1
    sleep 1.5
    screenshot "$WORK/sc-note2.png"
    pop="$(python3 "$HERE/text_bands.py" "$WORK/sc-note2.png" --ink-in 400 510 900 560)"
    check "scholar: an unmarked superscript link to a note opens it too" "$([ "${pop:-0}" -gt 250 ] && echo 1 || echo 0)" "(ink in the popover: $pop)"
    xdotool key Escape
    sleep 0.5
    for _ in $(seq 8); do xdotool mousemove 600 400 click 4; done
    sleep 0.5
    for _ in $(seq 22); do xdotool mousemove 600 400 click 5; done
    sleep 2
    screenshot "$WORK/sc-1.png"
    local geom x0 y0 x1 y1
    geom="$(python3 "$HERE/text_bands.py" "$WORK/sc-1.png" --last-line)"
    read -r x0 y0 x1 y1 <<<"$geom"
    local y=$(( (y0 + y1) / 2 ))
    xdotool mousemove $((x0 - 4)) "$y" mousedown 1 mousemove $(( (x0 + x1) / 2 )) "$y" mousemove $((x1 + 8)) "$y" mouseup 1
    sleep 1
    xdotool key 2
    sleep 1
    local second
    second="$(labels | awk '{print $2}')"
    check "scholar: further on, a highlight is on a later printed page" "$([[ "$second" =~ ^4[2-5]$ ]] && echo 1 || echo 0)" "(page labels: '$(labels)')"
    stop_app
    local side
    side="$(find "$WORK/h/data" -path '*annotations*' -name '*.json' | head -1)"
    python3 - "$side" <<'PY'
import json, sys
path = sys.argv[1]
d = json.load(open(path))
a = d["annotations"][0]
words = a["snippet"].split(" ")
words[3] = words[3][:-1] + "x"
a["snippet"] = " ".join(words)
a.pop("snippet_prefix", None)
a.pop("snippet_suffix", None)
a.get("extra", {}).pop("pos", None)
a.pop("pos", None)
json.dump(d, open(path, "w"))
PY
    start_app "$FX/scholar.epub" keep
    reader_window >/dev/null
    sleep 4
    for _ in $(seq 60); do xdotool mousemove 600 400 click 4; done
    sleep 1.5
    screenshot "$WORK/sc-drift.png"
    local tint
    tint="$(python3 "$HERE/text_bands.py" "$WORK/sc-drift.png" --tint-in 285 190 968 215)"
    check "scholar: a mark whose words no longer match exactly is still found and drawn" "$([ "${tint:-0}" -gt 300 ] && echo 1 || echo 0)" "(tinted pixels on its line: $tint)"
    stop_app
}

same_region() { # same_region A B X Y W H -> 1 when that rectangle looks the same in both screenshots
    convert "$1" -crop "$5x$6+$3+$4" +repage "$WORK/_a.png" 2>/dev/null
    convert "$2" -crop "$5x$6+$3+$4" +repage "$WORK/_b.png" 2>/dev/null
    [ "$(compare -metric AE "$WORK/_a.png" "$WORK/_b.png" null: 2>&1 | awk '{print $1}')" = 0 ] && echo 1 || echo 0
}

run_pages_case() {
    start_app "$FX/scholar.epub"
    local w
    w="$(reader_window)" || { check "pages: reader window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 4
    xdotool windowfocus "$w" 2>/dev/null
    xdotool mousemove 47 791 click 1
    sleep 2
    screenshot "$WORK/pg-1.png"
    local spread
    spread="$(python3 "$HERE/text_bands.py" "$WORK/pg-1.png" --ink-in 90 780 140 802)"
    check "pages: the Pages toggle shows where you are (1 / 2)" "$([ "${spread:-0}" -gt 40 ] && echo 1 || echo 0)" "(ink in the spread label: $spread)"
    xdotool mousemove 600 400
    xdotool key space
    sleep 1.5
    screenshot "$WORK/pg-2.png"
    check "pages: Space turns to the next spread" "$([ "$(same_region "$WORK/pg-1.png" "$WORK/pg-2.png" 0 90 1240 660)" = 0 ] && [ "$(same_region "$WORK/pg-1.png" "$WORK/pg-2.png" 90 780 50 22)" = 0 ] && echo 1 || echo 0)"
    xdotool key space
    sleep 2.5
    screenshot "$WORK/pg-3.png"
    check "pages: past the last page it goes on to the next chapter, and the printed page moves on" "$([ "$(same_region "$WORK/pg-2.png" "$WORK/pg-3.png" 205 15 50 26)" = 0 ] && echo 1 || echo 0)"
    xdotool key shift+space
    sleep 2.5
    screenshot "$WORK/pg-4.png"
    check "pages: Shift+Space from the top of a chapter lands on the last page of the one before" "$([ "$(same_region "$WORK/pg-2.png" "$WORK/pg-4.png" 90 780 50 22)" = 1 ] && [ "$(same_region "$WORK/pg-2.png" "$WORK/pg-4.png" 205 15 50 26)" = 1 ] && echo 1 || echo 0)"
    stop_app
    start_app "$FX/scholar.epub" keep
    reader_window >/dev/null
    sleep 4
    screenshot "$WORK/pg-5.png"
    spread="$(python3 "$HERE/text_bands.py" "$WORK/pg-5.png" --ink-in 90 780 140 802)"
    check "pages: it reopens paginated" "$([ "${spread:-0}" -gt 40 ] && echo 1 || echo 0)" "(ink in the spread label: $spread)"
    stop_app
}

run_epubnotes_case() {
    start_app "$FX/scholar.epub"
    local w
    w="$(reader_window)" || { check "epub notebook: reader window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 4
    xdotool windowfocus "$w" 2>/dev/null
    xdotool mousemove 286 201 mousedown 1 mousemove 600 201 mousemove 968 201 mouseup 1
    sleep 1
    xdotool key 1
    sleep 1
    xdotool mousemove 1061 28 click 1
    sleep 1.5
    xdotool mousemove 1181 74 click 1
    sleep 1.5
    xdotool mousemove 115 791 click 1
    sleep 2
    screenshot "$WORK/en-1.png"
    local body
    body="$(cat "$(notebook_file)" 2>/dev/null)"
    check "epub notebook: + on a note adds it as a quote that cites the printed page" "$([[ "$body" == *'label: "41"'* && "$body" == *'covenant liturgy scripture'* && "$body" != *'chapter: true'* ]] && echo 1 || echo 0)" "(file: $(tail -c 250 "$(notebook_file)" 2>/dev/null))"
    local card
    card="$(python3 "$HERE/text_bands.py" "$WORK/en-1.png" --ink-in 960 100 1230 200)"
    check "epub notebook: the Notebook toggle shows it beside the book, with the quote card" "$([ "${card:-0}" -gt 200 ] && echo 1 || echo 0)" "(ink where the card is: $card)"
    stop_app
}

run_epubimage_case() {
    start_app "$FX/scholar.epub"
    local w
    w="$(reader_window)" || { check "epub image: reader window opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 4
    xdotool windowfocus "$w" 2>/dev/null
    for _ in $(seq 70); do xdotool mousemove 600 400 click 5; done
    sleep 1.5
    xdotool mousemove 369 722 click 3
    sleep 1.2
    xdotool mousemove 434 873 click 1
    sleep 1.5
    local got
    got="$(annotations_json | python3 -c 'import sys,json
try:
    a=json.load(sys.stdin)["annotations"][0]
    print(a["kind"], a["extra"].get("image") if "extra" in a else a.get("image"), a.get("page_label"))
except Exception as e:
    print("")')"
    check "epub image: Clip this image saves an area on the picture" "$([[ "$got" == "area OEBPS/fig.png "* ]] && echo 1 || echo 0)" "(got '$got')"
    screenshot "$WORK/ei-2.png"
    local ring
    ring="$(python3 "$HERE/text_bands.py" "$WORK/ei-2.png" --tint-in 300 676 440 690)"
    check "epub image: the clip knows the printed page it is on" "$([[ "$got" == "area OEBPS/fig.png 43" ]] && echo 1 || echo 0)" "(got '$got')"
    xdotool mousemove 541 28 click 1
    sleep 1.5
    xdotool mousemove 383 27 click 1
    sleep 2
    xdotool key ctrl+a
    xdotool type --delay 20 "$WORK/book-notes.typ"
    xdotool key Return
    sleep 2.5
    local out
    out="$(cat "$WORK/book-notes.typ" 2>/dev/null)"
    check "epub image: the export has the picture as a Typst figure, cited by printed page" "$([[ "$out" == *'#figure(image("book-notes-figures/'*'.png")'* && "$out" == *'p. 43'* ]] && echo 1 || echo 0)" "(got: $(head -c 300 "$WORK/book-notes.typ" 2>/dev/null))"
    check "epub image: the picture file is written beside it" "$([ -n "$(ls "$WORK"/book-notes-figures/*.png 2>/dev/null)" ] && echo 1 || echo 0)"
    stop_app
}

run_search_case() {
    start_app "$FX/plain.pdf"
    reader_window >/dev/null || { check "search: reader opens" 0; stop_app; return; }
    sleep 1
    stop_app
    start_app "$FX/scholar.epub" keep
    reader_window >/dev/null
    sleep 1
    stop_app
    start_app - keep
    local w
    w="$(launcher_window)" || { check "search: launcher opens" 0 "(log: $(head -c 300 "$WORK/app.log"))"; stop_app; return; }
    sleep 1.5
    xdotool windowfocus "$w" 2>/dev/null
    xdotool key ctrl+f
    sleep 6
    xdotool type --delay 40 "sphinx quartz"
    sleep 2.5
    screenshot "$WORK/se-0.png"
    local ink
    ink="$(python3 "$HERE/text_bands.py" "$WORK/se-0.png" --ink-in 20 175 700 340)"
    check "search: the text of documents read is searchable, ranked, with the words marked" "$([ "${ink:-0}" -gt 1500 ] && echo 1 || echo 0)" "(ink in the results: $ink)"
    xdotool key ctrl+a
    xdotool type --delay 40 "\"covenant liturgy\""
    sleep 2.5
    screenshot "$WORK/se-1.png"
    ink="$(python3 "$HERE/text_bands.py" "$WORK/se-1.png" --ink-in 20 140 700 340)"
    check "search: an EPUB's chapters and a quoted phrase work too" "$([ "${ink:-0}" -gt 800 ] && echo 1 || echo 0)" "(ink in the results: $ink)"
    xdotool key ctrl+a
    xdotool type --delay 40 "sphinx"
    sleep 2.5
    xdotool mousemove 360 200
    sleep 0.4
    xdotool mousedown 1
    sleep 0.15
    xdotool mouseup 1
    sleep 3
    local rw
    rw="$(reader_window)" || { check "search: a hit opens its document" 0; stop_app; return; }
    sleep 3.5
    screenshot "$WORK/se-2.png"
    local blue
    blue="$(python3 "$HERE/text_bands.py" "$WORK/se-2.png" --has-search-match)"
    check "search: opening a hit marks the words it was found by" "$([ "${blue:-0}" -gt 100 ] && echo 1 || echo 0)" "(blue pixels: $blue)"
    stop_app
}

run_browser_case() {
    start_app "$FX/plain.pdf"
    local w
    w="$(reader_window)" || { check "browser: reader opens" 0; stop_app; return; }
    sleep 2
    xdotool windowfocus "$w" 2>/dev/null
    screenshot "$WORK/br-0.png"
    local geom x0 y0 x1 y1
    geom="$(text_bands "$WORK/br-0.png")"
    read -r x0 y0 x1 y1 <<<"$geom"
    local h=$(( (y1 - y0) / 4 )) sy band
    for band in 1 3; do
        sy=$(( y0 + band * h + h / 2 ))
        xdotool mousemove $((x0 - 6)) "$sy" mousedown 1 mousemove $(( (x0 + x1) / 2 )) "$sy" mousemove $((x1 + 6)) "$sy" mouseup 1
        sleep 1
        xdotool key 1
        sleep 1
    done
    sleep 1
    stop_app
    start_app - keep
    w="$(launcher_window)" || { check "browser: launcher opens" 0; stop_app; return; }
    sleep 1.5
    xdotool windowfocus "$w" 2>/dev/null
    xdotool mousemove 264 28 click 1
    sleep 2
    screenshot "$WORK/br-1.png"
    xdotool mousemove 41 172 click 1
    sleep 1
    screenshot "$WORK/br-2.png"
    xdotool mousemove 267 115 click 1
    sleep 1
    xdotool type --delay 40 "method"
    xdotool key Return
    sleep 1.5
    local tags
    tags="$(annotations_json | python3 -c 'import sys,json
try:
    a=json.load(sys.stdin)["annotations"]
    print(" ".join(sorted(",".join(x.get("tags", [])) or "-" for x in a)))
except Exception:
    print("")')"
    check "browser: Add tag… tags the selected note and only that one" "$([ "$tags" = "- method" ] && echo 1 || echo 0)" "(tags: '$tags')"
    screenshot "$WORK/br-3.png"
    xdotool mousemove 230 75 click 1
    xdotool type --delay 40 "#method"
    sleep 1.5
    screenshot "$WORK/br-4.png"
    check "browser: #tag in the search narrows the list" "$([ "$(same_region "$WORK/br-3.png" "$WORK/br-4.png" 17 135 200 20)" = 0 ] && echo 1 || echo 0)"
    stop_app
}

run_palette_case() {
    start_app "$FX/plain.pdf"
    local w
    w="$(reader_window)" || { check "palette: reader opens" 0; stop_app; return; }
    sleep 2
    xdotool windowfocus "$w" 2>/dev/null
    screenshot "$WORK/pa-0.png"
    xdotool key ctrl+k
    sleep 1.2
    xdotool type --delay 40 "notebook"
    sleep 0.8
    screenshot "$WORK/pa-1.png"
    local ink
    ink="$(python3 "$HERE/text_bands.py" "$WORK/pa-1.png" --ink-in 20 60 540 95)"
    check "palette: Ctrl+K lists the commands that match what is typed" "$([ "${ink:-0}" -gt 150 ] && echo 1 || echo 0)" "(ink in the list: $ink)"
    xdotool key Return
    sleep 1.5
    screenshot "$WORK/pa-2.png"
    local card
    card="$(python3 "$HERE/text_bands.py" "$WORK/pa-2.png" --ink-in 640 60 990 120)"
    check "palette: Enter runs it (the Notebook opens beside the page)" "$([ "${card:-0}" -gt 100 ] && echo 1 || echo 0)" "(ink in the notebook header: $card)"
    xdotool key ctrl+k
    sleep 1
    xdotool type --delay 40 "3"
    sleep 0.8
    xdotool key Return
    sleep 1.5
    screenshot "$WORK/pa-3.png"
    check "palette: a number goes to that page" "$([ "$(same_region "$WORK/pa-2.png" "$WORK/pa-3.png" 50 770 160 40)" = 0 ] && echo 1 || echo 0)"
    stop_app
}

run_patterns_case() {
    local variant label
    for variant in off on; do
        rm -rf "$WORK/h"
        mkdir -p "$WORK/h/config/pereplyot"
        [ "$variant" = on ] && echo '{"patterns": true}' >"$WORK/h/config/pereplyot/config.json"
        start_app "$FX/plain.pdf" keep
        local w
        w="$(reader_window)" || { check "patterns: reader opens" 0; stop_app; return; }
        sleep 2
        xdotool windowfocus "$w" 2>/dev/null
        screenshot "$WORK/pt-base.png"
        local geom x0 y0 x1 y1
        geom="$(text_bands "$WORK/pt-base.png")"
        read -r x0 y0 x1 y1 <<<"$geom"
        local h=$(( (y1 - y0) / 4 )) sy
        sy=$(( y0 + 2 * h + h / 2 ))
        xdotool mousemove $((x0 - 6)) "$sy" mousedown 1 mousemove $(( (x0 + x1) / 2 )) "$sy" mousemove $((x1 + 6)) "$sy" mouseup 1
        sleep 1
        xdotool key 1
        sleep 1.5
        screenshot "$WORK/pt-$variant.png"
        stop_app
    done
    check "patterns: with textures on, a highlight is drawn with hatching as well as colour" "$([ "$(same_region "$WORK/pt-off.png" "$WORK/pt-on.png" 95 240 360 28)" = 0 ] && echo 1 || echo 0)"
}

run_caret_case() {
    start_app "$FX/plain.pdf"
    local w
    w="$(reader_window)" || { check "caret: reader opens" 0; stop_app; return; }
    sleep 2
    xdotool windowfocus "$w" 2>/dev/null
    xdotool key F7
    sleep 1
    screenshot "$WORK/ca-0.png"
    for _ in $(seq 6); do xdotool key shift+Right; done
    sleep 0.6
    screenshot "$WORK/ca-1.png"
    check "caret: Shift+arrows select on the page itself" "$([ "$(same_region "$WORK/ca-0.png" "$WORK/ca-1.png" 95 185 400 30)" = 0 ] && echo 1 || echo 0)"
    xdotool key 1
    sleep 1
    local got
    got="$(snippets)"
    check "caret: 1 marks what the caret selected" "$([[ "$got" == "Page 1"* ]] && echo 1 || echo 0)" "(got '$got')"
    xdotool key Down
    sleep 0.5
    xdotool key shift+End
    xdotool key 2
    sleep 1
    got="$(snippets)"
    check "caret: Down, then Shift+End, selects to the end of that line" "$([[ "$got" == *"Pack my box with five dozen liquor jugs"* ]] && echo 1 || echo 0)" "(got '$got')"
    stop_app
}

run_welcome_case() {
    WELCOME_OK=PEREPLYOT_WELCOME=1 start_app -
    local w
    w="$(launcher_window)" || { check "welcome: launcher opens" 0; stop_app; return; }
    sleep 2.5
    screenshot "$WORK/wl-0.png"
    local ink
    ink="$(python3 "$HERE/text_bands.py" "$WORK/wl-0.png" --ink-in 130 120 600 200)"
    check "welcome: the first run shows the Welcome window" "$([ "${ink:-0}" -gt 400 ] && echo 1 || echo 0)" "(ink in the window: $ink)"
    stop_app
    WELCOME_OK=PEREPLYOT_WELCOME=1 start_app - keep
    launcher_window >/dev/null
    sleep 2.5
    screenshot "$WORK/wl-1.png"
    check "welcome: it is not shown again for the same version" "$([ "$(same_region "$WORK/wl-0.png" "$WORK/wl-1.png" 0 0 720 680)" = 0 ] && echo 1 || echo 0)"
    stop_app
}

run_bubble_case() {
    start_app "$FX/monograph.pdf"
    local w
    w="$(reader_window)" || { check "bubble: reader opens" 0; stop_app; return; }
    sleep 2
    xdotool windowfocus "$w" 2>/dev/null
    xdotool key t
    sleep 3
    xdotool mousemove 110 205 mousedown 1 mousemove 400 205 mousemove 650 205 mouseup 1
    sleep 1
    xdotool key 1
    sleep 1
    screenshot "$WORK/bb-before.png"
    stop_app
    local side
    side="$(find "$WORK/h/data" -path '*annotations*' -name '*.json' | head -1)"
    python3 - "$side" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
d["annotations"][0]["note"] = "Check this against Jones"
json.dump(d, open(sys.argv[1], "w"))
PY
    start_app "$FX/monograph.pdf" keep
    reader_window >/dev/null
    sleep 4
    screenshot "$WORK/bb-after.png"
    check "bubble: a marked passage with a note has a bubble beside it in Reading mode" "$([ "$(same_region "$WORK/bb-before.png" "$WORK/bb-after.png" 90 190 760 40)" = 0 ] && echo 1 || echo 0)"
    stop_app
}

# Line order in the fixtures: "Page N", fox, Pack, Sphinx.
want() { [ -z "${SMOKE_CASES:-}" ] || [[ " $SMOKE_CASES " == *" $1 "* ]]; }
want plain && run_pdf_case plain plain.pdf y 2 "Pack my box with five dozen liquor jugs."
want cropbox && run_pdf_case cropbox cropbox.pdf y 2 "Pack my box with five dozen liquor jugs."
want rotated && run_pdf_case rotated rotated.pdf y 2 "Pack my box with five dozen liquor jugs."
want history && run_history_case
want split && run_split_case
want resume && run_resume_case
want pin && run_pin_case
want hover && run_hover_case
want thumbs && run_thumbs_case
want reading && run_reading_case
want figures && run_figures_case
want area && run_area_case
want import && run_import_case
want epub && run_epub_case
want mixed && run_mixed_case
want notebook && run_notebook_case
want connect && run_connect_case
want scholar && run_scholar_case
want pages && run_pages_case
want epubnotes && run_epubnotes_case
want epubimage && run_epubimage_case
want launcher && run_launcher_case
want search && run_search_case
want browser && run_browser_case
want palette && run_palette_case
want welcome && run_welcome_case
want caret && run_caret_case
want bubble && run_bubble_case
want patterns && run_patterns_case

echo
if [ "$FAILS" -eq 0 ]; then echo "smoke: all passed"; else echo "smoke: $FAILS failed"; exit 1; fi
