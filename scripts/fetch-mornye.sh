#!/usr/bin/env bash
# Fetches the Mornye reference app (what Loss reaches parity with) at the pinned commit
# into a scratch folder. Never copy its code into Loss: read it to understand behaviour.
#   scripts/fetch-mornye.sh [destination]   (default: $TMPDIR/mornye-ref)
set -euo pipefail
DEST="${1:-${TMPDIR:-/tmp}/mornye-ref}"
COMMIT="c4a1c7b812eb135d6e1a295b6b25e11abbd72a7e"   # Mornye 5.4.0, 19 Nov 2026
if [ ! -d "$DEST/.git" ]; then
  git init -q "$DEST"
  git -C "$DEST" remote add origin https://github.com/mornyeapp/Mornye.git
fi
git -C "$DEST" fetch -q --depth 1 origin "$COMMIT"
git -C "$DEST" checkout -q FETCH_HEAD
echo "Mornye reference at $DEST ($(git -C "$DEST" log -1 --format='%h %cd' --date=short))"
echo "UI text:  $DEST/lib/l10n/arb/app_en.arb"
echo "Settings: $DEST/lib/models/settings.dart"
