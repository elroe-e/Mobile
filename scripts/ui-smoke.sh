#!/usr/bin/env bash
# Scripted tour of the desktop app with screenshots (M0-12). Runs under Xvfb; no phone needed.
#
#   scripts/ui-smoke.sh [locales...]      # default: en en_XA ar_XB
#
# Uses the fixtures from scripts/make-fixtures.sh as the music library, plays a song, visits
# every tab, the player, settings search and Back, and saves screenshots to
# target/ui-smoke/<locale>/. Fails if the app dies, panics or writes a crash report.
# Needs: Xvfb, xdotool, ImageMagick (import); PulseAudio is used when present (a null sink, so
# playback runs in real time without a sound card).
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/target/ui-smoke"
BIN="$ROOT/target/debug/loss"
LOCALES=("${@:-en en_XA ar_XB}")
read -r -a LOCALES <<< "${LOCALES[*]}"
W=412
H=880
export DISPLAY="${DISPLAY_SMOKE:-:97}"
# CI runners (Ubuntu 24.04) block the user namespaces WebKitGTK's sandbox needs, and have no GPU:
# without these the window stays black. Test runs only; the app itself doesn't set them.
export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export LIBGL_ALWAYS_SOFTWARE=1

cd "$ROOT"
[ -d target/fixtures ] || bash scripts/make-fixtures.sh > /dev/null
cargo build -q -p loss --no-default-features --features desktop || exit 1
rm -rf "$OUT"
mkdir -p "$OUT"

Xvfb "$DISPLAY" -screen 0 ${W}x${H}x24 > /dev/null 2>&1 &
XVFB=$!
trap 'kill $XVFB 2>/dev/null; pkill -f "$BIN" 2>/dev/null' EXIT
sleep 1
if command -v pulseaudio > /dev/null; then
  pulseaudio --check 2> /dev/null || pulseaudio --start --exit-idle-time=-1 > /dev/null 2>&1
  pactl load-module module-null-sink sink_name=smoke > /dev/null 2>&1 || true
  pactl set-default-sink smoke > /dev/null 2>&1 || true
  # The app runs with its own HOME, where PulseAudio clients look for the server and cookie.
  PULSE_SERVER="$(pactl info 2> /dev/null | sed -n 's/^Server String: //p')"
  [ -n "$PULSE_SERVER" ] && export PULSE_SERVER="unix:$PULSE_SERVER"
  [ -f "$HOME/.config/pulse/cookie" ] && export PULSE_COOKIE="$HOME/.config/pulse/cookie"
fi

