use generic_bootctl_backends::{
    crc32,
    misc::Misc,
    qcom::{BootLun, Qcom},
};
use generic_bootctl_core::{Backend, Operation, Service};
use std::{
    fs, io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};
struct Temp(PathBuf);
impl Temp {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(format!("generic-bootctl-{}-{name}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn misc_image() -> Vec<u8> {
    let mut b = vec![0xa5; 65536];
    let c = &mut b[2048..2080];
    c[..4].copy_from_slice(b"_a\0\0");
    c[4..8].copy_from_slice(b"BCAB");
    c[8] = 1;
    c[9] = 0xc2;
    c[10] = 1;
    c[12] = 0x9f;
    c[14] = 0xef;
    let crc = crc32(&c[..28]);
    c[28..].copy_from_slice(&crc.to_le_bytes());
    b[32768] = 2;
    b[32769..32773].copy_from_slice(&0x56740ab0u32.to_le_bytes());
    b[32773] = 0;
    b[32774] = 0;
    b
}
#[test]
fn misc_preserves_layout_and_crc() {
    let t = Temp::new("misc");
    let p = t.0.join("misc");
    let original = misc_image();
    fs::write(&p, &original).unwrap();
    let m = Misc::open(&p).unwrap();
    let mut s = Service::new(Box::new(m), 0, Box::new(|| true));
    s.set_active(1).unwrap();
    let changed = fs::read(&p).unwrap();
    let mut expected = original.clone();
    expected[2060] = 0x9e;
    expected[2062] = 0xef;
    expected[2063] &= !1;
    let crc = crc32(&expected[2048..2076]);
    expected[2076..2080].copy_from_slice(&crc.to_le_bytes());
    assert_eq!(changed, expected);
    s.set_unbootable(1).unwrap();
    assert!(!s.is_bootable(1).unwrap());
    s.mark_successful().unwrap();
    assert!(s.is_successful(0).unwrap());
    assert_eq!(crc32(b"123456789"), 0xcbf43926);
    let mut bad = original;
    bad[2052] ^= 1;
    fs::write(&p, bad).unwrap();
    assert!(Misc::open(&p).is_err());
}
#[test]
fn readonly_misc_is_byte_identical() {
    let t = Temp::new("readonly");
    let p = t.0.join("misc");
    let b = misc_image();
    fs::write(&p, &b).unwrap();
    let mut s = Service::new(Box::new(Misc::open(&p).unwrap()), 0, Box::new(|| false));
    assert!(s.set_active(1).is_err());
    assert!(s.mark_successful().is_err());
    assert!(s.set_unbootable(0).is_err());
    assert_eq!(fs::read(p).unwrap(), b);
}
fn put32(b: &mut [u8], i: usize, n: u32) {
    b[i..i + 4].copy_from_slice(&n.to_le_bytes());
}
fn put64(b: &mut [u8], i: usize, n: u64) {
    b[i..i + 8].copy_from_slice(&n.to_le_bytes());
}
fn gpt(stem: &str, block: usize) -> Vec<u8> {
    let mut b = vec![0; block * 64];
    let mut entries = vec![0u8; 512];
    for slot in 0..2 {
        let e = &mut entries[slot * 128..(slot + 1) * 128];
        e[..16].fill(slot as u8 + 1);
        e[16..32].fill(slot as u8 + 0x40);
        put64(e, 32, 8 + slot as u64 * 8);
        put64(e, 40, 15 + slot as u64 * 8);
        put64(e, 48, (0x10u64 << 56) | 1);
        e[54] = if slot == 0 { 0x7f } else { 0x3a };
        let name = format!("{stem}_{}", if slot == 0 { 'a' } else { 'b' });
        for (i, c) in name.encode_utf16().enumerate() {
            e[56 + i * 2..58 + i * 2].copy_from_slice(&c.to_le_bytes());
        }
    }
    for (lba, other, array) in [(1, 63, 2), (63, 1, 62)] {
        let mut h = vec![0; block];
        h[..8].copy_from_slice(b"EFI PART");
        put32(&mut h, 8, 0x10000);
        put32(&mut h, 12, 92);
        put64(&mut h, 24, lba);
        put64(&mut h, 32, other);
        put64(&mut h, 40, 4);
        put64(&mut h, 48, 60);
        h[56..72].fill(0xaa);
        put64(&mut h, 72, array);
        put32(&mut h, 80, 4);
        put32(&mut h, 84, 128);
        put32(&mut h, 88, crc32(&entries));
        let crc = crc32(&h[..92]);
        put32(&mut h, 16, crc);
        b[lba as usize * block..(lba as usize + 1) * block].copy_from_slice(&h);
        b[array as usize * block..array as usize * block + 512].copy_from_slice(&entries);
    }
    b
}
struct Lun(Arc<AtomicU8>);
impl BootLun for Lun {
    fn read(&mut self) -> io::Result<u8> {
        Ok(self.0.load(Ordering::Relaxed))
    }
    fn write(&mut self, n: u8) -> io::Result<()> {
        self.0.store(n, Ordering::Relaxed);
        Ok(())
    }
}
fn verify_gpt(b: &[u8], block: usize) {
    for lba in [1, 63] {
        let mut h = b[lba * block..lba * block + 92].to_vec();
        let stored = u32::from_le_bytes(h[16..20].try_into().unwrap());
        h[16..20].fill(0);
        assert_eq!(crc32(&h), stored);
        let array = u64::from_le_bytes(h[72..80].try_into().unwrap()) as usize * block;
        assert_eq!(
            crc32(&b[array..array + 512]),
            u32::from_le_bytes(h[88..92].try_into().unwrap())
        );
    }
}
#[test]
fn qti_transforms_both_copies_across_lus() {
    let t = Temp::new("qti");
    let a = t.0.join("lu0");
    let b = t.0.join("lu4");
    let misc = t.0.join("misc");
    fs::write(&a, gpt("boot", 512)).unwrap();
    fs::write(&b, gpt("vendor_boot", 4096)).unwrap();
    fs::write(&misc, misc_image()).unwrap();
    let lun = Arc::new(AtomicU8::new(1));
    let mut q = Qcom::images(
        &[(a.clone(), 512), (b.clone(), 4096)],
        misc,
        Box::new(Lun(lun.clone())),
    )
    .unwrap();
    let state = q.read_state().unwrap();
    assert!(state.slots[0].successful);
    q.commit(&state, Operation::SetActive(1)).unwrap();
    assert_eq!(lun.load(Ordering::Relaxed), 2);
    for (p, block) in [(&a, 512), (&b, 4096)] {
        let image = fs::read(p).unwrap();
        verify_gpt(&image, block);
        for array in [2, 62] {
            let e = &image[array * block..array * block + 256];
            assert_eq!(e[54], 0x7b);
            assert_eq!(e[182], 0x3f);
            assert_eq!(&e[..16], &[2; 16]);
            assert_eq!(&e[128..144], &[1; 16]);
            assert_eq!(e[55], 0x10);
            assert_eq!(e[48], 1);
        }
    }
    q.commit(&state, Operation::MarkSuccessful(1)).unwrap();
    q.commit(&state, Operation::SetUnbootable(1)).unwrap();
    let image = fs::read(&a).unwrap();
    assert_eq!(image[2 * 512 + 128 + 54], 0xff);
    let state = q.read_state().unwrap();
    assert!(!state.slots[1].bootable());
    assert!(state.slots[1].successful);
    verify_gpt(&image, 512);
}
#[test]
fn qti_refuses_disagreeing_copies_without_repair() {
    let t = Temp::new("bad-gpt");
    let p = t.0.join("lu");
    let mut b = gpt("boot", 512);
    b[62 * 512 + 54] ^= 64;
    fs::write(&p, &b).unwrap();
    assert!(
        Qcom::images(
            &[(p.clone(), 512)],
            t.0.join("misc"),
            Box::new(Lun(Arc::new(AtomicU8::new(1))))
        )
        .is_err()
    );
    assert_eq!(fs::read(p).unwrap(), b);
}

#[test]
fn discovered_misc_count_and_suffix_survive_storage_loss() {
    let t = Temp::new("cached-count");
    let p = t.0.join("misc");
    fs::write(&p, misc_image()).unwrap();
    let backend = Misc::open(&p).unwrap();
    assert_eq!(backend.slot_count(), 2);
    fs::remove_file(&p).unwrap();
    let mut service = Service::new(Box::new(backend), 0, Box::new(|| false));
    assert_eq!(service.number_slots(), Ok(2));
    assert_eq!(service.suffix(1), Ok(c"_b"));
    assert_eq!(service.suffix(2), Ok(c""));
    assert_eq!(
        service.is_bootable(2),
        Err(generic_bootctl_core::INVALID_SLOT)
    );
    assert_eq!(
        service.is_bootable(0),
        Err(generic_bootctl_core::COMMAND_FAILED)
    );
}
