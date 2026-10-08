//! linux/android/binder.h LP64 ABI. A single looper also services reentrant
//! hwservicemanager interfaceChain calls while waiting for add's reply.
use super::{SharedService, dispatch, registration};
use crate::parcel::Parcel;
use std::{
    ffi::{c_int, c_ulong, c_void},
    fs::{File, OpenOptions},
    io,
    os::fd::AsRawFd,
    sync::OnceLock,
};
const MAP_SIZE: usize = 1024 * 1024 - 8192;
const fn cmd(direction: u32, kind: u8, n: u8, size: u32) -> u32 {
    (direction << 30) | (size << 16) | ((kind as u32) << 8) | n as u32
}
const fn bc(n: u8, size: u32) -> u32 {
    cmd(1, b'c', n, size)
}
const fn br(n: u8, size: u32) -> u32 {
    cmd(2, b'r', n, size)
}
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Transaction {
    target: u64,
    cookie: u64,
    code: u32,
    flags: u32,
    sender_pid: i32,
    sender_euid: u32,
    data_size: u64,
    offsets_size: u64,
    buffer: u64,
    offsets: u64,
}
#[repr(C)]
#[derive(Default)]
struct WriteRead {
    write_size: u64,
    write_consumed: u64,
    write_buffer: u64,
    read_size: u64,
    read_consumed: u64,
    read_buffer: u64,
}
const _: () = assert!(std::mem::size_of::<Transaction>() == 64);
unsafe extern "C" {
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    fn mmap(
        addr: *mut c_void,
        len: usize,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        offset: i64,
    ) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> c_int;
    fn close(fd: c_int) -> c_int;
}
#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(name: *const std::ffi::c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const std::ffi::c_char) -> *mut c_void;
}
fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}
static SYSPROPS: OnceLock<unsafe extern "C" fn()> = OnceLock::new();
struct Driver {
    file: File,
    map: *mut c_void,
}
impl Drop for Driver {
    fn drop(&mut self) {
        // SAFETY: map is the successful mmap owned by this Driver.
        unsafe {
            munmap(self.map, MAP_SIZE);
        }
    }
}
impl Driver {
    fn open() -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/hwbinder")?;
        let mut version = 0i32;
        // SAFETY: ioctl writes one binder_version integer into a live local.
        if unsafe {
            ioctl(
                file.as_raw_fd(),
                cmd(3, b'b', 9, 4) as c_ulong,
                &mut version,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        if version != 8 {
            return Err(invalid("hwbinder requires LP64 protocol 8"));
        }
        let threads = 0u32;
        // SAFETY: BINDER_SET_MAX_THREADS reads this u32. One explicit looper is used.
        if unsafe { ioctl(file.as_raw_fd(), cmd(1, b'b', 5, 4) as c_ulong, &threads) } < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: read-only private binder mapping, offset zero, fd stays owned.
        let map = unsafe { mmap(std::ptr::null_mut(), MAP_SIZE, 1, 2, file.as_raw_fd(), 0) };
        if map as isize == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { file, map })
    }
    fn exchange(&self, write: &[u8], read: &mut [u8]) -> io::Result<usize> {
        let mut wr = WriteRead {
            write_size: write.len() as u64,
            write_buffer: write.as_ptr() as u64,
            read_size: read.len() as u64,
            read_buffer: read.as_mut_ptr() as u64,
            ..Default::default()
        };
        loop {
            // SAFETY: exact binder_write_read layout; buffers live through ioctl.
            let result = unsafe {
                ioctl(
                    self.file.as_raw_fd(),
                    cmd(3, b'b', 1, 48) as c_ulong,
                    &mut wr,
                )
            };
            if result >= 0 {
                break;
            }
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
        if wr.write_consumed != wr.write_size {
            return Err(invalid("partial hwbinder write"));
        }
        Ok(wr.read_consumed as usize)
    }
    fn send(&self, command: u32, bytes: &[u8]) -> io::Result<()> {
        let mut out = [0u8; 76];
        if bytes.len() > 72 {
            return Err(invalid("oversized binder command"));
        }
        out[..4].copy_from_slice(&command.to_le_bytes());
        out[4..4 + bytes.len()].copy_from_slice(bytes);
        self.exchange(&out[..4 + bytes.len()], &mut [])?;
        Ok(())
    }
    fn transaction(&self, command: u32, code: u32, p: &Parcel, status: bool) -> io::Result<()> {
        let t = Transaction {
            target: if command == bc(18, 72) {
                u32::MAX as u64
            } else {
                0
            },
            code,
            flags: if status {
                8
            } else if command == bc(17, 72) {
                0x10
            } else {
                0
            },
            data_size: p.data.len() as u64,
            offsets_size: (p.offsets.len() * 8) as u64,
            buffer: p.data.as_ptr() as u64,
            offsets: p.offsets.as_ptr() as u64,
            ..Default::default()
        };
        let mut bytes = [0u8; 72];
        // SAFETY: Transaction has no padding or uninitialized fields (64-byte ABI).
        bytes[..64].copy_from_slice(unsafe {
            std::slice::from_raw_parts((&t as *const Transaction).cast::<u8>(), 64)
        });
        bytes[64..].copy_from_slice(&p.buffers_size().to_le_bytes());
        self.send(command, &bytes)
    }
    fn mapped(&self, ptr: u64, len: u64) -> io::Result<&[u8]> {
        let start = self.map as u64;
        let end = start + MAP_SIZE as u64;
        if ptr < start || ptr.checked_add(len).is_none_or(|v| v > end) {
            return Err(invalid("parcel outside binder mapping"));
        }
        // SAFETY: range checked against the live read-only mmap; kernel owns data
        // until BC_FREE_BUFFER. No view survives that command.
        Ok(unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) })
    }
    fn free(&self, t: &Transaction) -> io::Result<()> {
        let data = self.mapped(t.buffer, t.data_size)?;
        let offsets = self.mapped(t.offsets, t.offsets_size)?;
        for off in offsets.as_chunks::<8>().0 {
            let offset = u64::from_le_bytes(*off) as usize;
            let object = data
                .get(offset..)
                .ok_or_else(|| invalid("bad binder object offset"))?;
            let kind = read32(object, 0)?;
            if kind == 0x6664_2a85 {
                // BINDER_TYPE_FD
                let fd = read32(object, 8)? as i32;
                // SAFETY: kernel-delivered fd is owned by this incoming parcel.
                unsafe {
                    close(fd);
                }
            } else if kind == 0x6664_6185 {
                // BINDER_TYPE_FDA
                let count = read64(object, 8)?;
                let parent = read64(object, 16)? as usize;
                let parent_offset = read64(object, 24)?;
                let off = offsets
                    .get(parent * 8..parent * 8 + 8)
                    .ok_or_else(|| invalid("bad FD parent"))?;
                let parent_pos = u64::from_le_bytes(off.try_into().expect("eight bytes")) as usize;
                let parent_obj = data
                    .get(parent_pos..)
                    .ok_or_else(|| invalid("bad FD buffer"))?;
                if read32(parent_obj, 0)? != crate::parcel::PTR {
                    return Err(invalid("FD parent not buffer"));
                }
                let length = read64(parent_obj, 16)?;
                let bytes = count
                    .checked_mul(4)
                    .ok_or_else(|| invalid("FD count overflow"))?;
                if parent_offset.checked_add(bytes).is_none_or(|v| v > length) {
                    return Err(invalid("FD array bounds"));
                }
                let fds = self.mapped(read64(parent_obj, 8)? + parent_offset, bytes)?;
                for fd in fds.as_chunks::<4>().0 {
                    // SAFETY: FDA contains owned descriptors installed by binder.
                    unsafe {
                        close(i32::from_le_bytes(*fd));
                    }
                }
            }
        }
        self.send(bc(3, 8), &t.buffer.to_le_bytes())
    }
    fn pump(
        &self,
        service: &SharedService,
        minor: u8,
        waiting: bool,
    ) -> io::Result<Option<Vec<u8>>> {
        let mut read = [0u8; 4096];
        let count = self.exchange(&[], &mut read)?;
        let mut pos = 0;
        let mut reply = None;
        while pos < count {
            let command = read32(&read[..count], pos)?;
            pos += 4;
            let size = ((command >> 16) & 0x3fff) as usize;
            let payload = read
                .get(pos..pos + size)
                .filter(|_| pos + size <= count)
                .ok_or_else(|| invalid("truncated binder event"))?;
            pos += size;
            match command {
                c if c == br(2, 64) || c == br(3, 64) || c == br(2, 72) => {
                    // SAFETY: checked event size contains full Transaction; the
                    // stream isn't aligned so read_unaligned is required.
                    let t =
                        unsafe { std::ptr::read_unaligned(payload.as_ptr().cast::<Transaction>()) };
                    let data = self.mapped(t.buffer, t.data_size)?;
                    if c == br(3, 64) {
                        if !waiting {
                            return Err(invalid("unexpected binder reply"));
                        }
                        if t.flags & 8 != 0 {
                            let status = read32(data, 0)? as i32;
                            self.free(&t)?;
                            return Err(invalid(&format!("binder transaction failed: {status}")));
                        }
                        reply = Some(data.to_vec());
                        self.free(&t)?;
                    } else {
                        // BHwBinder disallows IBase::debug from application UIDs.
                        let result = if t.code == 256131655 && t.sender_euid % 100000 >= 10000 {
                            Err(-1)
                        } else if t.target == 1 && t.cookie == 1 {
                            dispatch(service, minor, t.code, t.flags, data)
                        } else {
                            Err(-74)
                        };
                        if t.flags & 1 == 0 {
                            match result {
                                Ok(Some(p)) => self.transaction(bc(18, 72), 0, &p, false)?,
                                Ok(None) => return Err(invalid("synchronous request lacks reply")),
                                Err(status) => {
                                    let mut p = Parcel::default();
                                    p.scalar(status);
                                    self.transaction(bc(18, 72), 0, &p, true)?;
                                }
                            }
                        }
                        self.free(&t)?;
                    }
                }
                c if c == br(7, 16) || c == br(8, 16) => {
                    self.send(bc(if c == br(7, 16) { 8 } else { 9 }, 16), payload)?;
                }
                c if c == br(9, 16) || c == br(10, 16) => {} // object retained for process lifetime
                c if c == br(0, 4) => {
                    return Err(invalid(&format!(
                        "binder driver error: {}",
                        read32(payload, 0)? as i32
                    )));
                }
                c if c == cmd(0, b'r', 5, 0)
                    || c == cmd(0, b'r', 17, 0)
                    || c == cmd(0, b'r', 18, 0) =>
                {
                    return Err(invalid("dead/failed/frozen hwbinder transaction"));
                }
                c if c == cmd(0, b'r', 1, 0)
                    || c == cmd(0, b'r', 6, 0)
                    || c == cmd(0, b'r', 12, 0)
                    || c == cmd(0, b'r', 19, 0) => {}
                c if c == cmd(0, b'r', 13, 0) => {
                    return Err(invalid("unexpected thread spawn request"));
                }
                _ => return Err(invalid(&format!("unsupported binder event {command:#x}"))),
            }
        }
        Ok(reply)
    }
    fn call(
        &self,
        service: &SharedService,
        minor: u8,
        code: u32,
        p: &Parcel,
    ) -> io::Result<Vec<u8>> {
        self.transaction(bc(17, 72), code, p, false)?;
        loop {
            if let Some(reply) = self.pump(service, minor, true)? {
                return Ok(reply);
            }
        }
    }
}
fn read32(data: &[u8], offset: usize) -> io::Result<u32> {
    Ok(u32::from_le_bytes(
        data.get(offset..offset + 4)
            .ok_or_else(|| invalid("short parcel"))?
            .try_into()
            .expect("four bytes"),
    ))
}
fn read64(data: &[u8], offset: usize) -> io::Result<u64> {
    Ok(u64::from_le_bytes(
        data.get(offset..offset + 8)
            .ok_or_else(|| invalid("short parcel"))?
            .try_into()
            .expect("eight bytes"),
    ))
}
pub fn run(service: SharedService, minor: u8) -> io::Result<()> {
    if minor > 2 {
        return Err(invalid("unsupported HIDL minor"));
    }
    let callback = load_sysprops()?;
    SYSPROPS
        .set(callback)
        .map_err(|_| invalid("HIDL runtime already initialized"))?;
    let d = Driver::open()?;
    // ProcessState::getContextObject -> getStrongProxyForHandle(0):
    // BpHwBinder constructor retains weak handle, onFirstRef retains strong.
    d.send(bc(4, 4), &0u32.to_le_bytes())?;
    d.send(bc(5, 4), &0u32.to_le_bytes())?;
    d.send(cmd(0, b'c', 12, 0), &[])?; // BC_ENTER_LOOPER
    let reply = d.call(&service, minor, 2, &registration())?;
    if read32(&reply, 0)? != 0 || read32(&reply, 4)? != 1 {
        return Err(invalid("hwservicemanager rejected IBootControl/default"));
    }
    loop {
        d.pump(&service, minor, false)?;
    }
}
fn load_sysprops() -> io::Result<unsafe extern "C" fn()> {
    // SAFETY: terminated names, exact libutils C++ ABI; loaded handle is retained.
    unsafe {
        let handle = dlopen(c"libutils.so".as_ptr(), 2);
        if handle.is_null() {
            return Err(invalid("libutils unavailable for IBase sysprops"));
        }
        let pointer = dlsym(handle, c"_ZN7android21report_sysprop_changeEv".as_ptr());
        if pointer.is_null() {
            return Err(invalid("libutils report_sysprop_change unavailable"));
        }
        Ok(std::mem::transmute::<*mut c_void, unsafe extern "C" fn()>(
            pointer,
        ))
    }
}
pub fn notify_sysprops() {
    // SAFETY: runtime registration installs the checked libutils ABI callback.
    unsafe {
        SYSPROPS.get().expect("registered HIDL runtime")();
    }
}
#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