fail=0
for loc in "${LOCALES[@]}"; do
  dir="$OUT/$loc"
  home="$dir/home"
  mkdir -p "$home/Music"
  cp -r target/fixtures/. "$home/Music/"
  rm -rf "$home/Music/gapless"   # keep the library small and readable
  n=0
  shot() {
    n=$((n + 1))
    sleep "${2:-1.2}"
    local f="$dir/$(printf %02d $n)-$1.png"
    import -window root "$f"
    # A screenshot of one flat colour means nothing was drawn.
    if [ "$(convert "$f" -format '%[fx:standard_deviation>0.01]' info: 2> /dev/null)" != "1" ]; then
      echo "::error::[$loc] screenshot $(basename "$f") is blank"
      fail=1
    fi
  }
  # Bottom-bar tab positions; mirrored in right-to-left languages.
  tabs=(44 125 206 287 367)
  [ "$loc" = "ar_XB" ] && tabs=(367 287 206 125 44)
  tab() { xdotool mousemove "${tabs[$1]}" 840 click 1; }

  LANG="$loc.UTF-8" LC_ALL="$loc.UTF-8" HOME="$home" LOSS_DATA_DIR="$dir/data" \
    "$BIN" > "$dir/stdout.txt" 2>&1 &
  app=$!
  for _ in $(seq 1 60); do
    win=$(xdotool search --name '^Loss$' 2> /dev/null | head -1)
    [ -n "$win" ] && break
    sleep 0.5
  done
  if [ -z "${win:-}" ]; then
    echo "::error::[$loc] the window never appeared"
    fail=1
    continue
  fi
  xdotool windowsize "$win" $W $H windowmove "$win" 0 0
  # The app is up once its services log their start line (the page has run).
  # CI's software rendering needs about 25 s for the first page.
  for _ in $(seq 1 180); do
    grep -qs "Loss .* starting; data in" "$dir"/data/logs/loss.log && break
    sleep 0.5
  done
  if ! grep -qs "Loss .* starting; data in" "$dir"/data/logs/loss.log; then
    echo "::error::[$loc] the app never finished starting (no services line in the log)"
    tail -20 "$dir/stdout.txt"
    fail=1
  fi
  # Then wait until something is on screen.
  for _ in $(seq 1 60); do
    import -window root "$dir/.probe.png"
    [ "$(convert "$dir/.probe.png" -format '%[fx:standard_deviation>0.01]' info: 2> /dev/null)" = "1" ] && break
    sleep 0.5
  done
  rm -f "$dir/.probe.png"
  sleep 3 # first scan of the fixtures

  shot home
  # Shuffle the library (first shortcut tile; mirrored in RTL).
  if [ "$loc" = "ar_XB" ]; then xdotool mousemove 304 268 click 1; else xdotool mousemove 108 268 click 1; fi
  shot playing 2
  tab 1; shot library
  xdotool mousemove 206 250 click 1; shot library-albums
  # Songs (the virtualized list), then a long press (right click) on the first song.
  if [ "$loc" = "ar_XB" ]; then xdotool mousemove 347 210 click 1; else xdotool mousemove 65 210 click 1; fi
  shot library-songs
  xdotool mousemove 206 300 click 3; shot song-menu
  xdotool key Escape; sleep 0.5
  tab 2; shot discover
  tab 3; shot downloads
  tab 4; shot settings
  xdotool mousemove 206 145 click 1
  xdotool type --delay 40 "dark"; shot settings-search
  xdotool key ctrl+a BackSpace
  # Open the player from the mini player, then Escape (Back) closes it.
  xdotool mousemove 206 765 click 1; shot now-playing 1.5
  # The quality label opens the audio-info sheet; Back closes it first, the player next.
  xdotool mousemove 206 664 click 1; shot audio-info 1
  xdotool key Escape; sleep 0.5
  xdotool key Escape; shot back

  if grep -q "cannot play" "$dir"/data/logs/loss.log; then
    echo "::warning::[$loc] playback failed: $(grep -h 'cannot play' "$dir"/data/logs/loss.log | tail -1)"
  fi
  if ! kill -0 $app 2> /dev/null; then
    echo "::error::[$loc] Loss exited during the tour"
    fail=1
  fi
  kill $app 2> /dev/null
  wait $app 2> /dev/null
  if grep -q "panicked" "$dir/stdout.txt" "$dir"/data/logs/*.log 2> /dev/null; then
    echo "::error::[$loc] Rust panic:"
    grep -h -A3 "panicked" "$dir/stdout.txt" "$dir"/data/logs/*.log | head -20
    fail=1
  fi
  if ls "$dir"/data/logs/crashes/crash-*.txt > /dev/null 2>&1; then
    echo "::error::[$loc] crash report written"
    fail=1
  fi
  echo "[$loc] $n screenshots; $(grep -ho 'scan: ScanReport { added: [0-9]*' "$dir"/data/logs/loss.log | tail -1)"

  # Component gallery (debug builds): sheet, dialog, long-press menu, the 10,000-row list.
  LOSS_DEBUG_PAGE=gallery LANG="$loc.UTF-8" LC_ALL="$loc.UTF-8" HOME="$home" LOSS_DATA_DIR="$dir/data" \
    "$BIN" > "$dir/stdout-gallery.txt" 2>&1 &
  app=$!
  for _ in $(seq 1 180); do
    win=$(xdotool search --name '^Loss$' 2> /dev/null | head -1)
    [ -n "$win" ] && break
    sleep 0.5
  done
  xdotool windowsize "$win" $W $H windowmove "$win" 0 0
  for _ in $(seq 1 120); do
    import -window root "$dir/.probe.png"
    [ "$(convert "$dir/.probe.png" -format '%[fx:standard_deviation>0.01]' info: 2> /dev/null)" = "1" ] && break
    sleep 0.5
  done
  rm -f "$dir/.probe.png"
  sleep 1
  shot gallery
  # Buttons sit on the other side in right-to-left layouts.
  if [ "$loc" = "ar_XB" ]; then x_sheet=315; x_dialog=166; else x_sheet=97; x_dialog=246; fi
  xdotool mousemove $x_sheet 570 click 1; shot gallery-sheet
  xdotool key Escape; sleep 0.5
  xdotool mousemove $x_dialog 570 click 1; shot gallery-dialog
  xdotool key Escape; sleep 0.5
  xdotool mousemove 206 760 click 3; shot gallery-long-press
  xdotool key Escape; sleep 0.5
  xdotool key End; shot gallery-list-end 2
  if ! kill -0 $app 2> /dev/null; then
    echo "::error::[$loc] Loss exited in the gallery"
    fail=1
  fi
  kill $app 2> /dev/null
  wait $app 2> /dev/null
  rm -rf "$home"
done

[ "$fail" = 0 ] && echo "UI smoke test passed: screenshots in target/ui-smoke/"
exit $fail
