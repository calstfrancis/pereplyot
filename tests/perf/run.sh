#!/usr/bin/env bash
# Measure the performance budgets on large synthetic documents (see make_docs.py).
# Usage: tests/perf/run.sh [DOCDIR] [path/to/pereplyot]   (default DOCDIR ~/.cache/pereplyot-perf; release binary)
# Needs PDFIUM_LIB_PATH, Xvfb, xdotool, dbus-run-session. Output: one report block per document.
# Runs GTK with the Cairo renderer: the default GL renderer is software-emulated under Xvfb and its
# first-frame and shader work shows up as stalls that have nothing to do with this code.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
DOCS="${1:-$HOME/.cache/pereplyot-perf}"
BIN="${2:-$ROOT/target/release/pereplyot}"
WORK="$(mktemp -d)"
APP_PGID=""
cleanup() { stop_app; [ -n "${XVFB_PID:-}" ] && kill "$XVFB_PID" 2>/dev/null; rm -rf "$WORK"; }
trap cleanup EXIT
: "${PDFIUM_LIB_PATH:?set PDFIUM_LIB_PATH}"
[ -x "$BIN" ] || { echo "no binary at $BIN"; exit 2; }
[ -f "$DOCS/paper-50.pdf" ] || python3 "$HERE/make_docs.py" "$DOCS" >/dev/null

cat >"$WORK/session.conf" <<'CONF'
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig><type>session</type><keep_umask/><listen>unix:tmpdir=/tmp</listen>
<policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>
CONF
Xvfb -displayfd 3 -screen 0 1400x900x24 3>"$WORK/display" >/dev/null 2>&1 &
XVFB_PID=$!
for _ in $(seq 50); do [ -s "$WORK/display" ] && break; sleep 0.1; done
export DISPLAY=":$(tr -d '\n' <"$WORK/display")"

stop_app() {
    [ -n "$APP_PGID" ] || return 0
    kill -TERM -- "-$APP_PGID" 2>/dev/null; sleep 0.3; kill -KILL -- "-$APP_PGID" 2>/dev/null; APP_PGID=""
}

app_pid() { pgrep -f "^$BIN " | head -1; }
rss_mb() { local p; p="$(app_pid)"; [ -n "$p" ] && awk '/VmRSS/{printf "%.0f", $2/1024}' "/proc/$p/status"; }
peak_mb() { local p; p="$(app_pid)"; [ -n "$p" ] && awk '/VmHWM/{printf "%.0f", $2/1024}' "/proc/$p/status"; }

stalls_of() { # stalls_of LOG FROMLINE -> "n max" of main-thread stalls (>20 ms) after that line
    tail -n +"$(( $2 + 1 ))" "$1" | grep -E ' stall [0-9]+ ms' | sed -E 's/.* stall ([0-9]+) ms/\1/' | sort -n | awk '{a[NR]=$1} END{if(NR==0)print "0 -"; else printf "%d %d\n", NR, a[NR]}'
}

stat_of() { # stat_of LOG REGEX  -> count median max (ms of "took")
    grep -E "$2" "$1" | sed -E 's/.* took ([0-9.]+) ms/\1/' | sort -n | awk '{a[NR]=$1} END{if(NR==0){print "0 - -"} else printf "%d %.1f %.1f\n", NR, a[int((NR+1)/2)], a[NR]}'
}

