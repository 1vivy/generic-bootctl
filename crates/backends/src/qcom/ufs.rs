use crate::invalid;
use std::{
    fs::{self, OpenOptions},
    io,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
};
pub trait BootLun: Send {
    fn read(&mut self) -> io::Result<u8>;
    fn write(&mut self, lun: u8) -> io::Result<()>;
}
pub struct Bsg {
    path: PathBuf,
}
impl Bsg {
    pub fn discover(root: &Path) -> io::Result<Self> {
        let mut paths = Vec::new();
        for e in fs::read_dir(root.join("dev"))? {
            let e = e?;
            let name = e.file_name();
            let name = name.to_string_lossy();
            if name == "ufs-bsg"
                || name
                    .strip_prefix("ufs-bsg")
                    .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            {
                paths.push(e.path());
            }
        }
        if paths.len() != 1 {
            return Err(invalid(
                "no unique UFS BSG controller; legacy ioctl/eMMC unsupported",
            ));
        }
        Ok(Self {
            path: paths.remove(0),
        })
    }
    fn query(&self, write: Option<u8>) -> io::Result<u8> {
        let f = OpenOptions::new()
            .read(true)
            .write(write.is_some())
            .open(&self.path)?;
        let mut request = [0u8; 36];
        let mut response = [0u8; 40];
        request[..4].copy_from_slice(&0x16u32.to_ne_bytes());
        request[4] = 0x16;
        request[9] = if write.is_some() { 0x81 } else { 0x01 };
        request[16] = if write.is_some() { 4 } else { 3 };
        if let Some(lun) = write {
            request[24..28].copy_from_slice(&u32::from(lun).to_be_bytes());
        }
        let mut io = SgIo {
            guard: i32::from(b'Q'),
            subprotocol: 2,
            request_len: 36,
            request: request.as_ptr() as u64,
            max_response_len: 40,
            response: response.as_mut_ptr() as u64,
            timeout: 5000,
            ..SgIo::default()
        };
        unsafe extern "C" {
            fn ioctl(fd: i32, request: std::ffi::c_ulong, ...) -> i32;
        }
        // SAFETY: SG_IO copies sg_io_v4 and the referenced live request/reply
        // buffers synchronously; all lengths exactly match the UFS UAPI structs.
        if unsafe { ioctl(f.as_raw_fd(), 0x2285, &mut io as *mut SgIo) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if io.info != 0
            || io.driver_status != 0
            || io.transport_status != 0
            || io.device_status != 0
            || i32::from_ne_bytes(response[..4].try_into().unwrap()) != 0
            || io.response_len < 40
            || response[14] != 0
        {
            return Err(io::Error::other("UFS query failed"));
        }
        let value = u32::from_be_bytes(response[28..32].try_into().unwrap());
        u8::try_from(value).map_err(|_| invalid("invalid UFS attribute"))
    }
}
impl BootLun for Bsg {
    fn read(&mut self) -> io::Result<u8> {
        self.query(None)
    }
    fn write(&mut self, lun: u8) -> io::Result<()> {
        if !(1..=2).contains(&lun) {
            return Err(invalid("invalid UFS boot LUN"));
        }
        self.query(Some(lun))?;
        if self.query(None)? != lun {
            return Err(io::Error::other("UFS boot LUN verification failed"));
        }
        Ok(())
    }
}
/// Linux UAPI linux/bsg.h sg_io_v4, LP64 and fixed-width wire pointers.
#[repr(C)]
#[derive(Default)]
struct SgIo {
    guard: i32,
    protocol: u32,
    subprotocol: u32,
    request_len: u32,
    request: u64,
    request_tag: u64,
    request_attr: u32,
    request_priority: u32,
    request_extra: u32,
    max_response_len: u32,
    response: u64,
    dout_iovec_count: u32,
    dout_xfer_len: u32,
    din_iovec_count: u32,
    din_xfer_len: u32,
    dout_xferp: u64,
    din_xferp: u64,
    timeout: u32,
    flags: u32,
    usr_ptr: u64,
    spare_in: u32,
    driver_status: u32,
    transport_status: u32,
    device_status: u32,
    retry_delay: u32,
    info: u32,
    duration: u32,
    response_len: u32,
    din_resid: i32,
    dout_resid: i32,
    generated_tag: u64,
    spare_out: u32,
    padding: u32,
}
const _: () = assert!(std::mem::size_of::<SgIo>() == 160);
