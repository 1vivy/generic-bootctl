//! Hand-transcribed LP64 hardware.h/boot_control.h ABI; see docs/header-import.md.
use std::ffi::{c_char, c_void};
#[repr(C)]
pub struct HwModule {
    pub tag: u32,
    pub module_api_version: u16,
    pub hal_api_version: u16,
    pub id: *const c_char,
    pub name: *const c_char,
    pub author: *const c_char,
    pub methods: *mut c_void,
    pub dso: *mut c_void,
    pub reserved: [u64; 25],
}
#[repr(C)]
pub struct BootModule {
    pub common: HwModule,
    pub init: Option<unsafe extern "C" fn(*mut Self)>,
    pub number: Option<unsafe extern "C" fn(*mut Self) -> u32>,
    pub current: Option<unsafe extern "C" fn(*mut Self) -> u32>,
    pub successful: Option<unsafe extern "C" fn(*mut Self) -> i32>,
    pub activate: Option<unsafe extern "C" fn(*mut Self, u32) -> i32>,
    pub unbootable: Option<unsafe extern "C" fn(*mut Self, u32) -> i32>,
    pub is_bootable: Option<unsafe extern "C" fn(*mut Self, u32) -> i32>,
    pub suffix: Option<unsafe extern "C" fn(*mut Self, u32) -> *const c_char>,
    pub is_successful: Option<unsafe extern "C" fn(*mut Self, u32) -> i32>,
    pub active: Option<unsafe extern "C" fn(*mut Self) -> u32>,
    pub reserved: [*mut c_void; 30],
}
// SAFETY: these structs describe immutable static C callback tables. Mutation of
// vendor-owned instances occurs only in an isolated single-threaded probe process.
unsafe impl Sync for BootModule {}
