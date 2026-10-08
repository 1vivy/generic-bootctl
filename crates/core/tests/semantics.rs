use generic_bootctl_core::*;
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
struct Memory {
    state: State,
    merge: Merge,
    writes: Arc<AtomicUsize>,
}
impl Backend for Memory {
    fn read_state(&mut self) -> io::Result<State> {
        Ok(self.state.clone())
    }
    fn commit(&mut self, state: &State, _: Operation) -> io::Result<()> {
        self.state = state.clone();
        self.writes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn read_merge(&mut self) -> io::Result<Merge> {
        Ok(self.merge)
    }
    fn write_merge(&mut self, m: Merge) -> io::Result<()> {
        self.merge = m;
        self.writes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}
fn fixture() -> (Service, Arc<AtomicUsize>, Arc<AtomicBool>) {
    let writes = Arc::new(AtomicUsize::new(0));
    let memory = Memory {
        state: State {
            slots: vec![
                Slot {
                    priority: 15,
                    tries: 1,
                    successful: true,
                    verity_corrupted: true
                };
                2
            ],
            max_priority: 15,
            max_tries: 6,
            clear_successful_on_activate: false,
            successful_requires_bootable: true,
        },
        merge: Merge {
            status: MergeStatus::Snapshotted,
            source: 1,
        },
        writes: writes.clone(),
    };
    let gate = Arc::new(AtomicBool::new(false));
    let g = gate.clone();
    (
        Service::new(
            Box::new(memory),
            0,
            Box::new(move || g.load(Ordering::Relaxed)),
        ),
        writes,
        gate,
    )
}
#[test]
fn write_gate_never_calls_backend_mutation() {
    let (mut s, writes, g) = fixture();
    assert_eq!(s.mark_successful(), Err(COMMAND_FAILED));
    assert_eq!(s.set_active(1), Err(COMMAND_FAILED));
    assert_eq!(s.set_unbootable(0), Err(COMMAND_FAILED));
    assert_eq!(
        s.set_merge_status(MergeStatus::Merging),
        Err(COMMAND_FAILED)
    );
    assert_eq!(writes.load(Ordering::Relaxed), 0);
    g.store(true, Ordering::Relaxed);
    s.mark_successful().unwrap();
    g.store(false, Ordering::Relaxed);
    assert_eq!(s.mark_successful(), Err(COMMAND_FAILED));
    assert_eq!(writes.load(Ordering::Relaxed), 1);
}
#[test]
fn misc_semantics_and_invalid_slots() {
    let (mut s, _, g) = fixture();
    g.store(true, Ordering::Relaxed);
    assert_eq!(s.active_slot(), Ok(0));
    assert_eq!(s.number_slots(), Ok(2));
    assert_eq!(s.suffix(99), Ok(c""));
    assert_eq!(s.suffix(1), Ok(c"_b"));
    for slot in [2, u32::MAX] {
        assert_eq!(s.is_bootable(slot), Err(INVALID_SLOT));
        assert_eq!(s.is_successful(slot), Err(INVALID_SLOT));
        assert_eq!(s.set_active(slot), Err(INVALID_SLOT));
        assert_eq!(s.set_unbootable(slot), Err(INVALID_SLOT));
    }
    s.set_active(1).unwrap();
    assert_eq!(s.active_slot(), Ok(1));
    assert_eq!(s.is_successful(1), Ok(true));
    s.set_unbootable(0).unwrap();
    assert_eq!(s.is_bootable(0), Ok(false));
    assert_eq!(s.is_successful(0), Ok(false));
    s.mark_successful().unwrap();
    assert_eq!(s.is_bootable(0), Ok(true));
    assert_eq!(s.is_successful(0), Ok(true));
}
#[test]
fn merge_status_and_frozen_transactions() {
    let (mut s, _, g) = fixture();
    g.store(true, Ordering::Relaxed);
    assert_eq!(s.merge_status(), Ok(MergeStatus::Snapshotted));
    for status in [
        MergeStatus::None,
        MergeStatus::Unknown,
        MergeStatus::Snapshotted,
        MergeStatus::Merging,
        MergeStatus::Cancelled,
    ] {
        s.set_merge_status(status).unwrap();
        assert_eq!(
            s.merge_status(),
            Ok(if status == MergeStatus::Snapshotted {
                MergeStatus::None
            } else {
                status
            })
        );
    }
    assert_eq!(s.execute(11, 5), Err(COMMAND_FAILED));
    assert_eq!(s.execute(5, -1), Ok(Reply::Text(c"")));
    assert_eq!(s.execute(6, -1), Err(INVALID_SLOT));
    assert_eq!(s.execute(16_777_215, 0), Ok(Reply::Int(1)));
}
