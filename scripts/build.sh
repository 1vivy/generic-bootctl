#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
NDK="${ANDROID_NDK_ROOT:?set ANDROID_NDK_ROOT to an NDK r29 tree}"
TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
grep -q '^Pkg.Revision = 29\.' "$NDK/source.properties" || { echo 'NDK r29 required' >&2; exit 1; }
export CARGO_TARGET_DIR="$ROOT/target"
export RUSTFLAGS="${RUSTFLAGS:-} -Dwarnings -C link-arg=-Wl,-z,relro,-z,now"
STAGE="$ROOT/out/impl"
mkdir -p "$STAGE"
for ARCH in aarch64 x86_64; do
  TARGET="$ARCH-linux-android"
  LINKER="$TOOLCHAIN/${TARGET}35-clang"
  [ -x "$LINKER" ] || { echo "missing $LINKER" >&2; exit 1; }
  KEY="CARGO_TARGET_${TARGET^^}_LINKER"
  export "${KEY//-/_}=$LINKER"
  cargo build --manifest-path "$ROOT/Cargo.toml" --locked --offline --release --target "$TARGET" -p bootctl-unified -p generic-bootctl-legacy
  mkdir -p "$STAGE/$ARCH"
  cp "$ROOT/target/$TARGET/release/bootctl-unified" "$STAGE/$ARCH/bootctl-unified"
  cp "$ROOT/target/$TARGET/release/libgeneric_bootctl_legacy.so" "$STAGE/$ARCH/bootctrl.module.so"
  "$TOOLCHAIN/llvm-strip" "$STAGE/$ARCH/bootctl-unified" "$STAGE/$ARCH/bootctrl.module.so"
done
chmod 0755 "$STAGE/"*/bootctl-unified
printf '%s\n' "$STAGE"
