#!/usr/bin/env bash
set -euo pipefail

OUTPUT_DIR="${1:-dist/linux-musl}"
TARGET="x86_64-unknown-linux-musl"
BINARY_NAME="netherica"
MUSL_LINKER="${MUSL_LINKER:-x86_64-linux-musl-gcc}"

die() {
  echo "ERROR: $*" >&2
  exit 1
}

require_cmd() {
  local cmd="$1"
  local hint="$2"
  if ! command -v "${cmd}" >/dev/null 2>&1; then
    die "Missing required tool '${cmd}'. ${hint}"
  fi
}

echo "Building Netherica (${TARGET})..."

require_cmd cargo "Install Rust toolchain: https://rustup.rs"
require_cmd rustup "Install rustup: https://rustup.rs"
require_cmd "${MUSL_LINKER}" "Install musl C toolchain (provides x86_64-linux-musl-gcc). On Debian/Ubuntu: apt-get install musl-tools; Fedora: dnf install musl-gcc; Alpine: apk add musl-dev musl-tools."
require_cmd sha256sum "Install coreutils package (sha256sum)."

if ! rustup target list --installed | grep -qx "${TARGET}"; then
  echo "Rust target '${TARGET}' is not installed. Installing..."
  rustup target add "${TARGET}"
fi

mkdir -p "${OUTPUT_DIR}"

cargo build --locked --release --target "${TARGET}"

cp "target/${TARGET}/release/${BINARY_NAME}" "${OUTPUT_DIR}/${BINARY_NAME}"
sha256sum "${OUTPUT_DIR}/${BINARY_NAME}" > "${OUTPUT_DIR}/SHA256SUMS.txt"

echo "Done -> ${OUTPUT_DIR}/${BINARY_NAME}"
