#!/usr/bin/env bash
# Opt-in UniFFI Kotlin binding generation stub for Android.
#
# A real uniffi-bindgen executable is deliberately supplied by the caller so
# the root Rust workspace lockfile is not silently changed by this bootstrap
# task. MOB-001 will pin the production UniFFI toolchain with the FFI crate.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
UDL="$ROOT/crates/wiki-ffi/src/foundation_wikipedia.udl"
OUT="${UNIFFI_OUT_DIR:-$ROOT/android/app/build/generated/source/uniffi}"
BINDGEN="${UNIFFI_BINDGEN:-uniffi-bindgen}"

if [[ "${1:-}" == "--help" ]]; then
  printf 'Usage: UNIFFI_BINDGEN=/path/to/uniffi-bindgen gradle :app:generateUniFfiKotlin\n'
  printf 'Generates provisional Kotlin sources from %s into %s.\n' "$UDL" "$OUT"
  exit 0
fi
if [[ $# -ne 0 ]]; then
  echo "Unexpected argument: $1" >&2
  exit 2
fi
if [[ ! -f "$UDL" ]]; then
  echo "Missing UniFFI UDL: $UDL" >&2
  exit 2
fi

if [[ "$BINDGEN" == */* ]]; then
  if [[ ! -x "$BINDGEN" ]]; then
    echo "UNIFFI_BINDGEN is not executable: $BINDGEN" >&2
    exit 2
  fi
elif ! command -v "$BINDGEN" >/dev/null 2>&1; then
  echo "UniFFI generator not found. Set UNIFFI_BINDGEN to a pinned uniffi-bindgen executable." >&2
  exit 2
fi

mkdir -p "$OUT"
"$BINDGEN" generate "$UDL" --language kotlin --out-dir "$OUT"
printf 'Generated provisional UniFFI Kotlin bindings in %s\n' "$OUT"
printf 'NOTICE: generated source is not yet compiled into the app or a callable MOB-001 bridge.\n'
