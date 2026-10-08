//! Legacy vendor reads run in a fresh fail-closed Landlock+seccomp process.
use crate::{abi::BootModule, invalid, misc};
use generic_bootctl_core::{Backend, HealthOnSuccess, Merge, Operation, Slot, State};
use std::{
    ffi::{CStr, CString, c_char, c_int, c_long, c_void},
    io,
    path::{Path, PathBuf},
    process::Command,
};
pub struct Vendor {
    library: PathBuf,
    misc: PathBuf,
    slots: u32,
}
impl Vendor {
    pub fn new(library: PathBuf, misc: PathBuf) -> io::Result<Self> {
        let mut this = Self {
            library,
            misc,
            slots: 0,
        };
        this.slots = this.read_state()?.slots.len() as u32;
        Ok(this)
    }
}
impl Backend for Vendor {
    fn slot_count(&self) -> u32 {
        self.slots
    }
    fn read_state(&mut self) -> io::Result<State> {
        let output = Command::new("/data/adb/modules/generic_bootctl/impl/bootctl-unified")
            .arg("--vendor-probe")
            .arg(&self.library)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
        let text =
            std::str::from_utf8(&output.stdout).map_err(|_| invalid("invalid vendor response"))?;
        let values: Vec<u32> = text
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(|_| invalid("invalid vendor values"))?;
        if values.len() < 2 {
            return Err(invalid("short vendor response"));
        }
        let n = values[0] as usize;
        let active = values[1] as usize;
        if !(1..=4).contains(&n) || active >= n || values.len() != 2 + 2 * n {
            return Err(invalid("invalid vendor slots"));
        }
        let mut slots = Vec::with_capacity(n);
        for i in 0..n {
            if values[2 + i * 2] > 1 || values[3 + i * 2] > 1 {
                return Err(invalid("invalid vendor bool"));
            }
            slots.push(Slot {
                priority: if i == active { 2 } else { 1 },
                tries: values[2 + i * 2] as u8,
                successful: values[3 + i * 2] != 0,
                verity_corrupted: false,
            });
        }
        Ok(State {
            slots,
            max_priority: 2,
            max_tries: 1,
            clear_successful_on_activate: false,
            successful_requires_bootable: false,
            health_on_success: HealthOnSuccess::ResetToOne,
        })
    }
    fn commit(&mut self, _: &State, _: Operation) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "vendor parity backend is always read-only",
        ))
    }
    fn read_merge(&mut self) -> io::Result<Merge> {
        misc::read_merge(&self.misc)
    }
    fn write_merge(&mut self, _: Merge) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "vendor parity backend is always read-only",
        ))
    }
}
unsafe extern "C" {
    fn syscall(number: c_long, ...) -> c_long;
    fn prctl(option: c_int, ...) -> c_int;
    fn close(fd: c_int) -> c_int;
}
#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}
#[repr(C)]
struct Filter {
    code: u16,
    jt: u8,
    jf: u8,
    k: u32,
}
#[repr(C)]
struct Program {
    len: u16,
    filter: *const Filter,
}
fn sandbox() -> io::Result<()> {
    // Landlock syscall numbers are shared by aarch64/x86_64. ABI >=3 protects
    // truncation as well as writes. An empty allow-list denies all write opens.
    // SAFETY: syscall arguments match linux/landlock.h; no pointers on ABI query.
    let abi = unsafe { syscall(444, std::ptr::null::<u8>(), 0usize, 1u32) };
    if abi < 3 {
        return Err(io::Error::other(
            "vendor parity requires Landlock ABI >=3; refusing unsandboxed vendor code",
        ));
    }
    let rights: u64 = ((1u64 << 15) - 1) & !((1 << 0) | (1 << 2) | (1 << 3));
    // SAFETY: rights is a live landlock_ruleset_attr v1; kernel copies its 8 bytes.
    let fd = unsafe { syscall(444, &rights as *const u64, 8usize, 0u32) } as c_int;
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: PR_SET_NO_NEW_PRIVS and landlock_restrict_self use scalar arguments.
    let result = unsafe {
        if prctl(38, 1 as c_long, 0 as c_long, 0 as c_long, 0 as c_long) != 0 {
            -1
        } else {
            syscall(446, fd, 0u32) as c_int
        }
    };
    // SAFETY: fd is owned and is closed exactly once after restriction attempt.
    unsafe {
        close(fd);
    }
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    #[cfg(target_arch = "aarch64")]
    let (arch, ioctl) = (0xc00000b7u32, 29u32);
    #[cfg(target_arch = "x86_64")]
    let (arch, ioctl) = (0xc000003eu32, 16u32);
    // Reject alternate syscall ABIs and all ioctls (including storage mutations).
    let filters = [
        Filter {
            code: 0x20,
            jt: 0,
            jf: 0,
            k: 4,
        },
        Filter {
            code: 0x15,
            jt: 1,
            jf: 0,
            k: arch,
        },
        Filter {
            code: 0x06,
            jt: 0,
            jf: 0,
            k: 0x80000000,
        },
        Filter {
            code: 0x20,
            jt: 0,
            jf: 0,
            k: 0,
        },
        Filter {
            code: 0x45,
            jt: 0,
            jf: 1,
            k: 0x40000000,
        },
        Filter {
            code: 0x06,
            jt: 0,
            jf: 0,
            k: 0x80000000,
        },
        Filter {
            code: 0x15,
            jt: 0,
            jf: 1,
            k: ioctl,
        },
        Filter {
            code: 0x06,
            jt: 0,
            jf: 0,
            k: 0x00050001,
        },
        Filter {
            code: 0x06,
            jt: 0,
            jf: 0,
            k: 0x7fff0000,
        },
    ];
    let program = Program {
        len: filters.len() as u16,
        filter: filters.as_ptr(),
    };
    // SAFETY: kernel copies the live sock_fprog and its filter slice synchronously.
    if unsafe {
        prctl(
            22,
            2 as c_long,
            &program as *const Program,
            0 as c_long,
            0 as c_long,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
/// Called only by a dedicated child process, before any vendor code is loaded.
pub fn probe(path: &Path) -> io::Result<()> {
    let path = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| invalid("NUL library path"))?;
    sandbox()?;
    // SAFETY: library is explicitly supplied from backed-up vendor files; code
    // runs only after write opens/truncation and all ioctls have been denied.
    unsafe {
        let handle = dlopen(path.as_ptr(), 2);
        if handle.is_null() {
            return Err(io::Error::other("vendor dlopen failed"));
        }
        let module = dlsym(handle, c"HMI".as_ptr()).cast::<BootModule>();
        if module.is_null() {
            return Err(invalid("vendor HMI absent"));
        }
        let m = &*module;
        if m.common.tag != 0x48574d54
            || m.common.module_api_version != 1
            || m.common.id.is_null()
            || CStr::from_ptr(m.common.id) != c"bootctrl"
        {
            return Err(invalid("vendor boot module ABI mismatch"));
        }
        (m.init.ok_or_else(|| invalid("vendor init absent"))?)(module);
        let n = (m.number.ok_or_else(|| invalid("vendor count absent"))?)(module);
        let active = (m
            .active
            .ok_or_else(|| invalid("vendor getActiveBootSlot absent; cannot claim parity"))?)(
            module,
        );
        if !(1..=4).contains(&n) || active >= n {
            return Err(invalid("invalid vendor count/active"));
        }
        let bootable = m
            .is_bootable
            .ok_or_else(|| invalid("vendor bootability absent"))?;
        let successful = m
            .is_successful
            .ok_or_else(|| invalid("vendor successful absent"))?;
        let mut output = format!("{n} {active}");
        for s in 0..n {
            let b = bootable(module, s);
            let ok = successful(module, s);
            if !(0..=1).contains(&b) || !(0..=1).contains(&ok) {
                return Err(invalid("vendor read failed"));
            }
            output.push_str(&format!(" {b} {ok}"));
        }
        println!("{output}");
    }
    Ok(())
}
