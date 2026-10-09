#!/usr/bin/env bash
# Build the platform-neutral Rust FFI library for Android arm64.
# This is the native ABI build stage only; UniFFI bindings are not generated yet.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TARGET="aarch64-linux-android"
API=26
OUT="$ROOT/android/app/src/main/jniLibs/arm64-v8a"

if [[ "${1:-}" == "--help" ]]; then
  printf 'Usage: ANDROID_NDK_HOME=/path/to/ndk bash android/scripts/build-rust-android.sh\n'
  printf 'Builds wiki-ffi for arm64-v8a (Android API %s). UniFFI generation is not yet wired.\n' "$API"
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

CLANG="$NDK/toolchains/llvm/prebuilt/$HOST/bin/aarch64-linux-android${API}-clang"
if [[ ! -x "$CLANG" ]]; then
  echo "Missing Android NDK compiler: $CLANG" >&2
  exit 2
fi
if ! rustup target list --installed | grep -qx "$TARGET"; then
  echo "Install the pinned Rust Android target: rustup target add $TARGET" >&2
  exit 2
fi

export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$CLANG"
cargo build --manifest-path "$ROOT/Cargo.toml" --package wiki-ffi --target "$TARGET" --release --locked
LIB="$ROOT/target/$TARGET/release/libwiki_ffi.so"
test -f "$LIB"
python3 "$ROOT/scripts/verify_android_elf.py" "$LIB"
LLVM_NM="$NDK/toolchains/llvm/prebuilt/$HOST/bin/llvm-nm"
if [[ ! -x "$LLVM_NM" ]]; then
  echo "Missing Android NDK llvm-nm: $LLVM_NM" >&2
  exit 2
fi
if ! "$LLVM_NM" -D --defined-only "$LIB" | grep -Eq '[[:space:]][TW][[:space:]]foundation_wikipedia_ffi_abi_version
cp "$LIB" "$OUT/libwiki_ffi.so"
printf 'Built %s\n' "$OUT/libwiki_ffi.so"
printf 'NOTICE: native ABI library only; UniFFI Kotlin bindings and JNI API are not yet implemented.\n'
; then
  echo "wiki-ffi AArch64 library is missing its required ABI version handshake" >&2
  exit 2
fi
mkdir -p "$OUT"
cp "$LIB" "$OUT/libwiki_ffi.so"
printf 'Built %s\n' "$OUT/libwiki_ffi.so"
printf 'NOTICE: native ABI library only; UniFFI Kotlin bindings and JNI API are not yet implemented.\n'
