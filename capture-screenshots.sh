#!/usr/bin/env bash
# capture-screenshots.sh — refresh screenshots/pereplyot-*.png (light and dark) from a throwaway
# profile and an obviously fictional document ("Example Monograph"), never your own data.
# Needs a built target/release/pereplyot (cargo build --release), Xvfb, xdotool and ImageMagick.
# If ../calstfrancis.github.io exists the WebP copies are dropped into its images/ too.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="${1:-$HERE/target/release/pereplyot}"
OUT="$HERE/screenshots"
WEBSITE_DIR="${WEBSITE_DIR:-$HERE/../calstfrancis.github.io}"
WORK="$(mktemp -d)"
: "${PDFIUM_LIB_PATH:=$HERE/build-flatpak/files/lib}"
export PDFIUM_LIB_PATH
[ -x "$BIN" ] || { echo "build first: cargo build --release (no binary at $BIN)"; exit 2; }
for tool in Xvfb xdotool import convert python3 dbus-run-session; do
    command -v "$tool" >/dev/null || { echo "missing tool: $tool"; exit 2; }
done
mkdir -p "$OUT"
cleanup() { [ -n "${PGID:-}" ] && kill -TERM -- "-$PGID" 2>/dev/null; [ -n "${XVFB_PID:-}" ] && kill "$XVFB_PID" 2>/dev/null; rm -rf "$WORK"; }
trap cleanup EXIT

cat >"$WORK/session.conf" <<'CONF'
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig><type>session</type><keep_umask/><listen>unix:tmpdir=/tmp</listen>
<policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>
CONF
Xvfb -displayfd 3 -screen 0 1400x900x24 3>"$WORK/display" >/dev/null 2>&1 &
XVFB_PID=$!
for _ in $(seq 50); do [ -s "$WORK/display" ] && break; sleep 0.1; done
export DISPLAY=":$(tr -d '\n' <"$WORK/display")"

# The fictional document: obviously fake title, filler text.
cp "$HERE/tests/fixtures/monograph.pdf" "$WORK/Example Monograph (fictional).pdf"
DOC="$WORK/Example Monograph (fictional).pdf"

start_app() {
    local scheme="$1"; shift
    [ -n "${KEEP_PROFILE:-}" ] || rm -rf "$WORK/h"
    mkdir -p "$WORK/h"/{home,data,config/glib-2.0/settings,cache,data/fonts}
    cp "$HERE"/packaging/fonts/*.otf "$WORK/h/data/fonts/" 2>/dev/null
    mkdir -p "$WORK/h/config/glib-2.0/settings"
    printf "[org/gnome/desktop/interface]\ncolor-scheme='%s'\n" "$scheme" >"$WORK/h/config/glib-2.0/settings/keyfile"
    env -u WAYLAND_DISPLAY GDK_BACKEND=x11 GTK_A11Y=none ADW_DISABLE_PORTAL=1 GSETTINGS_BACKEND=keyfile \
        PEREPLYOT_NO_WELCOME=1 WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 WEBKIT_DISABLE_COMPOSITING_MODE=1 \
        HOME="$WORK/h/home" XDG_DATA_HOME="$WORK/h/data" XDG_CONFIG_HOME="$WORK/h/config" XDG_CACHE_HOME="$WORK/h/cache" \
        setsid dbus-run-session --config-file="$WORK/session.conf" -- "$BIN" "$@" >"$WORK/app.log" 2>&1 </dev/null &
    PGID=$!
}
stop_app() { [ -n "${PGID:-}" ] && { kill -TERM -- "-$PGID" 2>/dev/null; sleep 0.4; kill -KILL -- "-$PGID" 2>/dev/null; PGID=""; }; }
window_named() { # window_named PATTERN -> id
    for _ in $(seq 100); do
        for w in $(xdotool search --onlyvisible --name '' 2>/dev/null); do
            case "$(xdotool getwindowname "$w" 2>/dev/null)" in $1) echo "$w"; return 0 ;; esac
        done
        sleep 0.2
    done
    return 1
}
shot() { # shot WINDOW FILE  — the window's own rectangle
    local g x y w h
    g="$(xdotool getwindowgeometry --shell "$1")"
    eval "$g"
    import -window root -crop "${WIDTH}x${HEIGHT}+${X}+${Y}" +repage "$2" 2>/dev/null
}

for scheme in ${SCHEMES:-default prefer-dark}; do
    suffix=""; [ "$scheme" = prefer-dark ] && suffix="-dark"

    # --- the reader, in the Synthesise posture: marks, notes list and notebook ---
    start_app "$scheme" "$DOC"
    w="$(window_named 'Pereplyot*')" || { echo "no reader window"; exit 1; }
    sleep 6
    xdotool windowfocus "$w" 2>/dev/null
    x0=186; x1=680
    h=117
    for band in 0 1 2; do
        sy=$(( 337 + band * h ))
        xdotool mousemove $((x0 - 6)) "$sy" mousedown 1 mousemove $(( (x0 + x1) / 2 )) "$sy" mousemove $((x1 + 6)) "$sy" mouseup 1
        sleep 0.8
        xdotool key $((band + 1))
        sleep 0.8
    done
    xdotool key n
    sleep 1.5
    xdotool mousemove 738 28 click 1
    sleep 1.5
    xdotool mousemove 546 160 click 1
    sleep 1
    xdotool mousemove 546 347 click 1
    sleep 1.5
    xdotool mousemove 738 28 click 1
    sleep 1.5
    xdotool mousemove 800 600
    sleep 28
    shot "$w" "$OUT/pereplyot-main$suffix.png"
    stop_app

    # --- the launcher's Search tab, over the same document ---
    start_app "$scheme" "$DOC"
    window_named 'Pereplyot*' >/dev/null; sleep 2; stop_app
    KEEP_PROFILE=1 start_app "$scheme"
    l="$(window_named 'Pereplyot')" || { echo "no launcher"; exit 1; }
    sleep 1.5
    xdotool windowfocus "$l" 2>/dev/null
    xdotool key ctrl+f
    sleep 6
    xdotool type --delay 40 "interpretation"
    sleep 3
    shot "$l" "$OUT/pereplyot-search$suffix.png"
    stop_app
done

echo "wrote $(ls "$OUT"/pereplyot-*.png | wc -l) screenshots to $OUT"
if [ -d "$WEBSITE_DIR/images" ]; then
    for f in "$OUT"/pereplyot-main.png "$OUT"/pereplyot-main-dark.png; do
        cp "$f" "$WEBSITE_DIR/images/"
        convert "$f" -quality 80 "$WEBSITE_DIR/images/$(basename "${f%.png}").webp"
    done
    echo "also copied to $WEBSITE_DIR/images (review and commit there yourself)"
fi
