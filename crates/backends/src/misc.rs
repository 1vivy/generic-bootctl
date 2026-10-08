//! AOSP bootloader_control at misc+2048; never auto-initialize damaged metadata.
use crate::{crc32, invalid};
use generic_bootctl_core::{Backend, HealthOnSuccess, Merge, MergeStatus, Operation, Slot, State};
use std::{
    fs::{File, OpenOptions},
    io,
    os::unix::fs::FileExt,
    path::{Path, PathBuf},
};
pub const CONTROL_OFFSET: u64 = 2048;
pub const MERGE_OFFSET: u64 = 32768;
#[derive(Clone)]
pub struct Misc {
    path: PathBuf,
    slots: u32,
}
impl Misc {
    pub fn open(path: &Path) -> io::Result<Self> {
        let mut this = Self {
            path: path.into(),
            slots: 0,
        };
        this.slots = u32::from(this.control()?[9] & 7);
        Ok(this)
    }
    fn control(&self) -> io::Result<[u8; 32]> {
        let mut b = [0; 32];
        File::open(&self.path)?.read_exact_at(&mut b, CONTROL_OFFSET)?;
        if &b[4..8] != b"BCAB"
            || b[8] != 1
            || !(1..=4).contains(&(b[9] & 7))
            || crc32(&b[..28]) != u32::from_le_bytes(b[28..32].try_into().unwrap())
        {
            return Err(invalid(
                "invalid misc bootloader_control magic/version/count/CRC",
            ));
        }
        Ok(b)
    }
}
pub(crate) fn write_verified(path: &Path, offset: u64, bytes: &[u8]) -> io::Result<()> {
    let f = OpenOptions::new().read(true).write(true).open(path)?;
    f.write_all_at(bytes, offset)?;
    f.sync_all()?;
    let mut check = vec![0; bytes.len()];
    f.read_exact_at(&mut check, offset)?;
    if check != bytes {
        return Err(io::Error::other("storage readback mismatch"));
    }
    Ok(())
}
pub fn read_merge(path: &Path) -> io::Result<Merge> {
    let b = merge_bytes(path)?;
    Ok(Merge {
        status: MergeStatus::try_from(i32::from(b[5]))
            .map_err(|_| invalid("invalid merge status"))?,
        source: u32::from(b[6]),
    })
}
fn merge_bytes(path: &Path) -> io::Result<[u8; 64]> {
    let mut b = [0; 64];
    File::open(path)?.read_exact_at(&mut b, MERGE_OFFSET)?;
    if b[0] != 2 || b[1..5] != 0x56740ab0u32.to_le_bytes() {
        return Err(invalid("uninitialized/invalid virtual A/B message"));
    }
    Ok(b)
}
pub fn write_merge(path: &Path, merge: Merge) -> io::Result<()> {
    let mut b = merge_bytes(path)?;
    b[5] = merge.status as u8;
    b[6] = u8::try_from(merge.source).map_err(|_| invalid("invalid merge source"))?;
    write_verified(path, MERGE_OFFSET, &b)
}
impl Backend for Misc {
    fn slot_count(&self) -> u32 {
        self.slots
    }
    fn read_state(&mut self) -> io::Result<State> {
        let b = self.control()?;
        let slots = (0..usize::from(b[9] & 7))
            .map(|i| {
                let x = b[12 + i * 2];
                Slot {
                    priority: x & 15,
                    tries: (x >> 4) & 7,
                    successful: x & 128 != 0,
                    verity_corrupted: b[13 + i * 2] & 1 != 0,
                }
            })
            .collect();
        Ok(State {
            slots,
            max_priority: 15,
            max_tries: 6,
            clear_successful_on_activate: false,
            successful_requires_bootable: true,
            health_on_success: HealthOnSuccess::ResetToOne,
        })
    }
    fn commit(&mut self, state: &State, _: Operation) -> io::Result<()> {
        let mut b = self.control()?;
        if state.slots.len() != usize::from(b[9] & 7) {
            return Err(invalid("slot count changed"));
        }
        for (i, s) in state.slots.iter().enumerate() {
            if s.priority > 15 || s.tries > 7 {
                return Err(invalid("slot field overflow"));
            }
            b[12 + i * 2] = s.priority | s.tries << 4 | u8::from(s.successful) << 7;
            b[13 + i * 2] = (b[13 + i * 2] & !1) | u8::from(s.verity_corrupted);
        }
        let crc = crc32(&b[..28]);
        b[28..].copy_from_slice(&crc.to_le_bytes());
        write_verified(&self.path, CONTROL_OFFSET, &b)
    }
    fn read_merge(&mut self) -> io::Result<Merge> {
        read_merge(&self.path)
    }
    fn write_merge(&mut self, merge: Merge) -> io::Result<()> {
        write_merge(&self.path, merge)
    }
}
