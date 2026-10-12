# generic-bootctl

**Status (2026-10-07)** — Rust boot-control HAL. AIDL frozen V1,
raw hwbinder HIDL 1.0–1.2, LP64 legacy module ABI, AOSP misc, QTI UFS GPT, and
sandboxed vendor parity are implemented. No device boot/OTA validation is claimed.

A boot HAL is an interface, not a storage format. The bootloader must interpret
exactly the metadata this HAL changes. This is experimental boot-critical storage
software, not a universal device compatibility promise.

## Design

| Decision | Implemented choice | Reason |
| --- | --- | --- |
| Replacement | Original executable and discovered LP64 bootctrl libraries, overlaid at the same paths by the loading module | Keep existing rc, VINTF, instance and domain |
| Binaries | One `bootctl-unified` executable per CPU ABI plus `bootctrl.module.so` | Different executable/module ABIs, one shared core |
| Transport | Manifest-declared AIDL V1 and/or HIDL 1.0–1.2, default instance | No AIDL V2 exists; no undeclared registration |
| Lifecycle | Non-lazy Binder and hwservicemanager registration | Avoid lazy/non-oneshot restart loops |
| Own rc/manifest/policy | None | No extra registration or permissive domain |
| Writes | Exact `persist.generic_bootctl.rw=1`, checked per mutation | Default read-only; no read-time initialization/repair |
| Backend | Runtime metadata/controller discovery; reject ambiguity | No model-name allow-lists |

## Supported matrix

| Surface | Support and limits |
| --- | --- |
| Android CPU | aarch64, x86_64; API 35 build baseline; NDK r29 |
| AIDL | Frozen V1, default instance, normal registration |
| HIDL 1.0–1.2 | Raw `/dev/hwbinder`; highest declared minor, default instance, inherited IBase contract |
| Legacy | LP64 `boot_control_module_t`, `HMI` export (`HAL_MODULE_INFO_SYM` macro) |
| AOSP misc | Valid v1 32-byte control block, 1–4 slots, CRC checked; existing VAB v2 message |
| Qualcomm | UFS QTI attribute layout (bits 48–55), both GPT copies on all discovered controller LUs, type-GUID swap, BSG boot-LUN switch/readback |
| Vendor passthrough | Backed-up LP64 library, always read-only, isolated Landlock ABI ≥3 + seccomp probe; requires complete readable legacy table |
| Unsupported | 32-bit modules, eMMC boot-select, legacy QTI ioctl kernels, ambiguous controllers/backends, invalid/disagreeing GPTs, custom instances, rc command arguments, manifest overrides |

`getActiveBootSlot` is absent from some old vendor legacy tables; those cannot
provide full parity and fail closed. Vendor libraries requiring prohibited writes
or ioctls during initialization also fail. The vendor probe cannot be silently
run without confinement on kernels lacking Landlock.

## Build and host gates

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test --locked
cargo fmt --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked -p bootctl-unified
# ANDROID_NDK_ROOT must name an Android NDK r29 tree.
bash scripts/build.sh
bash scripts/check-abi.sh
```

`scripts/build.sh` writes one directory per Android architecture under `out/impl/`.
`rust-toolchain.toml` pins nightly-2026-08-08. There are no crates.io dependencies,
no platform AOSP build requirement, and no `.cargo/config.toml`. CI runs host
contracts and cross-builds with the downloadable r29 NDK.

HIDL uses the kernel Binder LP64 ABI directly, with AOSP scatter-gather parcel
buffer fixups; no `libhidlbase` or `libhwbinder` is linked or bundled. In a mixed
manifest both transports share one serialized `Service`, including its live
write gate. See [wire contract and golden-byte derivation](docs/hidl-wire.md).

## Staged rollout

`bootctl-unified --install-plan ROOT` is the same-path plan a loading module must
apply: it parses VINTF XML and init service stanzas, identifies the actual
executable by interface declaration/content rather than a guessed filename, and
refuses absent/unsupported declarations, critical services, unmarked-successful
current slots, pre-enabled writes, unsupported architecture, and ambiguous service
ownership. Applying the plan deliberately fails closed rather than producing a
partial replacement; this repository ships no installer, rc, manifest or policy.

1. Set up recovery/safe mode first. Install **read-only** only after the
   current slot has been marked successful by the stock HAL.
2. Compare stock and replacement slot count, current/active slot, suffix,
   bootability, success and merge state. Capture SELinux denials; successful label
   copying does **not** prove that the stock domain permits every runtime access.
3. Only after parity and repeated successful boot cycles, explicitly enable
   `persist.generic_bootctl.rw=1` and validate current-slot mark-successful.
4. Slot activation testing comes last, after both slots and recovery are proven.
   This repository does not run any device commands or enable the property for you.
5. A separate rc/manifest/policy deployment is not shipped.

## Risks and escape hatches

The primary brick vector is **writes**, especially a wrong GPT slot mapping or UFS
boot-LUN switch. Readback detects I/O mismatch, not bootability. GPT copies/LUs and
UFS attributes are not one atomic transaction: a power loss or later-LU failure
can leave a partial update. There is no automatic rollback, repair, or retry that
could hide that state. See [backend guarantees](docs/backends.md).

Read-only substitution is unsafe immediately after an OTA if the current slot is
not successful: refusing success writes can exhaust retry counts. The install plan
reports that state before any overlay or backup is created. Disabling writes after
install also prevents future OTAs from completing their boot-control mutations.

A module manager's volume-down safe mode disables modules; removing the overlay
and rebooting restores the stock executable. It never rewrites vendor partitions
and **does not undo boot metadata writes**. Do not assume a HAL crash is harmless:
the install plan refuses `critical` rc services rather than relying on that claim.

## Sources and license

Apache-2.0 for this project; imported QTI references retain their BSD NOTICE.
AIDL parcel transport was adapted from
`kernelesp/payloads/boot-hal@1d8413b84b32316979a0f3ae3a8c11d712da7646`
(Apache-2.0), removing its gobbl-specific efivarfs/GBS1 backend. The slot and merge
semantics were checked against pinned AOSP and LineageOS sources, not copied from
an unrelated GPT convention. [Header imports](docs/header-import.md) lists pins,
minimal-dependency strategy and binding reproduction. [Architecture](docs/architecture.md)
describes the public core contract for additional transports.
