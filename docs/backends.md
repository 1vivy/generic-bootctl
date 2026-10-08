# Backend formats and durability

**Status (2026-10-07)** — Implemented format contracts; synthetic image proof is
not hardware, bootloader or OTA proof.

## AOSP misc

At offset 2048, the 32-byte bootloader_control is magic 0x42414342, version 1,
slot count in bits 0–2 of byte 9, four two-byte slot records at 12, IEEE CRC32 over
bytes 0–27 at 28. Slot byte 0 has priority[3:0], tries[6:4], success[7]; byte 1
has verity[0]. Every reserved bit/byte, suffix and bootloader merge field is
preserved. SetActive uses priority 15 and tries 6, demotes other priority-15 slots
to 14, preserves success, and clears verity only on a different slot. Success sets
tries=1; unbootable clears tries/success. Bootable means tries!=0. Active chooses
highest priority with current-slot tie precedence, matching AOSP libboot_control.

VAB is a separate 64-byte packed message at 32768: version 2 at 0, little-endian
magic 0x56740ab0 at 1, merge status at 5, source slot at 6. SNAPSHOTTED on its
source slot reads as NONE. Reserved bytes survive writes. Invalid/uninitialized
records fail rather than reproducing AOSP's read-time auto-initialization; this
is essential to the default read-only guarantee. Writes sync and read back the
exact touched region. Neither format is power-fail atomic on every storage stack.

## Qualcomm QTI UFS

Reference: LineageOS/android_hardware_qcom_bootctrl commit
846dfb0652cc142e1783cbe085783527bbe4a190 (CLO upstream), not the incompatible
52–59-bit convention found in some old third-party bootctl forks. Attribute bits:
priority 48–49, active 50, retry 51–53, success 54, unbootable 55. Reads of the boot
pair use active, success and inverse-unbootable exactly as the QTI HAL does.

Discovery uses the boot partition's sysfs controller ancestry and all whole disks
on that UFS controller, not a single assumed sda LU or a device/model list. GPT
primary and backup headers and entry CRCs must validate and their entries must
agree. 512-byte/4096-byte blocks and 128-byte entries are supported. Refuse unusual
geometry, incomplete/duplicate A/B pairs, no boot pair, or pairs without exactly
one active member. No repair or normalization is performed. XBL, xbl_config,
multiimgoem and multiimgqti are excluded because UFS boot selection handles them.

Activation transfers the active **type GUID** to the target (unique GUIDs never
change), sets its attribute byte 54 to 0x3f, and clears only active on the other
slot. Mark-successful ORs 0x40; unbootable ORs 0x80, preserving success/retries.
Every discovered pair across all LUs is changed, in both GPT arrays. Array/header
CRCs are recomputed; backup then primary are written, synced and read back. This
ordering deliberately improves on QTI's header-before-array write order but is
still not a multi-LU transaction. Partial failures are reported, never repaired.

Before activation, the existing UFS boot-LUN attribute must read as 1 or 2. Only
after GPT writes does SG_IO on the unique discovered UFS BSG node issue
WRITE_ATTR BOOT_LU_EN with 1 for A or 2 for B, followed by READ_ATTR verification.
The process must pass the same property gate before either effect. No legacy
0x5388 ioctl fallback, no eMMC selection, no pretend-success no-op. An ioctl error
or readback mismatch is a hard failure; it does not imply preceding GPT writes
were rolled back. A LUN read is not a bootability check.

## Vendor parity

When neither native format is recognized, exactly one backed-up bootctrl library
may be selected. Each read launches the installed unified executable as a fresh
probe, enables no_new_privs, installs an empty Landlock write allow-list (ABI≥3,
including truncate), denies all ioctls/alternate syscall ABIs with seccomp, then
loads the library and invokes its legacy init/getters. No mutation callback is
called, even if the global write property is enabled. Unsupported confinement,
loader/SELinux failure, missing functions or invalid replies fail closed.
Landlock/SELinux support on the actual vendor domain remains a deployment gate.
