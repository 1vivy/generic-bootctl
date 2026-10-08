//! Runtime-discovered boot metadata backends. No device allow-lists.
pub mod abi;
pub mod misc;
pub mod qcom;
pub mod vendor;
use generic_bootctl_core::{Backend, Service};
use std::{ffi::CString, fs, io, path::Path};
pub(crate) fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}
pub fn property(name: &str) -> Option<String> {
    let name = CString::new(name).ok()?;
    #[cfg(target_os = "android")]
    {
        unsafe extern "C" {
            fn __system_property_get(
                name: *const std::ffi::c_char,
                value: *mut std::ffi::c_char,
            ) -> i32;
        }
        let mut value = [0u8; 92];
        // SAFETY: name is NUL-terminated and bionic guarantees PROP_VALUE_MAX (92).
        let len = unsafe { __system_property_get(name.as_ptr(), value.as_mut_ptr().cast()) };
        if len <= 0 || len as usize >= value.len() {
            return None;
        }
        String::from_utf8(value[..len as usize].to_vec()).ok()
    }
    #[cfg(not(target_os = "android"))]
    {
        std::env::var(name.to_str().ok()?).ok()
    }
}
pub fn service() -> io::Result<Service> {
    let current = match property("ro.boot.slot_suffix").as_deref() {
        Some("_a") => 0,
        Some("_b") => 1,
        Some("_c") => 2,
        Some("_d") => 3,
        _ => return Err(invalid("invalid current slot property")),
    };
    let backend = discover(Path::new("/"))?;
    Ok(Service::new(
        backend,
        current,
        Box::new(|| property(generic_bootctl_core::WRITE_PROPERTY).as_deref() == Some("1")),
    ))
}
pub fn discover(root: &Path) -> io::Result<Box<dyn Backend>> {
    let misc_path = root.join("dev/block/by-name/misc");
    // Explicitly recognized native metadata wins; ambiguity is an error, not a guess.
    let misc = misc::Misc::open(&misc_path).ok();
    let qcom = qcom::Qcom::discover(root, &misc_path)?;
    match (misc, qcom) {
        (Some(_), Some(_)) => {
            return Err(invalid(
                "both misc and QTI metadata detected; backend ambiguous",
            ));
        }
        (Some(m), None) => return Ok(Box::new(m)),
        (None, Some(q)) => return Ok(Box::new(q)),
        _ => {}
    }
    let backup = root.join("data/adb/modules/generic_bootctl/backup");
    let mut libraries = Vec::new();
    for partition in ["vendor", "odm", "system"] {
        let dir = backup.join(partition).join("lib64/hw");
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.starts_with("bootctrl.") && s.ends_with(".so"))
            {
                libraries.push(path);
            }
        }
    }
    if libraries.len() != 1 {
        return Err(invalid("no unambiguous supported boot metadata/backend"));
    }
    Ok(Box::new(vendor::Vendor::new(
        libraries.remove(0),
        misc_path,
    )))
}
