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
    import -window root "$dir/$(printf %02d $n)-$1.png"
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
  sleep 4 # first scan of the fixtures

  shot home
  # Shuffle the library (first shortcut tile; mirrored in RTL).
  if [ "$loc" = "ar_XB" ]; then xdotool mousemove 304 268 click 1; else xdotool mousemove 108 268 click 1; fi
  shot playing 2
  tab 1; shot library
  xdotool mousemove 206 250 click 1; shot library-albums
  tab 2; shot discover
  tab 3; shot downloads
  tab 4; shot settings
  xdotool mousemove 206 145 click 1
  xdotool type --delay 40 "dark"; shot settings-search
  xdotool key ctrl+a BackSpace
  # Open the player from the mini player, then Escape (Back) closes it.
  xdotool mousemove 206 765 click 1; shot now-playing 1.5
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
  rm -rf "$home"
done

[ "$fail" = 0 ] && echo "UI smoke test passed: screenshots in target/ui-smoke/"
exit $fail
