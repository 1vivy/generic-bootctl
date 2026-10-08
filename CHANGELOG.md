# Changelog

## 0.1.0 — 2026-10-07

- Split generic Rust core, frozen AIDL V1, LP64 legacy ABI and native backends.
- Add fail-closed VINTF/init discovery and same-path KernelSU module packaging.
- Add misc CRC/layout and multi-LU QTI GPT/UFS operations behind a live write gate.
- Add confined read-only vendor parity, host/image/install contracts and NDK r29 builds.
- Import pinned minimal AOSP/QTI reference sources with provenance.
- Implement dependency-free raw hwbinder HIDL boot 1.0–1.2 and IBase, with
  scatter-gather string/vector fixups and non-lazy hwservicemanager registration.
- Register AIDL, HIDL or both exactly as declared, around one serialized core;
  accept HIDL-only/mixed installations without relaxing other safety aborts.
- Add golden-byte HIDL requests/replies, descriptor/hash and registration tests.
- Extend the shared core for consumers with cached slot count, invalid-slot
  precedence, a preparation hook and explicit mark-successful health policy;
  existing native backend health policy remains unchanged.
- Match generated AOSP BnHw BAD_TYPE status for incorrect, absent and
  unterminated interface tokens.
