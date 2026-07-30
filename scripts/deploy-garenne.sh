#!/usr/bin/env bash
# Deploy garenne into the clapier tribe overlay.
#
# The overlay (garenne/overlay, served by clapier --overlay) resolves
# rabbits/<mac>/vl/bc.jsp before common/vl/bc.jsp before the base tree.
# The rabbit refetches bc.jsp at every boot, so a deploy takes effect at
# the next power cycle (or remote reboot, once garenne knows how).
#
# Usage:
#   deploy-garenne.sh [--rabbit MAC]            build + install bc.jsp
#   deploy-garenne.sh rollback [--rabbit MAC]   remove the deployed bc.jsp
#   deploy-garenne.sh status                    show what the overlay holds
#
# Without --rabbit, the target is the whole tribe (common/). With it, a
# single canary (MAC accepted as 00:19:db:9c:28:15 or 0019db9c2815).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OVERLAY="$ROOT/garenne/overlay"
ACTION="deploy"
TARGET="common"

normalize_mac() {
  local mac
  mac="$(echo "$1" | tr -d ':' | tr '[:upper:]' '[:lower:]')"
  if [[ ! "$mac" =~ ^[0-9a-f]{12}$ ]]; then
    echo "invalid MAC: $1" >&2
    exit 1
  fi
  echo "$mac"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    rollback|status) ACTION="$1"; shift ;;
    --rabbit) TARGET="rabbits/$(normalize_mac "$2")"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 1 ;;
  esac
done

case "$ACTION" in
  status)
    echo "overlay: $OVERLAY"
    found=0
    while IFS= read -r f; do
      found=1
      printf '  %s  %s bytes  %s\n' "${f#"$OVERLAY"/}" \
        "$(stat -f %z "$f")" "$(stat -f '%Sm' "$f")"
    done < <(find "$OVERLAY" -type f -name bc.jsp | sort)
    if [[ $found -eq 0 ]]; then
      echo "  no bc.jsp deployed - every rabbit eats the base tree"
    fi
    ;;
  rollback)
    if [[ -f "$OVERLAY/$TARGET/vl/bc.jsp" ]]; then
      rm "$OVERLAY/$TARGET/vl/bc.jsp"
      echo "removed $TARGET/vl/bc.jsp - back to the base tree at next boot"
    else
      echo "nothing deployed at $TARGET/vl/bc.jsp"
    fi
    ;;
  deploy)
    bash "$ROOT/garenne/build.sh"
    mkdir -p "$OVERLAY/$TARGET/vl"
    # Atomic within the same filesystem: the rabbit never sees a half file.
    cp "$ROOT/garenne/build/garenne.bin" "$OVERLAY/$TARGET/vl/.bc.jsp.tmp"
    mv "$OVERLAY/$TARGET/vl/.bc.jsp.tmp" "$OVERLAY/$TARGET/vl/bc.jsp"
    echo "deployed $(stat -f %z "$OVERLAY/$TARGET/vl/bc.jsp") bytes to $TARGET/vl/bc.jsp"
    echo "power-cycle the rabbit to load it"
    ;;
esac
