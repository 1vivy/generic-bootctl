# Architecture

**Status (2026-10-07)** — Implemented, host/image-testable; not device-qualified.

`bootctl-unified` parses manifests before registration, discovers the backend,
constructs `core::Service`, then registers only AIDL V1/default using the NDK C ABI.
The frozen transaction order/hash come from the imported AIDL snapshot. Metadata
transactions use one Mutex; the property gate is evaluated on every mutation.
`crates/aidl` serializes replies and maps -1 INVALID_SLOT / -2 COMMAND_FAILED.
`crates/legacy` uses the identical service and translates these errors to errno.
Its Rust `HAL_MODULE_INFO_SYM` static is exported as **HMI**, exactly as hardware.h
requires. It does not export a second, nonstandard alias.

`core::Backend: Send` has `read_state`, `commit(&State, Operation)`, `read_merge`,
and `write_merge`. State contains normalized slot metadata and native-policy
parameters: maximum priority/tries, successful-bit clearing on activation, and
whether successful queries require bootability. QTI encodes bootability as the
inverse unbootable bit and active selection as normalized priority 2 versus 1;
its native commit uses Operation to preserve fields the QTI HAL leaves alone.
AOSP misc uses native 4-bit priorities and 3-bit retry counters.

The public `Service` methods are the contract for the HIDL follow-up:
`new(Box<dyn Backend>, current: u32, Box<dyn Fn() -> bool + Send>)`, `current_slot`,
`number_slots`, `suffix`, `active_slot`, `is_bootable`, `is_successful`,
`mark_successful`, `set_active`, `set_unbootable`, `merge_status`, and
`set_merge_status`. Fallible methods return `Result<T, i32>`. Suffix returns a
static CStr, including the empty string for invalid slots. Invalid slot queries
and mutators return -1; backend/gate failures return -2. HIDL owns its own wire
error mapping and must serialize shared Service access; do not bypass the gate.
`execute` is specifically the frozen AIDL V1 dispatch, not a HIDL transaction ABI.

The XML reader accepts ordinary manifest elements, attributes, comments and XML
processing instructions, but no DTD/entities, overrides, custom instances or
unsupported versions. It does not pretend to replace libvintf's full merge rules.
Those unsupported constructs fail closed. HIDL declarations are detected for
installation rejection; no HIDL service is registered in this release.

Installer discovery is exposed as `--install-plan ROOT` for host fake-root tests.
It reads the service stanza associated with the boot interface, not the first
service in an rc file. Binary content can associate stock rc files lacking an
`interface` line. Multiple matches or command arguments are refused. The installer
uses the stock bootctl only before installing any overlay. No own rc or manifest
is shipped. KernelSU magic-mount layout puts separate partitions under system/.
