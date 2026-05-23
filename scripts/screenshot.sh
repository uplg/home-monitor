#!/usr/bin/env bash
# Clean screenshot of the app via headless Chrome — no extra deps.
#
# Chrome's `--screenshot` captures exactly the `--window-size` region (one
# viewport, NOT a stitched full page), so the viewport-fixed background
# gradient (frontend/src/index.css) renders correctly. Make the window tall
# enough to fit the whole page and you get the full content in one clean frame.
#
# Auth is cookie-based, so we use a dedicated, persistent Chrome profile:
#   1) `screenshot.sh login`  -> opens a normal Chrome window, you log in once
#   2) `screenshot.sh`         -> headless capture reusing those cookies
#
# Usage:
#   scripts/screenshot.sh login                 # one-time: log in
#   scripts/screenshot.sh                        # capture with defaults
#   scripts/screenshot.sh --height 2600          # taller page
#   scripts/screenshot.sh --url https://home.kahn.studio/tempo-predictions \
#                         --out screenshots/tempo.jpg
#
# Output is a JPG (default screenshots/maison.jpg). When the target file is
# referenced in README.md, its image URL gets a fresh `?v=<timestamp>` so
# GitHub busts its image cache and shows the new screenshot.
#
# Flags: --url --width --height --out --scale --quality --no-readme
set -euo pipefail

CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="$REPO/scripts/.shot-profile"

URL="https://home.kahn.studio/"
WIDTH=1280
HEIGHT=2200
SCALE=2
QUALITY=90
OUT="$REPO/screenshots/maison.jpg"
BUMP_README=1

# `login` subcommand: open a real window so you can authenticate once.
if [[ "${1:-}" == "login" ]]; then
  shift
  echo "Opening Chrome to log in. Sign in, then close the window."
  exec "$CHROME" --user-data-dir="$PROFILE" --no-first-run \
    --no-default-browser-check "${1:-$URL}"
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --url)    URL="$2";    shift 2 ;;
    --width)  WIDTH="$2";  shift 2 ;;
    --height) HEIGHT="$2"; shift 2 ;;
    --scale)   SCALE="$2";   shift 2 ;;
    --quality) QUALITY="$2"; shift 2 ;;
    --out)     OUT="$2";     shift 2 ;;
    --no-readme) BUMP_README=0; shift ;;
    *) echo "Unknown flag: $1" >&2; exit 1 ;;
  esac
done

if [[ ! -d "$PROFILE" ]]; then
  echo "No saved login profile yet. Run:  scripts/screenshot.sh login" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUT")"

# Chrome writes a PNG; we convert to JPG afterwards.
PNG="${TMPDIR:-/tmp}/cat-monitor-shot-$$.png"
trap 'rm -f "$PNG"' EXIT
rm -f "$PNG"

"$CHROME" \
  --headless=new \
  --user-data-dir="$PROFILE" \
  --no-first-run --no-default-browser-check \
  --disable-background-networking --disable-component-update \
  --disable-sync --disable-features=Translate \
  --hide-scrollbars \
  --force-device-scale-factor="$SCALE" \
  --window-size="${WIDTH},${HEIGHT}" \
  --virtual-time-budget=4000 \
  --screenshot="$PNG" \
  "$URL" >/dev/null 2>&1 &
CHROME_PID=$!

# Watchdog: never hang — kill Chrome if it overstays.
( sleep 30; kill -9 "$CHROME_PID" 2>/dev/null ) &
WATCHDOG_PID=$!
wait "$CHROME_PID" 2>/dev/null || true
kill "$WATCHDOG_PID" 2>/dev/null || true

if [[ ! -s "$PNG" ]]; then
  echo "Capture failed (no image produced)." >&2
  exit 1
fi

# PNG -> JPG (built-in `sips`, no deps).
sips -s format jpeg -s formatOptions "$QUALITY" "$PNG" --out "$OUT" >/dev/null
echo "Saved -> $OUT  (${WIDTH}x${HEIGHT} @${SCALE}x, q${QUALITY})"

# Cache-bust: refresh `?v=<timestamp>` on this image's URL in README.md so
# GitHub stops serving the stale cached version.
README="$REPO/README.md"
REL="${OUT#"$REPO"}"   # e.g. /screenshots/maison.jpg
if [[ "$BUMP_README" == 1 && -f "$README" ]] && grep -q "$REL" "$README"; then
  TS="$(date +%s)"
  perl -i -pe "s{\Q$REL\E(\?v=\d+)?}{$REL?v=$TS}g" "$README"
  echo "Bumped README image cache -> $REL?v=$TS"
fi
