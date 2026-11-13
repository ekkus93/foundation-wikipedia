#!/usr/bin/env bash
# Cross-build the wiki-ffi UniFFI library for Android (MOB-001).
# RUST_ANDROID_ABIS selects ABIs (default "arm64-v8a"; the CI emulator also
# needs "x86_64"). Each library is copied to app/src/main/jniLibs/<abi>/.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
API=26
ABIS="${RUST_ANDROID_ABIS:-arm64-v8a}"

if [[ "${1:-}" == "--help" ]]; then
  printf 'Usage: ANDROID_NDK_HOME=/path/to/ndk bash android/scripts/build-rust-android.sh\n'
  printf 'Builds wiki-ffi for %s (Android API %s) into app/src/main/jniLibs.\n' "$ABIS" "$API"
  exit 0
fi
if [[ $# -ne 0 ]]; then
  echo "Unknown argument: $1" >&2
  exit 2
fi

NDK="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}"
if [[ -z "$NDK" || ! -d "$NDK" ]]; then
  echo "Set ANDROID_NDK_HOME to an installed Android NDK directory." >&2
  exit 2
fi

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) HOST=linux-x86_64 ;;
  Darwin-arm64) HOST=darwin-x86_64 ;;
  Darwin-x86_64) HOST=darwin-x86_64 ;;
  *) echo "Unsupported NDK host: $(uname -s)-$(uname -m)" >&2; exit 2 ;;
esac

TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/$HOST/bin"
AR="$TOOLCHAIN/llvm-ar"
for ABI in $ABIS; do
  case "$ABI" in
    arm64-v8a) TARGET=aarch64-linux-android ;;
    x86_64) TARGET=x86_64-linux-android ;;
    *) echo "Unsupported Android ABI: $ABI" >&2; exit 2 ;;
  esac
  CLANG="$TOOLCHAIN/${TARGET}${API}-clang"
  CXX="$TOOLCHAIN/${TARGET}${API}-clang++"
  for tool in "$CLANG" "$CXX" "$AR"; do
    if [[ ! -x "$tool" ]]; then
      echo "Missing Android NDK tool: $tool" >&2
      exit 2
    fi
  done
  if ! rustup target list --installed | grep -qx "$TARGET"; then
    echo "Install the pinned Rust Android target: rustup target add $TARGET" >&2
    exit 2
  fi
  NORMALIZED="${TARGET//-/_}"
  UPPER="$(printf '%s' "$NORMALIZED" | tr '[:lower:]' '[:upper:]')"
  export "CARGO_TARGET_${UPPER}_LINKER=$CLANG"
  # Native dependency build scripts (for example zstd-sys via cc-rs) do not
  # consume Cargo's linker variable; provide target-normalized compiler tools.
  export "CC_${NORMALIZED}=$CLANG"
  export "CXX_${NORMALIZED}=$CXX"
  export "AR_${NORMALIZED}=$AR"
  cargo build --manifest-path "$ROOT/Cargo.toml" --package wiki-ffi --target "$TARGET" --release --locked
  LIB="$ROOT/target/$TARGET/release/libwiki_ffi.so"
  test -f "$LIB"
  if [[ "$ABI" == arm64-v8a ]]; then
    python3 "$ROOT/scripts/verify_android_elf.py" "$LIB"
  fi
  OUT="$ROOT/android/app/src/main/jniLibs/$ABI"
  mkdir -p "$OUT"
  cp "$LIB" "$OUT/libwiki_ffi.so"
  printf 'Built %s for %s\n' "$OUT/libwiki_ffi.so" "$ABI"
done
