use super::*;
#[test]
fn kernel_ioctl_abi() {
    assert_eq!(cmd(3, b'b', 1, 48), 0xc0306201);
    assert_eq!(cmd(3, b'b', 9, 4), 0xc0046209);
    assert_eq!(bc(17, 72), 0x40486311);
    assert_eq!(bc(18, 72), 0x40486312);
    assert_eq!(bc(8, 16), 0x40106308);
    assert_eq!(bc(9, 16), 0x40106309);
    assert_eq!(br(2, 64), 0x80407202);
    assert_eq!(br(2, 72), 0x80487202);
    assert_eq!(std::mem::size_of::<Transaction>(), 64);
    assert_eq!(std::mem::size_of::<WriteRead>(), 48);
    assert_eq!(std::mem::offset_of!(Transaction, buffer), 48);
    assert_eq!(std::mem::offset_of!(Transaction, offsets), 56);
    assert!(read32(&[0; 3], 0).is_err());
    assert!(read64(&[0; 7], 0).is_err());
}
