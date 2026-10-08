//! LP64 Android libhardware boot_control_module_t ABI over the shared service.
use generic_bootctl_backends::abi::{BootModule, HwModule};
use generic_bootctl_core::{INVALID_SLOT, Service};
use std::{
    ffi::c_char,
    sync::{LazyLock, Mutex},
};
static SERVICE: LazyLock<Mutex<Option<Service>>> = LazyLock::new(|| Mutex::new(None));
fn with<T>(f: impl FnOnce(&mut Service) -> Result<T, i32>) -> Result<T, i32> {
    let mut service = SERVICE.lock().map_err(|_| -5)?;
    if service.is_none() {
        *service = Some(generic_bootctl_backends::service().map_err(|_| -5)?);
    }
    f(service.as_mut().ok_or(-5)?)
}
fn errno(result: Result<(), i32>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(INVALID_SLOT) => -22,
        Err(_) => -5,
    }
}
fn boolean(result: Result<bool, i32>) -> i32 {
    match result {
        Ok(v) => i32::from(v),
        Err(INVALID_SLOT) => -22,
        Err(_) => -5,
    }
}
unsafe extern "C" fn init(_: *mut BootModule) {
    if with(|_| Ok(())).is_err() {
        eprintln!("generic_bootctl legacy backend initialization failed");
    }
}
unsafe extern "C" fn number(_: *mut BootModule) -> u32 {
    with(Service::number_slots).unwrap_or(0)
}
unsafe extern "C" fn current(_: *mut BootModule) -> u32 {
    with(|s| Ok(s.current_slot())).unwrap_or(0)
}
unsafe extern "C" fn successful(_: *mut BootModule) -> i32 {
    errno(with(Service::mark_successful))
}
unsafe extern "C" fn activate(_: *mut BootModule, slot: u32) -> i32 {
    errno(with(|s| s.set_active(slot)))
}
unsafe extern "C" fn unbootable(_: *mut BootModule, slot: u32) -> i32 {
    errno(with(|s| s.set_unbootable(slot)))
}
unsafe extern "C" fn is_bootable(_: *mut BootModule, slot: u32) -> i32 {
    boolean(with(|s| s.is_bootable(slot)))
}
unsafe extern "C" fn is_successful(_: *mut BootModule, slot: u32) -> i32 {
    boolean(with(|s| s.is_successful(slot)))
}
unsafe extern "C" fn suffix(_: *mut BootModule, slot: u32) -> *const c_char {
    with(|s| s.suffix(slot))
        .ok()
        .filter(|s| !s.is_empty())
        .map_or(std::ptr::null(), |s| s.as_ptr())
}
unsafe extern "C" fn active(_: *mut BootModule) -> u32 {
    with(Service::active_slot).unwrap_or(0)
}
// hardware.h defines HAL_MODULE_INFO_SYM as HMI; libhardware dlsym uses "HMI".
#[unsafe(export_name = "HMI")]
pub static HAL_MODULE_INFO_SYM: BootModule = BootModule {
    common: HwModule {
        tag: 0x48574d54,
        module_api_version: 1,
        hal_api_version: 0x0100,
        id: c"bootctrl".as_ptr(),
        name: c"Generic Boot Control".as_ptr(),
        author: c"1vivy".as_ptr(),
        methods: std::ptr::null_mut(),
        dso: std::ptr::null_mut(),
        reserved: [0; 25],
    },
    init: Some(init),
    number: Some(number),
    current: Some(current),
    successful: Some(successful),
    activate: Some(activate),
    unbootable: Some(unbootable),
    is_bootable: Some(is_bootable),
    suffix: Some(suffix),
    is_successful: Some(is_successful),
    active: Some(active),
    reserved: [std::ptr::null_mut(); 30],
};
const _: () = assert!(std::mem::size_of::<HwModule>() == 248);
const _: () = assert!(std::mem::size_of::<BootModule>() == 568);
