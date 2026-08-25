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

# Any Chromium-based browser will do — they share the headless flags used
# below. Picked in preference order, overridable with SHOT_BROWSER=/path.
find_browser() {
  if [[ -n "${SHOT_BROWSER:-}" ]]; then
    printf '%s' "$SHOT_BROWSER"
    return
  fi
  local candidates=(
    "/Applications/Thorium.app/Contents/MacOS/Thorium"
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
    "/Applications/Chromium.app/Contents/MacOS/Chromium"
    "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"
  )
  local candidate
  for candidate in "${candidates[@]}"; do
    [[ -x "$candidate" ]] && printf '%s' "$candidate" && return
  done
}

CHROME="$(find_browser)"
if [[ -z "$CHROME" ]]; then
  echo "No Chromium-based browser found. Install one, or set SHOT_BROWSER=/path/to/binary" >&2
  exit 1
fi
REPO="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="$REPO/scripts/.shot-profile"

URL="https://home.kahn.studio/"
WIDTH=1280
HEIGHT=2200
SCALE=2
QUALITY=90
OUT="$REPO/screenshots/maison.jpg"
BUMP_README=1
BUDGET=8000
WATCHDOG=90

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
    --budget)   BUDGET="$2";   shift 2 ;;
    --watchdog) WATCHDOG="$2"; shift 2 ;;
    *) echo "Unknown flag: $1" >&2; exit 1 ;;
  esac
done

if [[ ! -d "$PROFILE" ]]; then
  echo "No saved login profile yet. Run:  scripts/screenshot.sh login" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUT")"

# Chromium spawns helper processes, and killing only the parent leaves them
# holding the profile — the next run then blocks until the watchdog fires and
# reports a bare "no image produced". Clear the whole group and any lock files
# left by a capture that was interrupted. Cookies, hence the login, are kept.
reap_profile() {
  pkill -9 -f "$PROFILE" 2>/dev/null || true
  sleep 1
  rm -f "$PROFILE"/Singleton* 2>/dev/null || true
}
reap_profile

# The browser is silenced by default; SHOT_DEBUG=1 lets it explain itself.
SHOT_LOG=/dev/null
[[ -n "${SHOT_DEBUG:-}" ]] && SHOT_LOG=/dev/stderr

# --disable-gpu is not optional here: with GPU rasterisation, Thorium
# silently produces no file at all for a scaled capture (--force-device-
# scale-factor=2), whatever the window size. Software rasterisation renders
# a static page identically, so nothing is lost.
#
# The browser writes a PNG; we convert to JPG afterwards.
PNG="${TMPDIR:-/tmp}/maison-shot-$$.png"
trap 'rm -f "$PNG"' EXIT
rm -f "$PNG"

"$CHROME" \
  --headless=new \
  --user-data-dir="$PROFILE" \
  --no-first-run --no-default-browser-check \
  --disable-background-networking --disable-component-update \
  --disable-sync --disable-features=Translate \
  --hide-scrollbars \
  --disable-gpu \
  --force-device-scale-factor="$SCALE" \
  --window-size="${WIDTH},${HEIGHT}" \
  --virtual-time-budget="$BUDGET" \
  --screenshot="$PNG" \
  "$URL" >"$SHOT_LOG" 2>&1 &
CHROME_PID=$!

# Watchdog: never hang — kill the browser if it overstays. Software
# rasterisation of a tall @2x page is slow (a 2560x7200 frame takes tens of
# seconds), so this has to be generous or it truncates the capture into a
# "no image produced" failure.
# The browser writes the PNG and then lingers instead of exiting, so waiting
# on the process costs the whole watchdog on every run. Poll for the file and
# stop as soon as it has stopped growing.
deadline=$(( SECONDS + WATCHDOG ))
last_size=0
while (( SECONDS < deadline )); do
  if [[ -s "$PNG" ]]; then
    size=$(stat -f%z "$PNG" 2>/dev/null || echo 0)
    [[ "$size" == "$last_size" ]] && break
    last_size="$size"
  fi
  sleep 1
done
reap_profile

if [[ ! -s "$PNG" ]]; then
  echo "Capture failed (no image produced) using: $CHROME" >&2
  echo "Re-run with SHOT_DEBUG=1 to see the browser's own output." >&2
  exit 1
fi

# PNG -> JPG (built-in `sips`, no deps).
sips -s format jpeg -s formatOptions "$QUALITY" "$PNG" --out "$OUT" >/dev/null
echo "Saved -> $OUT  (${WIDTH}x${HEIGHT} @${SCALE}x, q${QUALITY}, $(basename "$CHROME"))"

# Cache-bust: refresh `?v=<timestamp>` on this image's URL in README.md so
# GitHub stops serving the stale cached version.
README="$REPO/README.md"
REL="${OUT#"$REPO"}"   # e.g. /screenshots/maison.jpg
if [[ "$BUMP_README" == 1 && -f "$README" ]] && grep -q "$REL" "$README"; then
  TS="$(date +%s)"
  perl -i -pe "s{\Q$REL\E(\?v=\d+)?}{$REL?v=$TS}g" "$README"
  echo "Bumped README image cache -> $REL?v=$TS"
fi
