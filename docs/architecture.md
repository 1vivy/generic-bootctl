# Architecture

**Status (2026-10-08)** — Implemented, host/image-testable; not device-qualified.

`bootctl-unified` discovers the backend, constructs one `core::Service`, then
registers it without reading any VINTF manifest: the service managers already refuse
a name the device does not declare. AIDL V1/default is tried first (what AOSP's
`BootControlClient` prefers whenever it is declared); only if servicemanager refuses it
are HIDL 1.2, 1.1 and 1.0/default tried against hwservicemanager. AIDL uses the NDK C
ABI; HIDL uses raw hwbinder ioctls and scatter-gather parcels. The property gate is
evaluated on every mutation. The runtime once parsed manifests itself and died with
EACCES in an enforcing `hal_bootctl_default`, which may not read `system_file`
(`/system/etc/vintf`); manifest parsing now only backs the `--install-plan` tooling.
`crates/aidl` serializes replies and maps -1 INVALID_SLOT / -2 COMMAND_FAILED;
`crates/hidl` maps these onto CommandResult/BoolResult without bypassing core.
`crates/legacy` uses the identical service and translates these errors to errno.
Its Rust `HAL_MODULE_INFO_SYM` static is exported as **HMI**, exactly as hardware.h
requires. It does not export a second, nonstandard alias.

`bootctl-unified` is a serving-only library plus the standalone module's binary.
`bootctl_unified::serve(service)` is the only library entry: it registers one
`core::Service` and never decides how the stock HAL was replaced. Replacement is the
consumer's payload method. The standalone module's own method, `--install-plan` (find
the stock service and legacy libraries for a same-path overlay), `--vendor-probe` and
serving `generic_bootctl_backends::service`, lives in the binary behind the default
`standalone` feature. Another consumer (for example one that stops the stock service and
starts its own) builds its own `Service` and links the library with
`default-features = false`.

`core::Backend: Send` has cached `slot_count(&self) -> u32`, a default no-op
`prepare(&mut self)`, `read_state`, `commit(&State, Operation)`, `read_merge`,
and `write_merge`. State contains normalized slot metadata and native-policy
parameters: maximum priority/tries, successful-bit clearing on activation,
whether successful queries require bootability, and `health_on_success`.
`HealthOnSuccess::ResetToOne` is the default: successful writes set tries=1
and preserve priority. All native backends use it. Consumers may select
`PreserveNonZero` to set successful=true while retaining nonzero tries/priority
and raising either zero field to 1. QTI encodes bootability as the
inverse unbootable bit and active selection as normalized priority 2 versus 1;
its native commit uses Operation to preserve fields the QTI HAL leaves alone.
AOSP misc uses native 4-bit priorities and 3-bit retry counters.

The public `Service` methods are the shared transport contract:
`new(Box<dyn Backend>, current: u32, Box<dyn Fn() -> bool + Send>)`, `current_slot`,
`number_slots`, `suffix`, `active_slot`, `is_bootable`, `is_successful`,
`mark_successful`, `set_active`, `set_unbootable`, `merge_status`, and
`set_merge_status`. Fallible methods return `Result<T, i32>`. Suffix returns a
static CStr, including the empty string for invalid slots. Invalid slot queries
and mutators return -1; backend/gate failures return -2. HIDL owns its own wire
error mapping and must serialize shared Service access; do not bypass the gate.
`execute` is specifically the frozen AIDL V1 dispatch, not a HIDL transaction ABI.

Count/suffix queries use only the immutable count cached when a backend is opened
or probed (QTI validates exactly two slots). Invalid slots in both boolean queries
and both slot setters return INVALID_SLOT **before** preparation or state reads.
`prepare` precedes each valid state-dependent operation, including merge reads and
writes (AIDL codes 1,4,6,7,8,9,10,11), but never current-slot/count/suffix or
interface metadata queries (2,3,5,16777214,16777215). Write refusal may return before
preparation. Preparation must preserve read-only guarantees; it does not bypass
the write gate. A state read whose slot count differs from the cached count fails.

The XML reader accepts ordinary manifest elements, attributes, comments and XML
processing instructions, but no DTD/entities, overrides, custom instances or
unsupported versions. It does not pretend to replace libvintf's full merge rules.
Those unsupported constructs fail closed. Both transports register non-lazily
and only when declared. HIDL inherits older minors in interfaceChain; its
registration handles hwservicemanager's reentrant interfaceChain transaction.
See [HIDL wire derivation](hidl-wire.md) for the driver and marshalling contracts.

Install-plan discovery is exposed as `--install-plan ROOT` for host fake-root tests.
It reads the service stanza associated with the boot interface, not the first
service in an rc file. Binary content can associate stock rc files lacking an
`interface` line. Multiple matches or command arguments are refused. The plan is
computed against the stock bootctl before any overlay exists. No own rc or manifest
is shipped. The module magic-mount layout puts separate partitions under system/.
