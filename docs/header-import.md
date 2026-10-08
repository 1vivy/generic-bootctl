# Minimal source imports and reproducible bindings

**Status (2026-10-07)** — No AOSP platform tree and no libhidlbase/libvintf NDK
link dependency. Apache/BSD licenses remain beside the imported sources.

The minimal-import pattern was checked against LineageOS
[QTI bootctrl](https://github.com/LineageOS/android_hardware_qcom_bootctrl/tree/846dfb0652cc142e1783cbe085783527bbe4a190):
`aidl/BootControl.h` includes only BnBootControl + the shared backend header;
`1.2/impl/BootControl.h` includes the needed boot interface + HIDL support;
`Android.bp` uses libhardware_headers rather than copying/building all libhardware.
We use that separation but transcribe the stable C/wire ABI into Rust, avoiding
C++ platform-library linkage entirely for the shipped AIDL path.

`third_party/aosp/PROVENANCE` records exact paths, revisions and SHA-256 values:
* hardware/interfaces@5688f7eb1e117ed26e642a695de300b7683acb87: frozen AIDL V1
  `.aidl` + `.hash`, HIDL 1.0–1.2 `.hal`, and misc reference implementation/struct.
* hardware/libhardware@cd2b68d71a1dd9a45668ddd9a507000ccd7ff114:
  hardware.h + boot_control.h, with include_all symlink targets materialized.
* bootable/recovery@80fbea7e9af1dd883f2046e9b299d6fe45a0f693:
  bootloader_message.h for offsets and packed VAB layout.
* QTI source references at 846dfb0652cc142e1783cbe085783527bbe4a190 have a separate
  `third_party/qcom/PROVENANCE` and BSD NOTICE.

## Mechanism

Bindings are reviewed **hand transcriptions**, not bindgen outputs. Reproduction
is deterministic from the committed headers: `abi.rs` lists each C member in
header order, fixed-width integers, LP64 pointers and nullable extern-C function
pointers. Const assertions check hw_module_t=248 and boot_control_module_t=568.
`HAL_MODULE_INFO_SYM` is the upstream macro for `HMI`, not a symbol named literally
HAL_MODULE_INFO_SYM. `scripts/check-abi.sh` verifies both ABI offsets and the actual
export with the NDK compiler/readelf. It extracts only the two required structs
and numeric ABI constants from the pinned headers, eliminating unused native_handle
and graphics includes instead of importing their large unrelated closure.

`aidl/android.rs` transcribes the NDK parcel/binder C signatures; method order,
metadata transaction IDs, version 1 and the hash are fixed by the committed AIDL
snapshot. The transport derives from kernelesp@1d8413b84b32316979a0f3ae3a8c11d712da7646
payloads/boot-hal/src/{android,service}.rs. Platform-only NDK entrypoints are
resolved with checked dlsym; no vendor libraries are bundled.

On-disk bitfields use explicit byte masks/LE integers, not Rust/C bitfield layout
assumptions. Golden image tests verify layout, reserved-byte preservation and
independently recomputed CRCs. UFS sg_io_v4 is transcribed from NDK r29's
linux/bsg.h; request/reply bytes follow the pinned QTI recovery-ufs-bsg source and
NDK scsi/scsi_bsg_ufs.h. Its size is compile-time asserted at 160 bytes.

`hidl/parcel.rs` and `hidl/driver.rs` transcribe the LP64 kernel Binder ABI and
libhwbinder encoding, not generated C++ bindings. Minimal transport references
from `system/libhwbinder@45db897b21385e9db4f6d1a0af6573ca649bc88b` and
`system/libhidl@69123db938e2d4b7d129e8d7bedbfdb98ec6e5b8` remain reference-only.
Imported `current.txt` files pin each boot and IBase hash. See
[wire derivation](hidl-wire.md); no private platform HIDL library is linked.
