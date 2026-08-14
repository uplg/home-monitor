#!/usr/bin/env bash
set -euo pipefail

TARGET="${TARGET:-arm-unknown-linux-musleabihf}"
TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
MANIFEST="backend/Cargo.toml"
PACKAGE_NAME="maison-backend"
DEFAULT_RUSTFLAGS="-C target-cpu=arm1176jzf-s"

if ! command -v rustup >/dev/null 2>&1; then
  printf '%s\n' 'Missing rustup. Install rustup and the target toolchain first.' >&2
  exit 1
fi

if ! command -v cargo-zigbuild >/dev/null 2>&1; then
  printf '%s\n' 'Missing cargo-zigbuild. Install it with: cargo install cargo-zigbuild' >&2
  exit 1
fi

if ! command -v zig >/dev/null 2>&1; then
  printf '%s\n' 'Missing zig. Install it first, for example with: brew install zig' >&2
  exit 1
fi

if ! rustup target list --installed | grep -qx "${TARGET}"; then
  printf 'Missing Rust target: %s\n' "${TARGET}" >&2
  printf 'Install it with: rustup target add %s\n' "${TARGET}" >&2
  exit 1
fi

export RUSTFLAGS="${DEFAULT_RUSTFLAGS}${RUSTFLAGS:+ ${RUSTFLAGS}}"
export RUSTC="$(rustup which --toolchain "${TOOLCHAIN}" rustc)"

if [ "$(uname -s)" = "Darwin" ]; then
  ulimit -n "${BUILD_ULIMIT_NOFILE:-4096}" 2>/dev/null || true
fi

printf 'Cross-compiling %s for %s\n' "${PACKAGE_NAME}" "${TARGET}"
printf 'Using rustup toolchain=%s\n' "${TOOLCHAIN}"
printf 'Using RUSTC=%s\n' "${RUSTC}"
printf 'Using RUSTFLAGS=%s\n' "${RUSTFLAGS}"

rustup run "${TOOLCHAIN}" cargo zigbuild \
  --release \
  --manifest-path "${MANIFEST}" \
  --target "${TARGET}" \
  --no-default-features \
  --bin "${PACKAGE_NAME}" \
  "$@"

printf '%s\n' "Artifact: target/${TARGET}/release/${PACKAGE_NAME}"
