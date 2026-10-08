use super::*;
use generic_bootctl_core::{Backend, HealthOnSuccess, Merge, Operation, Slot, State};
use std::io;
struct Memory {
    state: State,
    merge: Merge,
}
impl Backend for Memory {
    fn slot_count(&self) -> u32 {
        2
    }
    fn read_state(&mut self) -> io::Result<State> {
        Ok(self.state.clone())
    }
    fn commit(&mut self, s: &State, _: Operation) -> io::Result<()> {
        self.state = s.clone();
        Ok(())
    }
    fn read_merge(&mut self) -> io::Result<Merge> {
        Ok(self.merge)
    }
    fn write_merge(&mut self, m: Merge) -> io::Result<()> {
        self.merge = m;
        Ok(())
    }
}
fn service(writable: bool) -> SharedService {
    Arc::new(Mutex::new(Service::new(
        Box::new(Memory {
            state: State {
                slots: vec![
                    Slot {
                        priority: 15,
                        tries: 1,
                        successful: true,
                        verity_corrupted: false,
                    },
                    Slot {
                        priority: 14,
                        tries: 6,
                        successful: false,
                        verity_corrupted: false,
                    },
                ],
                max_priority: 15,
                max_tries: 6,
                clear_successful_on_activate: false,
                successful_requires_bootable: false,
                health_on_success: HealthOnSuccess::ResetToOne,
            },
            merge: Merge {
                status: MergeStatus::None,
                source: 0,
            },
        }),
        0,
        Box::new(move || writable),
    )))
}
fn request(code: u32, arg: i32) -> Parcel {
    let mut p = Parcel::default();
    p.token(match code {
        1..=8 => DESCRIPTORS[0],
        9..=10 => DESCRIPTORS[1],
        11 => DESCRIPTORS[2],
        _ => BASE,
    });
    if matches!(code, 4..=9) {
        p.scalar(arg);
    }
    if code == 256131655 {
        p.data.extend([0; 8]);
        p.strings(&[]);
    }
    p
}
// Canonical golden bytes replace only process-dependent buffer addresses with
// zero. Remaining bytes are literal LP64 binder_buffer_object fields.
fn canonical(p: &Parcel) -> Vec<u8> {
    let mut data = p.data.clone();
    for o in &p.offsets {
        let o = *o as usize;
        if u32::from_le_bytes(data[o..o + 4].try_into().unwrap()) == parcel::PTR {
            data[o + 8..o + 16].fill(0);
        }
    }
    data
}
fn hex(s: &str) -> Vec<u8> {
    s.split_whitespace()
        .map(|v| u8::from_str_radix(v, 16).unwrap())
        .collect()
}
fn object(length: u8, parent: Option<(u8, u8)>) -> Vec<u8> {
    let mut bytes = hex(
        "85 2a 74 70 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00",
    );
    bytes[16] = length;
    if let Some((index, offset)) = parent {
        bytes[4] = 1;
        bytes[24] = index;
        bytes[32] = offset;
    }
    bytes
}
#[test]
fn all_boot_requests_and_replies() {
    for code in 1..=11 {
        let p = request(code, 0);
        let mut expected = match code {
            1..=8 => b"android.hardware.boot@1.0::IBootControl\0".to_vec(),
            9..=10 => b"android.hardware.boot@1.1::IBootControl\0".to_vec(),
            _ => b"android.hardware.boot@1.2::IBootControl\0".to_vec(),
        };
        if matches!(code, 4..=9) {
            expected.extend([0; 4]);
        }
        assert_eq!(p.data, expected, "request {code}");
        let reply = dispatch(&service(true), 2, code, 0, &p.data)
            .unwrap()
            .unwrap();
        let mut expected = vec![0; 4];
        match code {
            1 => expected.extend([2, 0, 0, 0]),
            2 | 10 | 11 => expected.extend([0; 4]),
            6 | 7 | 9 => expected.extend([1, 0, 0, 0]),
            3..=5 => {
                expected.extend(object(24, None));
                expected.extend(object(1, Some((0, 8))));
                assert_eq!(
                    reply.buffers[0].as_ref(),
                    hex("01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00")
                );
                assert_eq!(reply.buffers[1].as_ref(), [0]);
                assert_eq!(reply.offsets, [4, 44]);
            }
            8 => {
                expected.extend(object(16, None));
                expected.extend(object(3, Some((0, 0))));
                assert_eq!(
                    reply.buffers[0].as_ref(),
                    hex("00 00 00 00 00 00 00 00 02 00 00 00 00 00 00 00")
                );
                assert_eq!(reply.buffers[1].as_ref(), b"_a\0");
            }
            _ => unreachable!(),
        }
        assert_eq!(canonical(&reply), expected, "reply {code}");
    }
}
#[test]
fn command_failure_and_invalid_slot() {
    let s = service(false);
    let p = request(3, 0);
    let r = dispatch(&s, 2, 3, 0, &p.data).unwrap().unwrap();
    assert_eq!(r.buffers[0][0], 0);
    assert_eq!(&r.buffers[0][16..20], [14, 0, 0, 0]);
    assert_eq!(r.buffers[1].as_ref(), b"Command failed\0");
    for code in [6, 7] {
        let p = request(code, 99);
        assert_eq!(
            dispatch(&s, 2, code, 0, &p.data).unwrap().unwrap().data,
            hex("00 00 00 00 ff ff ff ff")
        );
    }
    let p = request(9, 99);
    assert_eq!(
        dispatch(&s, 2, 9, 0, &p.data).unwrap().unwrap().data,
        [0; 8]
    );
}
#[test]
fn base_chain_hashes_and_registration() {
    for minor in 0..=2 {
        let chain = interface_chain(minor);
        assert_eq!(chain.len(), minor as usize + 2);
        assert_eq!(chain[0], DESCRIPTORS[minor as usize]);
        assert_eq!(chain.last(), Some(&BASE));
        let current = include_str!("../../../third_party/aosp/hardware/interfaces/current.txt");
        for (i, hash) in hash_chain(minor).as_chunks::<32>().0.iter().enumerate() {
            let encoded: String = hash.iter().map(|b| format!("{b:02x}")).collect();
            if i + 1 < chain.len() {
                assert!(
                    current
                        .lines()
                        .any(|l| l == format!("{encoded} {}", chain[i]))
                );
            } else {
                assert_eq!(encoded, HASHES[3]);
            }
        }
        let p = request(256067662, 0);
        let r = dispatch(&service(false), minor, 256067662, 0, &p.data)
            .unwrap()
            .unwrap();
        assert_eq!(
            r.offsets,
            (0..chain.len() + 2)
                .map(|i| 4 + 40 * i as u64)
                .collect::<Vec<_>>()
        );
        assert_eq!(r.buffers[0][8], chain.len() as u8);
        assert_eq!(r.buffers[1].len(), chain.len() * 16);
        for (i, name) in chain.iter().enumerate() {
            assert_eq!(r.buffers[i + 2].as_ref(), format!("{name}\0").as_bytes());
        }
        let p = request(256398152, 0);
        let r = dispatch(&service(false), minor, 256398152, 0, &p.data)
            .unwrap()
            .unwrap();
        assert_eq!(r.buffers[1].as_ref(), hash_chain(minor));
    }
    let p = registration();
    let mut expected = b"android.hidl.manager@1.0::IServiceManager\0\0\0".to_vec();
    expected.extend(object(16, None));
    expected.extend(object(8, Some((0, 0))));
    expected.extend(hex(
        "85 2a 62 73 00 09 00 00 01 00 00 00 00 00 00 00 01 00 00 00 00 00 00 00",
    ));
    assert_eq!(canonical(&p), expected);
    assert_eq!(p.offsets, [44, 84, 124]);
    assert_eq!(p.buffers[1].as_ref(), b"default\0");
    assert_eq!(p.buffers_size(), 24);
}
#[test]
fn base_methods_and_bad_dispatch() {
    let s = service(false);
    for code in [
        256067662, 256131655, 256136003, 256398152, 256462420, 256921159, 257049926, 257120595,
    ] {
        let p = request(code, 0);
        let oneway = matches!(code, 256462420 | 257120595);
        let r = dispatch(&s, 2, code, u32::from(oneway), &p.data).unwrap();
        assert_eq!(r.is_none(), oneway);
        if code == 256136003 {
            assert_eq!(
                r.unwrap().buffers[1].as_ref(),
                b"android.hardware.boot@1.2::IBootControl\0"
            );
        } else if code == 256921159 || code == 256131655 {
            assert_eq!(r.unwrap().data, [0; 4]);
        }
    }
    assert_eq!(dispatch(&s, 2, 99, 0, &[]).unwrap_err(), -74);
    assert_eq!(dispatch(&s, 2, 1, 0, b"bad\0").unwrap_err(), i32::MIN + 1);
    assert_eq!(
        dispatch(&s, 2, 1, 0, b"unterminated").unwrap_err(),
        i32::MIN + 1
    );
    assert_eq!(dispatch(&s, 2, 1, 0, &[]).unwrap_err(), i32::MIN + 1);
    assert_eq!(dispatch(&s, 2, 4, 0, &request(1, 0).data).unwrap_err(), -22);
    assert_eq!(dispatch(&s, 0, 9, 0, &request(9, 0).data).unwrap_err(), -74);
    assert_eq!(dispatch(&s, 2, 1, 1, &request(1, 0).data).unwrap_err(), -22);
    assert_eq!(
        dispatch(&s, 2, 257120595, 0, &request(257120595, 0).data).unwrap_err(),
        -22
    );
    // linkToDeath/unlinkToDeath have no wire transactions: BpHwBase uses binder
    // death notifications, so guessed transaction numbers must be rejected.
    assert_eq!(
        dispatch(&s, 2, 256660548, 0, &request(256660548, 0).data).unwrap_err(),
        -74
    );
}

