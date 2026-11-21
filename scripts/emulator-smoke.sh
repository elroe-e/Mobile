#!/usr/bin/env bash
# Installs Loss on a running emulator (or any adb device), launches it and checks that it starts
# cleanly. Used by CI (M0-03); also works against a phone with USB debugging.
#
#   scripts/emulator-smoke.sh path/to/app.apk [out-dir]
#
# Writes screenshot.png, logcat.txt, media_session.txt and processes.txt to out-dir
# (default target/emulator-smoke). Fails on a Java FATAL EXCEPTION, a Rust panic, a native crash,
# if the app isn't running after the wait, or if the Rust↔Kotlin bridge didn't start.
set -uo pipefail

APK="${1:?usage: emulator-smoke.sh app.apk [out-dir]}"
OUT="${2:-target/emulator-smoke}"
PKG=app.loss.player
ACTIVITY=dev.dioxus.main.MainActivity
WAIT="${SMOKE_WAIT:-20}"
mkdir -p "$OUT"

adb wait-for-device
# Wait for the system to finish booting (CI boots it fresh).
for _ in $(seq 1 120); do
  [ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = "1" ] && break
  sleep 2
done

adb uninstall "$PKG" > /dev/null 2>&1 || true
# -g grants the runtime permissions (music files, notifications) so no prompt covers Home.
adb install -r -g "$APK" || { echo "install failed"; exit 1; }

adb logcat -c
adb shell am start -W -n "$PKG/$ACTIVITY"
sleep "$WAIT"

adb exec-out screencap -p > "$OUT/screenshot.png"
adb logcat -d -v time > "$OUT/logcat.txt"
adb shell dumpsys media_session > "$OUT/media_session.txt"
adb shell ps -A > "$OUT/processes.txt"

fail=0
if grep -q "FATAL EXCEPTION" "$OUT/logcat.txt"; then
  echo "::error::Java crash (FATAL EXCEPTION) in logcat"
  fail=1
fi
if grep -Eq "panicked at|Rust panic" "$OUT/logcat.txt"; then
  echo "::error::Rust panic in logcat"
  fail=1
fi
if grep -Eq "Fatal signal [0-9]+ .*\($PKG|>>> $PKG <<<" "$OUT/logcat.txt"; then
  echo "::error::native crash in logcat"
  fail=1
fi
if ! adb shell pidof "$PKG" > /dev/null; then
  echo "::error::Loss is not running after ${WAIT}s"
  fail=1
fi
if ! grep -q "platform bridge ready" "$OUT/logcat.txt"; then
  echo "::error::Rust didn't register its native methods (no 'platform bridge ready' line)"
  fail=1
fi
if ! grep -q "Loss .* starting" "$OUT/logcat.txt"; then
  echo "::error::Rust side never logged its start line"
  fail=1
fi

grep -E "FATAL|panicked|Fatal signal|\bLoss\b|loss::" "$OUT/logcat.txt" | tail -40
[ "$fail" = 0 ] && echo "Smoke test passed: Loss is running, screenshot in $OUT/screenshot.png"
exit "$fail"