run_doc() {
    local doc="$1" search="$2"
    local log="$WORK/$doc.log"
    rm -rf "$WORK/h"; mkdir -p "$WORK/h"/{home,data,config,cache}
    env -u WAYLAND_DISPLAY GDK_BACKEND=x11 GTK_A11Y=none GSETTINGS_BACKEND=memory PEREPLYOT_PERF=1 GSK_RENDERER=cairo \
        HOME="$WORK/h/home" XDG_DATA_HOME="$WORK/h/data" XDG_CONFIG_HOME="$WORK/h/config" XDG_CACHE_HOME="$WORK/h/cache" \
        setsid dbus-run-session --config-file="$WORK/session.conf" -- "$BIN" "$DOCS/$doc.pdf" >"$log" 2>&1 </dev/null &
    APP_PGID=$!
    for _ in $(seq 150); do grep -q "pdf reader presented" "$log" 2>/dev/null && break; sleep 0.2; done
    sleep 3
    local w; w="$(xdotool search --onlyvisible --name 'Reader' | head -1)"
    local rss_open; rss_open="$(rss_mb)"
    local open_ms first_ms
    open_ms="$(grep 'open_pdf took' "$log" | head -1 | sed -E 's/.* took ([0-9.]+) ms/\1/')"
    first_ms="$(grep 'presented' "$log" | head -1 | awk '{print $2}')"
    # page through: 40 page-downs, spaced so idle rendering can interleave
    xdotool windowfocus "$w" 2>/dev/null
    local mark; mark="$(wc -l <"$log")"
    local open_stalls; open_stalls="$(stalls_of "$log" 0)"
    for _ in $(seq 40); do xdotool key Next; sleep 0.12; done
    sleep 2
    stat_of "$log" 'render page' >"$WORK/render.stat"
    local scroll_renders; scroll_renders="$(tail -n +"$((mark + 1))" "$log" | grep -E 'render page' | sed -E 's/.* took ([0-9.]+) ms/\1/' | sort -n | awk '{a[NR]=$1} END{if(NR==0)print "0 - -"; else printf "%d %.1f %.1f\n", NR, a[int((NR+1)/2)], a[NR]}')"
    local page_stalls; page_stalls="$(stalls_of "$log" "$mark")"
    local rss_scroll; rss_scroll="$(rss_mb)"
    # search
    mark="$(wc -l <"$log")"
    xdotool key ctrl+f; sleep 0.4; xdotool type --delay 20 "$search"; xdotool key Return
    for _ in $(seq 100); do tail -n +"$((mark + 1))" "$log" | grep -q 'search .* took' && break; sleep 0.3; done
    local search_stalls; search_stalls="$(stalls_of "$log" "$mark")"
    local search_ms; search_ms="$(tail -n +"$((mark + 1))" "$log" | grep 'search .* took' | head -1 | sed -E 's/.* took ([0-9.]+) ms/\1/')"
    sleep 1
    # zoom ~2x (stands in for a HiDPI render: four times the pixels), then page through again
    mark="$(wc -l <"$log")"
    xdotool key Escape; sleep 0.3
    for _ in $(seq 3); do xdotool key ctrl+equal; sleep 0.4; done
    sleep 1.5
    for _ in $(seq 20); do xdotool key Next; sleep 0.15; done
    sleep 2
    local zoom_mark="$mark"
    local zoom_renders; zoom_renders="$(tail -n +"$((mark + 1))" "$log" | grep -E 'render page' | sed -E 's/.* took ([0-9.]+) ms/\1/' | sort -n | awk '{a[NR]=$1} END{if(NR==0)print "0 - -"; else printf "%d %.1f %.1f\n", NR, a[int((NR+1)/2)], a[NR]}')"
    local zoom_stalls; zoom_stalls="$(stalls_of "$log" "$zoom_mark")"
    local peak; peak="$(peak_mb)"
    local first_render; first_render="$(grep -m1 'render page' "$log" | sed -E 's/.* took ([0-9.]+) ms/\1/')"
    local file_mb; file_mb="$(du -m "$DOCS/$doc.pdf" | cut -f1)"
    printf '%-12s file %4s MB | open_pdf %8s ms | window up at %8s ms | first page render %7s ms\n' "$doc" "$file_mb" "${open_ms:-?}" "${first_ms:-?}" "${first_render:-?}"
    printf '%-12s renders while paging: n/median/max = %s ms | search %s: %s ms (main thread)\n' "" "$scroll_renders" "\"$search\"" "${search_ms:-?}"
    printf '%-12s main-thread stalls >20ms (count/worst ms): open %s | paging %s | search %s | zoom %s\n' "" "$open_stalls" "$page_stalls" "$search_stalls" "$zoom_stalls"
    printf '%-12s renders at ~2x zoom:    n/median/max = %s ms\n' "" "$zoom_renders"
    printf '%-12s memory: after open %s MB, after paging %s MB, peak %s MB\n\n' "" "${rss_open:-?}" "${rss_scroll:-?}" "${peak:-?}"
    stop_app
}

echo "binary: $BIN"; echo
run_doc paper-50 covenant
run_doc book-600 covenant
run_doc scan-600 covenant
