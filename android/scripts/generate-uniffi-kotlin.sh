#!/usr/bin/env bash
# Generate the committed Kotlin bindings for wiki-ffi (MOB-001).
#
# UniFFI "library mode": the exported API is read from the host-built
# wiki-ffi library, and the workspace-pinned tools/uniffi-bindgen writes
# Kotlin with formatting disabled so output is deterministic. CI regenerates
# and fails if the committed bindings drift from the Rust exports.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="${UNIFFI_OUT_DIR:-$ROOT/android/app/src/main/java}"

if [[ "${1:-}" == "--help" ]]; then
  printf 'Usage: bash android/scripts/generate-uniffi-kotlin.sh\n'
  printf 'Regenerates uniffi/wiki_ffi/wiki_ffi.kt under %s from the wiki-ffi exports.\n' "$OUT"
  exit 0
fi
if [[ $# -ne 0 ]]; then
  echo "Unexpected argument: $1" >&2
  exit 2
fi

if [[ -n "${UNIFFI_LIBRARY:-}" ]]; then
  LIB="$UNIFFI_LIBRARY"
else
  cargo build --manifest-path "$ROOT/Cargo.toml" --package wiki-ffi --locked
  case "$(uname -s)" in
    Darwin) LIB="$ROOT/target/debug/libwiki_ffi.dylib" ;;
    *) LIB="$ROOT/target/debug/libwiki_ffi.so" ;;
  esac
fi
if [[ ! -f "$LIB" ]]; then
  echo "Missing host wiki-ffi library: $LIB" >&2
  exit 2
fi

if [[ -n "${UNIFFI_BINDGEN:-}" ]]; then
  if [[ ! -x "$UNIFFI_BINDGEN" ]]; then
    echo "UNIFFI_BINDGEN is not executable: $UNIFFI_BINDGEN" >&2
    exit 2
  fi
  BINDGEN=("$UNIFFI_BINDGEN")
else
  BINDGEN=(cargo run --quiet --manifest-path "$ROOT/Cargo.toml" --package uniffi-bindgen --locked --)
fi

mkdir -p "$OUT"
"${BINDGEN[@]}" generate --library "$LIB" --language kotlin --no-format --out-dir "$OUT"
printf 'Generated UniFFI Kotlin bindings for wiki-ffi in %s\n' "$OUT"
