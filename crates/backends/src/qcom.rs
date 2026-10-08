//! QTI attribute byte 54, type-GUID activation swap, both GPT copies on every LU.
use crate::{crc32, invalid, misc};
use generic_bootctl_core::{Backend, Merge, Operation, Slot, State};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io,
    os::unix::fs::FileExt,
    path::{Path, PathBuf},
};
mod ufs;
pub use ufs::BootLun;
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}
#[derive(Clone)]
struct Copy {
    header: Vec<u8>,
    entries: Vec<u8>,
    header_offset: u64,
    entries_offset: u64,
}
impl Copy {
    fn load(f: &File, block: u64, lba: u64, size: u64) -> io::Result<Self> {
        let offset = lba
            .checked_mul(block)
            .ok_or_else(|| invalid("GPT offset overflow"))?;
        if offset.checked_add(block).is_none_or(|n| n > size) {
            return Err(invalid("GPT header out of bounds"));
        }
        let mut header = vec![0; block as usize];
        f.read_exact_at(&mut header, offset)?;
        if &header[..8] != b"EFI PART"
            || u32_at(&header, 8) != 0x10000
            || u64_at(&header, 24) != lba
        {
            return Err(invalid("invalid GPT signature/revision/location"));
        }
        let hs = u32_at(&header, 12) as usize;
        if !(92..=header.len()).contains(&hs) {
            return Err(invalid("invalid GPT header size"));
        }
        let expected = u32_at(&header, 16);
        header[16..20].fill(0);
        let actual = crc32(&header[..hs]);
        header[16..20].copy_from_slice(&expected.to_le_bytes());
        if expected != actual {
            return Err(invalid("invalid GPT header CRC"));
        }
        let count = u32_at(&header, 80) as usize;
        let stride = u32_at(&header, 84) as usize;
        if count == 0 || count > 16384 || stride != 128 {
            return Err(invalid("unsupported GPT entry geometry"));
        }
        let entries_offset = u64_at(&header, 72)
            .checked_mul(block)
            .ok_or_else(|| invalid("GPT array overflow"))?;
        let len = count * stride;
        if entries_offset
            .checked_add(len as u64)
            .is_none_or(|n| n > size)
        {
            return Err(invalid("GPT entries out of bounds"));
        }
        let first = u64_at(&header, 40)
            .checked_mul(block)
            .ok_or_else(|| invalid("GPT first overflow"))?;
        let last = u64_at(&header, 48)
            .checked_add(1)
            .and_then(|n| n.checked_mul(block))
            .ok_or_else(|| invalid("GPT last overflow"))?;
        if first >= last
            || last > size
            || !(entries_offset + len as u64 <= first || entries_offset >= last)
        {
            return Err(invalid("GPT entries overlap usable space"));
        }
        if entries_offset < offset + block && offset < entries_offset + len as u64 {
            return Err(invalid("GPT entries overlap header"));
        }
        let mut entries = vec![0; len];
        f.read_exact_at(&mut entries, entries_offset)?;
        if crc32(&entries) != u32_at(&header, 88) {
            return Err(invalid("invalid GPT array CRC"));
        }
        Ok(Self {
            header,
            entries,
            header_offset: offset,
            entries_offset,
        })
    }
    fn refresh(&mut self) {
        self.header[88..92].copy_from_slice(&crc32(&self.entries).to_le_bytes());
        self.header[16..20].fill(0);
        let crc = crc32(&self.header[..u32_at(&self.header, 12) as usize]);
        self.header[16..20].copy_from_slice(&crc.to_le_bytes());
    }
    fn persist(&self, path: &Path) -> io::Result<()> {
        misc::write_verified(path, self.entries_offset, &self.entries)?;
        misc::write_verified(path, self.header_offset, &self.header)
    }
}
#[derive(Clone)]
struct Disk {
    path: PathBuf,
    block: u64,
    size: u64,
    primary: Copy,
    backup: Copy,
}
impl Disk {
    fn load(path: PathBuf, block: u64, size: u64) -> io::Result<Self> {
        if ![512, 4096].contains(&block) || size < block * 6 || !size.is_multiple_of(block) {
            return Err(invalid("invalid disk geometry"));
        }
        let f = File::open(&path)?;
        let primary = Copy::load(&f, block, 1, size)?;
        let backup = Copy::load(&f, block, size / block - 1, size)?;
        if u64_at(&primary.header, 32) != size / block - 1
            || u64_at(&backup.header, 32) != 1
            || primary.header[40..72] != backup.header[40..72]
            || primary.entries != backup.entries
        {
            return Err(invalid("GPT copies disagree; no automatic repair"));
        }
        Ok(Self {
            path,
            block,
            size,
            primary,
            backup,
        })
    }
}
/// The boot-LUN adapter is injectable for image tests; production uses UFS BSG.
pub struct Qcom {
    disks: Vec<Disk>,
    merge: PathBuf,
    lun: Box<dyn BootLun>,
}
impl Qcom {
    pub fn images(
        images: &[(PathBuf, u64)],
        merge: PathBuf,
        lun: Box<dyn BootLun>,
    ) -> io::Result<Self> {
        let mut disks = Vec::new();
        for (p, b) in images {
            disks.push(Disk::load(p.clone(), *b, fs::metadata(p)?.len())?);
        }
        let q = Self { disks, merge, lun };
        q.pairs()?;
        Ok(q)
    }
    pub fn discover(root: &Path, merge: &Path) -> io::Result<Option<Self>> {
        if !crate::property("ro.boot.bootdevice").is_some_and(|s| s.ends_with(".ufshc")) {
            return Ok(None);
        }
        let boot = root.join("dev/block/by-name/boot_a");
        if !boot.exists() {
            return Ok(None);
        }
        let partition = fs::canonicalize(boot)?;
        let name = partition
            .file_name()
            .ok_or_else(|| invalid("boot partition path"))?;
        let sys = fs::canonicalize(root.join("sys/class/block").join(name))?;
        let parent = sys
            .parent()
            .ok_or_else(|| invalid("partition sysfs parent"))?;
        let device = fs::canonicalize(parent.join("device"))?;
        let text = device.to_string_lossy();
        let controller = text
            .split("/host")
            .next()
            .ok_or_else(|| invalid("UFS controller missing"))?;
        if controller == text {
            return Err(invalid("unsupported UFS sysfs topology"));
        }
        let mut disks = Vec::new();
        for e in fs::read_dir(root.join("sys/block"))? {
            let e = e?;
            let p = e.path();
            let Ok(dev) = fs::canonicalize(p.join("device")) else {
                continue;
            };
            if !dev
                .to_string_lossy()
                .starts_with(&format!("{controller}/host"))
            {
                continue;
            }
            let block = fs::read_to_string(p.join("queue/logical_block_size"))?
                .trim()
                .parse::<u64>()
                .map_err(|_| invalid("invalid logical block size"))?;
            let size = fs::read_to_string(p.join("size"))?
                .trim()
                .parse::<u64>()
                .map_err(|_| invalid("invalid disk size"))?
                .checked_mul(512)
                .ok_or_else(|| invalid("disk size overflow"))?;
            let path = root.join("dev/block").join(e.file_name());
            let mut signature = [0; 8];
            File::open(&path)?.read_exact_at(&mut signature, block)?;
            if &signature != b"EFI PART" {
                continue;
            }
            disks.push(Disk::load(path, block, size)?);
        }
        if disks.is_empty() {
            return Ok(None);
        }
        let q = Self {
            disks,
            merge: merge.into(),
            lun: Box::new(ufs::Bsg::discover(root)?),
        };
        q.pairs()?;
        Ok(Some(q))
    }
    fn reload(&mut self) -> io::Result<()> {
        for d in &mut self.disks {
            *d = Disk::load(d.path.clone(), d.block, d.size)?;
        }
        Ok(())
    }
    fn pairs(&self) -> io::Result<BTreeMap<String, [(usize, usize); 2]>> {
        let mut candidates: BTreeMap<String, [Option<(usize, usize)>; 2]> = BTreeMap::new();
        for (disk, d) in self.disks.iter().enumerate() {
            for (index, entry) in d.primary.entries.as_chunks::<128>().0.iter().enumerate() {
                if entry[..16].iter().all(|&b| b == 0) {
                    continue;
                }
                let utf: Vec<u16> = entry[56..128]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|s| u16::from_le_bytes([s[0], s[1]]))
                    .take_while(|&c| c != 0)
                    .collect();
                let name =
                    String::from_utf16(&utf).map_err(|_| invalid("invalid GPT UTF16 name"))?;
                let (stem, slot) = if let Some(s) = name.strip_suffix("_a") {
                    (s, 0)
                } else if let Some(s) = name.strip_suffix("_b") {
                    (s, 1)
                } else {
                    continue;
                };
                if ["xbl", "xbl_config", "multiimgoem", "multiimgqti"].contains(&stem) {
                    continue;
                }
                if candidates.entry(stem.into()).or_default()[slot]
                    .replace((disk, index))
                    .is_some()
                {
                    return Err(invalid("duplicate A/B partition"));
                }
            }
        }
        let mut pairs = BTreeMap::new();
        for (stem, pair) in candidates {
            let [Some(a), Some(b)] = pair else {
                return Err(invalid("incomplete A/B pair"));
            };
            let aa = self.entry(a)[54];
            let bb = self.entry(b)[54];
            if (aa & 4 != 0) == (bb & 4 != 0) {
                return Err(invalid("QTI pair must have exactly one active attribute"));
            }
            pairs.insert(stem, [a, b]);
        }
        if !pairs.contains_key("boot") {
            return Err(invalid("QTI boot pair missing"));
        }
        Ok(pairs)
    }
    fn entry(&self, (disk, index): (usize, usize)) -> &[u8] {
        &self.disks[disk].primary.entries[index * 128..(index + 1) * 128]
    }
}
impl Backend for Qcom {
    fn read_state(&mut self) -> io::Result<State> {
        self.reload()?;
        let pair = self.pairs()?["boot"];
        let slots = pair
            .iter()
            .map(|&p| {
                let b = self.entry(p)[54];
                Slot {
                    priority: if b & 4 != 0 { 2 } else { 1 },
                    tries: u8::from(b & 128 == 0),
                    successful: b & 64 != 0,
                    verity_corrupted: false,
                }
            })
            .collect();
        Ok(State {
            slots,
            max_priority: 2,
            max_tries: 1,
            clear_successful_on_activate: true,
            successful_requires_bootable: false,
        })
    }
    fn commit(&mut self, _: &State, operation: Operation) -> io::Result<()> {
        self.reload()?;
        let pairs = self.pairs()?;
        let slot = match operation {
            Operation::MarkSuccessful(s)
            | Operation::SetActive(s)
            | Operation::SetUnbootable(s) => s as usize,
        };
        if slot > 1 {
            return Err(invalid("invalid QTI slot"));
        }
        // Read and validate the existing LUN before any GPT mutation. The caller's
        // property gate is already checked. Failure never silently skips the LUN.
        if matches!(operation, Operation::SetActive(_)) {
            let old = self.lun.read()?;
            if !(1..=2).contains(&old) {
                return Err(invalid("invalid UFS boot LUN"));
            }
        }
        for pair in pairs.values() {
            let active = usize::from(self.entry(pair[1])[54] & 4 != 0);
            let active_guid: [u8; 16] = self.entry(pair[active])[..16].try_into().unwrap();
            let inactive_guid: [u8; 16] = self.entry(pair[1 - active])[..16].try_into().unwrap();
            for (s, &(disk, index)) in pair.iter().enumerate() {
                let disk = &mut self.disks[disk];
                for copy in [&mut disk.primary, &mut disk.backup] {
                    let e = &mut copy.entries[index * 128..(index + 1) * 128];
                    match operation {
                        Operation::SetActive(_) => {
                            if s == slot {
                                e[..16].copy_from_slice(&active_guid);
                                e[54] = 0x3f;
                            } else {
                                e[..16].copy_from_slice(&inactive_guid);
                                e[54] &= !4;
                            }
                        }
                        Operation::MarkSuccessful(_) if s == slot => e[54] |= 0x40,
                        Operation::SetUnbootable(_) if s == slot => e[54] |= 0x80,
                        _ => {}
                    }
                }
            }
        }
        for d in &mut self.disks {
            d.primary.refresh();
            d.backup.refresh();
            d.backup.persist(&d.path)?;
            d.primary.persist(&d.path)?;
        }
        if matches!(operation, Operation::SetActive(_)) {
            self.lun.write(slot as u8 + 1)?;
            if self.lun.read()? != slot as u8 + 1 {
                return Err(io::Error::other("UFS boot LUN readback mismatch"));
            }
        }
        Ok(())
    }
    fn read_merge(&mut self) -> io::Result<Merge> {
        misc::read_merge(&self.merge)
    }
    fn write_merge(&mut self, merge: Merge) -> io::Result<()> {
        misc::write_merge(&self.merge, merge)
    }
}