#[test]
fn base_golden_bytes_and_shared_core() {
    let s = service(true);
    for code in [
        256067662, 256131655, 256136003, 256398152, 256462420, 256921159, 257049926, 257120595,
    ] {
        let p = request(code, 0);
        let mut expected = b"android.hidl.base@1.0::IBase\0\0\0\0".to_vec();
        if code == 256131655 {
            expected.extend([0; 8]);
            expected.extend(object(16, None));
            expected.extend(object(0, Some((0, 0))));
            assert_eq!(p.offsets, [40, 80]);
            assert_eq!(p.buffers[0].as_ref(), [0; 16]);
        }
        assert_eq!(canonical(&p), expected, "IBase request {code}");
        let oneway = matches!(code, 256462420 | 257120595);
        let r = dispatch(&s, 2, code, u32::from(oneway), &p.data).unwrap();
        if oneway {
            assert!(r.is_none());
            continue;
        }
        let r = r.unwrap();
        let mut expected = vec![0; 4];
        match code {
            256067662 => {
                expected.extend(object(16, None));
                expected.extend(object(64, Some((0, 0))));
                for i in 0..4 {
                    expected.extend(object(if i < 3 { 40 } else { 29 }, Some((1, 16 * i))));
                }
                assert_eq!(
                    r.buffers[0].as_ref(),
                    hex("00 00 00 00 00 00 00 00 04 00 00 00 00 00 00 00")
                );
                for (i, len) in [39u32, 39, 39, 28].iter().enumerate() {
                    assert_eq!(&r.buffers[1][16 * i..16 * i + 8], [0; 8]);
                    assert_eq!(&r.buffers[1][16 * i + 8..16 * i + 12], len.to_le_bytes());
                    assert_eq!(&r.buffers[1][16 * i + 12..16 * i + 16], [0; 4]);
                }
            }
            256136003 => {
                expected.extend(object(16, None));
                expected.extend(object(40, Some((0, 0))));
            }
            256398152 => {
                expected.extend(object(16, None));
                expected.extend(object(128, Some((0, 0))));
                assert_eq!(
                    r.buffers[0].as_ref(),
                    hex("00 00 00 00 00 00 00 00 04 00 00 00 00 00 00 00")
                );
            }
            257049926 => {
                expected.extend(object(24, None));
                let mut info =
                    hex("00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 01 00 00 00 00 00 00 00");
                info[..4].copy_from_slice(&std::process::id().to_le_bytes());
                assert_eq!(r.buffers[0].as_ref(), info);
            }
            256131655 | 256921159 => {}
            _ => unreachable!(),
        }
        assert_eq!(canonical(&r), expected, "IBase reply {code}");
    }
    let p = request(4, 1);
    let reply = dispatch(&s, 2, 4, 0, &p.data).unwrap().unwrap();
    assert_eq!(reply.buffers[0][0], 1);
    // AIDL's frozen execute sees the exact state changed by HIDL, not a copy.
    assert_eq!(
        s.lock().expect("test HAL mutex").execute(1, 0),
        Ok(generic_bootctl_core::Reply::Int(1))
    );
}
