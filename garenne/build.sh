#!/usr/bin/env bash
# Build garenne (the rabbit's embedded application) with the MTL toolchain.
#
# Usage:
#   ./build.sh        device build -> build/garenne.bin (servable as bc.jsp)
#   ./build.sh sim    simulator build + run (ANSI LED view; Ctrl-C to quit)
#   ./build.sh test   golden-frame test suite in the simulator
#
# Requires the mtl-dev Docker image (debian bookworm amd64 + multilib) and
# the MTL toolchain (compiler, simulator, preproc scripts) built under
# vendor/ServerlessNabaztag. Toolchain only: garenne includes none of it.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SN="vendor/ServerlessNabaztag"
MODE="${1:-bin}"

mkdir -p "$ROOT/garenne/build"

in_toolchain() {
  docker run --rm --platform linux/amd64 -v "$ROOT:/work" \
    -w "/work/$SN/firmware" mtl-dev bash -c "$1"
}

preprocess() {  # entry file, output file, extra defs
  in_toolchain "perl ../scripts/preproc.pl ${3:-} $1 \
    | python3 ../scripts/preproc_remove_extra_protos.py > $2"
}

case "$MODE" in
  bin)
    preprocess /work/garenne/main.mtl /work/garenne/build/garenne.mtl
    in_toolchain "../compiler/mtl_comp/mtl_comp -s /work/garenne/build/garenne.mtl \
        /work/garenne/build/garenne.bin \
      && ls -la /work/garenne/build/garenne.bin"
    ;;
  sim)
    preprocess /work/garenne/main.mtl /work/garenne/build/garenne.mtl "-D SIMU"
    docker run --rm --platform linux/amd64 -v "$ROOT:/work" \
      -w "/work/$SN/firmware" mtl-dev \
      ../compiler/mtl_simu/mtl_simu --mac 0123456789ab --logs init,vm \
      --source /work/garenne/build/garenne.mtl
    ;;
  test)
    preprocess /work/garenne/tests/main.mtl /work/garenne/build/garenne-tests.mtl "-D SIMU"
    # Secho prints on the vm log channel; the LED ANSI rendering is noise
    # we strip before judging the run.
    out="$(in_toolchain "timeout 8 ../compiler/mtl_simu/mtl_simu --mac 0123456789ab \
      --logs init,vm --source /work/garenne/build/garenne-tests.mtl; true" \
      | perl -pe 's/\e\[[0-9;]*[A-Za-z]|\e\[[su]|\r//g')"
    echo "$out" | grep -E 'FAIL|got|want|TESTS' || true
    if echo "$out" | grep -q 'fail=0' && ! echo "$out" | grep -q 'FAIL'; then
      echo "test suite green"
    else
      echo "test suite RED" >&2
      exit 1
    fi
    ;;
  *)
    echo "unknown mode: $MODE" >&2
    exit 1
    ;;
esac
