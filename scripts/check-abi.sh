#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
NDK=${ANDROID_NDK_HOME:-${GENERIC_BOOTCTL_NDK:-/home/vivy/Projects/efisp-projects/gobbl/out/toolchains/android-ndk-r29}}
BIN="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
python3 - "$ROOT" "$TMP" <<'PY'
from pathlib import Path
import sys
root, tmp = map(Path, sys.argv[1:])
headers = root / 'third_party/aosp/hardware/libhardware/include/hardware'
(tmp / 'hardware').mkdir()
for name in ('hardware.h', 'boot_control.h'):
    data = (headers / name).read_text()
    # These two unrelated imports are unused by every declaration in hardware.h.
    data = data.replace('#include <cutils/native_handle.h>', '').replace('#include <system/graphics.h>', '')
    (tmp / 'hardware' / name).write_text(data)
(tmp / 'abi.c').write_text('''#include <stddef.h>
#include <hardware/boot_control.h>
_Static_assert(sizeof(hw_module_t)==248,"hw_module_t");
_Static_assert(sizeof(boot_control_module_t)==568,"boot_control_module_t");
_Static_assert(offsetof(boot_control_module_t,init)==248,"init");
_Static_assert(offsetof(boot_control_module_t,getActiveBootSlot)==320,"active");
_Static_assert(offsetof(boot_control_module_t,reserved)==328,"reserved");
''')
PY
for ARCH in aarch64 x86_64; do
  "$BIN/$ARCH-linux-android35-clang" -Wall -Werror -I "$TMP" -c "$TMP/abi.c" -o "$TMP/$ARCH.o"
  "$BIN/llvm-readelf" --dyn-syms "$ROOT/target/$ARCH-linux-android/release/libgeneric_bootctl_legacy.so" | grep -E 'GLOBAL +DEFAULT +[0-9]+ +HMI$'
done
