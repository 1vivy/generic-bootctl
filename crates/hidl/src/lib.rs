//! Raw LP64 hwbinder boot HAL, no C++ runtime or platform HIDL linkage.
#[cfg(any(target_os = "android", test))]
pub mod driver;
#[cfg(any(target_os = "android", test))]
mod parcel;
use generic_bootctl_core::Service;
#[cfg(any(target_os = "android", test))]
use generic_bootctl_core::{INVALID_SLOT, MergeStatus};
use std::sync::{Arc, Mutex};
pub const BASE: &str = "android.hidl.base@1.0::IBase";
pub type SharedService = Arc<Mutex<Service>>;
#[cfg(any(target_os = "android", test))]
use parcel::Parcel;
pub const DESCRIPTORS: [&str; 3] = [
    "android.hardware.boot@1.0::IBootControl",
    "android.hardware.boot@1.1::IBootControl",
    "android.hardware.boot@1.2::IBootControl",
];
const HASHES: [&str; 4] = [
    "7192d756aeba00aba32f4504981df8172ffca83e210c4838dabf295e53e93590",
    "07d0a252b2d8fa35887908a996ba395cf392968395fc30afab791f46e0c22a52",
    "6763dd2273b1b47f3ac68af9b66870287eba33fb5b4d66e8fe1d30ae18ce24cb",
    "ec7fd79ed02dfa85bc499426adae3ebe23ef0524f3cd6957139324b83b18ca4c",
];
pub fn interface_chain(minor: u8) -> Vec<&'static str> {
    (0..=minor as usize)
        .rev()
        .map(|i| DESCRIPTORS[i])
        .chain([BASE])
        .collect()
}
pub fn hash_chain(minor: u8) -> Vec<u8> {
    (0..=minor as usize)
        .rev()
        .chain([3])
        .flat_map(|i| {
            (0..32).map(move |j| {
                u8::from_str_radix(&HASHES[i][2 * j..2 * j + 2], 16).expect("pinned hash")
            })
        })
        .collect()
}
#[cfg(any(target_os = "android", test))]
fn registration() -> Parcel {
    let mut p = Parcel::default();
    p.token("android.hidl.manager@1.0::IServiceManager");
    p.string("default");
    p.binder();
    p
}
#[cfg(any(target_os = "android", test))]
fn dispatch(
    service: &SharedService,
    minor: u8,
    code: u32,
    flags: u32,
    data: &[u8],
) -> Result<Option<Parcel>, i32> {
    let method_minor = match code {
        1..=8 => 0,
        9..=10 => 1,
        11 => 2,
        256067662 | 256131655 | 256136003 | 256398152 | 256462420 | 256921159 | 257049926
        | 257120595 => 3,
        _ => return Err(-74),
    };
    if method_minor != 3 && method_minor > minor {
        return Err(-74);
    }
    let oneway = matches!(code, 256462420 | 257120595);
    if (flags & 1 != 0) != oneway {
        return Err(-22);
    }
    let token = if method_minor == 3 {
        BASE
    } else {
        DESCRIPTORS[method_minor as usize]
    };
    let arg = parcel::input(data, token, matches!(code, 4..=9))?;
    if oneway {
        #[cfg(target_os = "android")]
        if code == 257120595 {
            driver::notify_sysprops();
        }
        // There are no instrumented callbacks in this transport to enable.
        return Ok(None);
    }
    let mut p = Parcel::default();
    p.scalar(0); // Status::EX_NONE
    if method_minor == 3 {
        match code {
            256067662 => p.strings(&interface_chain(minor)),
            256136003 => p.string(DESCRIPTORS[minor as usize]),
            256398152 => {
                let hashes = hash_chain(minor);
                let mut header = vec![0; 16];
                header[8..12].copy_from_slice(&((hashes.len() / 32) as u32).to_le_bytes());
                let parent = p.buffer(header, None);
                p.buffer(hashes, Some((parent, 0)));
            }
            257049926 => {
                let mut info = vec![0; 24];
                info[..4].copy_from_slice(&(std::process::id() as i32).to_le_bytes());
                info[16..20].copy_from_slice(&1i32.to_le_bytes());
                p.buffer(info, None);
            }
            256921159 => {}
            // AOSP's default debug is intentionally empty. Kernel-delivered FDs
            // are closed by the driver after this synchronous callback.
            256131655 => {}
            _ => return Err(-74),
        }
        return Ok(Some(p));
    }
    let mut s = service.lock().map_err(|_| -2147483646)?;
    match code {
        1 => p.scalar(s.number_slots().map_err(|_| -2147483646)? as i32),
        2 => p.scalar(s.current_slot() as i32),
        3 => p.command(s.mark_successful()),
        4 => p.command(s.set_active(arg as u32)),
        5 => p.command(s.set_unbootable(arg as u32)),
        6 | 7 => {
            let result = if code == 6 {
                s.is_bootable(arg as u32)
            } else {
                s.is_successful(arg as u32)
            };
            p.scalar(match result {
                Ok(v) => i32::from(v),
                Err(INVALID_SLOT) => -1,
                Err(_) => return Err(-2147483646),
            });
        }
        8 => p.string(
            s.suffix(arg as u32)
                .map_err(|_| -2147483646)?
                .to_str()
                .map_err(|_| -22)?,
        ),
        9 => p.boolean(
            MergeStatus::try_from(arg)
                .and_then(|v| s.set_merge_status(v))
                .is_ok(),
        ),
        10 => p.scalar(s.merge_status().map_err(|_| -2147483646)? as i32),
        11 => p.scalar(s.active_slot().map_err(|_| -2147483646)? as i32),
        _ => return Err(-74),
    }
    Ok(Some(p))
}
#[cfg(target_os = "android")]
pub fn run(service: SharedService, minor: u8) -> Result<(), String> {
    driver::run(service, minor).map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests;
