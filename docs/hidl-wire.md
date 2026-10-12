# HIDL wire contract

The imported AOSP sources are reference-only. `crates/hidl` talks to
`/dev/hwbinder` with protocol 8 (LP64), a read-only Binder mapping, one explicit
looper and `BINDER_WRITE_READ`. No libhidlbase/libhwbinder dependency is used.
AIDL and HIDL threads share the same `Arc<Mutex<Service>>`; a transport failure
terminates the unified process rather than leaving a partial registration alive.

## Golden-byte derivation

The host tests in `crates/hidl/src/tests.rs` derive expected bytes from pinned
libhwbinder `Parcel.cpp`, libhidl `HidlBinderSupport.cpp`, the boot `.hal` files
and `current.txt` hashes. Byte order is little-endian on both shipped ABIs.
Only runtime addresses in BINDER_TYPE_PTR objects are zeroed for comparison;
all other object fields, offsets, and actual pointed-to buffers are checked.

* `writeInterfaceToken` calls `writeCString`: UTF-8 descriptor followed by NUL,
  rounded to four bytes. A boot descriptor is 39 bytes plus NUL: exactly 40.
  The manager descriptor is 41 bytes plus NUL and two padding bytes: 44.
* All inline scalars occupy four bytes. `writeBool` is `writeInt8`, with the
  remaining three bytes zero-filled by `writeAligned`. Slot, BoolResult and
  MergeStatus are 32-bit values. Each synchronous HIDL reply starts with the
  four-byte `Status::EX_NONE` (zero); transport errors use TF_STATUS_CODE instead.
* LP64 `binder_buffer_object` is 40 bytes: type `85 2a 74 70`, flags u32,
  pointer u64, length u64, parent-object index u64, parent offset u64. Inline
  objects retain Parcel's four-byte padding; SG payloads contribute their lengths
  rounded to eight bytes to `buffers_size`.
* `hidl_string` is a 16-byte buffer: pointer at 0, length u32 at 8, ownership
  byte at 12 and zero padding. Its NUL-terminated bytes are a second PTR object
  with HAS_PARENT=1, parent equal to the header's object index, offset=0.
  The kernel fixes the pointer in the receiving copy; no sender address is
  dereferenced by a client.
* `CommandResult` is 24 bytes: success at 0, string header at 8 (length at 16).
  Its child string therefore uses parent offset=8. An empty success reply has
  inline objects at offsets 4 and 44, lengths 24 and 1, SG size 32.
* `hidl_vec<T>` is a 16-byte header with count u32 at 8. Its element array is
  an embedded PTR at parent offset 0. For interfaceChain, each 16-byte string
  element has its own embedded character buffer with parent offset `16*i`.
  Hash vectors instead embed `count*32` contiguous bytes.
* DebugInfo is 24 bytes: PID i32 at 0, pointer u64 at 8, architecture i32 at
  16, with zero alignment padding. Like AOSP's non-instrumented release build,
  the pointer is hidden (zero), PID is the process PID, architecture is IS_64BIT.
  A null debug native handle is an inline uint64 zero, followed by the options
  vector's two PTR objects. An app UID cannot call debug, matching BHwBinder.
* IServiceManager::add is code 2, token for manager 1.0, a `hidl_string` named
  `default`, then a 24-byte flat BINDER_TYPE_BINDER object (`85 2a 62 73`),
  flags `0x900` (accept FDs, inherit RT, SCHED_NORMAL priority 0), stable local
  node/cookie IDs. Object offsets are 44,84,124;
  SG size is 16+align8(8)=24. The reply is status zero followed by boolean true.

Boot methods are numbered in inheritance/declaration order:

| Code | Interface token | Argument | Reply after status |
| --- | --- | --- | --- |
| 1 | boot 1.0 | none | uint32 slot count |
| 2 | boot 1.0 | none | uint32 current slot |
| 3 | boot 1.0 | none | CommandResult |
| 4 | boot 1.0 | uint32 slot | CommandResult |
| 5 | boot 1.0 | uint32 slot | CommandResult |
| 6 | boot 1.0 | uint32 slot | BoolResult |
| 7 | boot 1.0 | uint32 slot | BoolResult |
| 8 | boot 1.0 | uint32 slot | hidl_string suffix |
| 9 | boot 1.1 | MergeStatus | bool success |
| 10 | boot 1.1 | none | MergeStatus |
| 11 | boot 1.2 | none | uint32 active slot |

Methods route only through the core's named methods. Command failures return
`success=false` and an error string; invalid-slot boolean queries return -1.
Read/backend failures that cannot be represented by the method's result use a
failed Binder transaction rather than inventing a slot or merge state.

## Registration and IBase

The service exposes the highest manifest-declared minor, plus each ancestor and
IBase in interfaceChain/hash order. It does not claim a higher undeclared minor.
`ProcessState::getContextObject` resolves to `getStrongProxyForHandle(0)`;
BpHwBinder's constructor retains handle 0 with BC_INCREFS and onFirstRef sends
BC_ACQUIRE. There is no manager `get` request to obtain another handle. The
non-lazy add transaction services nested interfaceChain callbacks while waiting
for its reply, acknowledges BR_INCREFS/BR_ACQUIRE, and frees all received Binder
buffers. Local node IDs remain alive for the entire process.

IBase has interfaceChain, descriptor, hashes, ping, debug, DebugInfo, oneway
instrumentation and sysprop notification. The default debug callback is empty,
like AOSP; transferred file descriptors are released after the callback. There
are no instrumented Rust callbacks to enable. Sysprop notification invokes
libutils' callback reporter through a checked dynamic symbol (no link-time
dependency). `linkToDeath` and `unlinkToDeath` are **not HIDL
wire methods**: BpHwBase uses the kernel death-notification protocol on the
remote node; the kernel delivers those notifications when this process dies.
Unknown transaction codes, incorrect interface tokens, truncated scalar inputs,
undeclared minor methods and incorrect oneway flags are rejected.

Host golden contracts and dual-ABI builds do not establish runtime SELinux,
hwservicemanager, reboot, OTA or bootloader correctness. The same-path install plan
still refuses custom instances, unsupported versions/transports, manifest
merging constructs it cannot resolve, ambiguous/critical init services and all
other documented safety conditions.

Generated BnHw stubs reject failed `enforceInterface` with `BAD_TYPE`
(`INT32_MIN+1`), including missing or unterminated interface tokens. The minimal
`generateCpp-dispatch.cpp` reference excerpt and golden dispatcher tests pin that
status separately from `PERMISSION_DENIED` used for app-UID debug refusal.
