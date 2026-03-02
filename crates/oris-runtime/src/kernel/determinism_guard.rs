//! Determinism guard: trap handlers for clock, randomness, and thread spawn in Replay/Verify.
//!
//! When the kernel is in [crate::kernel::kernel_mode::KernelMode::Replay] or
//! [crate::kernel::kernel_mode::KernelMode::Verify], these traps immediately fail
//! execution if code attempts clock access, hardware randomness, or uncontrolled
//! thread spawning, so the same run guarantees an identical event stream.

use sha2::{Digest, Sha256};

use crate::kernel::event::{EventStore, SequencedEvent};
use crate::kernel::identity::{RunId, Seq};
use crate::kernel::kernel_mode::KernelMode;
use crate::kernel::KernelError;

/// Guard that enforces determinism in Replay/Verify mode.
///
/// Call [DeterminismGuard::check_clock_access], [DeterminismGuard::check_random_access],
/// or [DeterminismGuard::check_spawn_access] before performing the corresponding
/// operation; they return [Err] in Replay/Verify mode.
#[derive(Clone, Debug)]
pub struct DeterminismGuard {
    pub mode: KernelMode,
}

impl DeterminismGuard {
    pub fn new(mode: KernelMode) -> Self {
        Self { mode }
    }

    /// Call before reading the system clock. Fails in Replay/Verify.
    pub fn check_clock_access(&self) -> Result<(), KernelError> {
        if self.mode.traps_nondeterminism() {
            return Err(KernelError::Driver(
                "clock access not allowed in Replay/Verify mode (determinism guard)".into(),
            ));
        }
        Ok(())
    }

    /// Call before using hardware randomness. Fails in Replay/Verify.
    pub fn check_random_access(&self) -> Result<(), KernelError> {
        if self.mode.traps_nondeterminism() {
            return Err(KernelError::Driver(
                "hardware randomness not allowed in Replay/Verify mode (determinism guard)".into(),
            ));
        }
        Ok(())
    }

    /// Call before spawning a new thread. Fails in Replay/Verify.
    pub fn check_spawn_access(&self) -> Result<(), KernelError> {
        if self.mode.traps_nondeterminism() {
            return Err(KernelError::Driver(
                "thread spawn not allowed in Replay/Verify mode (determinism guard)".into(),
            ));
        }
        Ok(())
    }
}

/// Computes a deterministic SHA-256 hash of the event stream for a run.
///
/// Used in Record mode to store the hash and in Verify mode to detect replay mismatches.
pub fn event_stream_hash(store: &dyn EventStore, run_id: &RunId) -> Result<[u8; 32], KernelError> {
    const FROM: Seq = 1;
    let events = store.scan(run_id, FROM)?;
    Ok(compute_event_stream_hash(&events))
}

/// Computes SHA-256 hash of the canonical serialized event sequence.
pub fn compute_event_stream_hash(events: &[SequencedEvent]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for se in events {
        let canonical = serde_json::to_string(&se).unwrap_or_default();
        hasher.update(canonical.as_bytes());
    }
    hasher.finalize().into()
}

/// Verifies that the current event stream for `run_id` matches `expected_hash`.
/// Returns [Err] if the hash does not match (replay mismatch).
pub fn verify_event_stream_hash(
    store: &dyn EventStore,
    run_id: &RunId,
    expected_hash: &[u8; 32],
) -> Result<(), KernelError> {
    let actual = event_stream_hash(store, run_id)?;
    if actual != *expected_hash {
        return Err(KernelError::Driver(format!(
            "replay mismatch: event stream hash differs from expected (determinism verify)"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::event::Event;
    use crate::kernel::event_store::InMemoryEventStore;

    #[test]
    fn guard_normal_allows_clock() {
        let g = DeterminismGuard::new(KernelMode::Normal);
        assert!(g.check_clock_access().is_ok());
    }

    #[test]
    fn guard_replay_traps_clock() {
        let g = DeterminismGuard::new(KernelMode::Replay);
        let e = g.check_clock_access().unwrap_err();
        assert!(e.to_string().contains("Replay"));
    }

    #[test]
    fn guard_verify_traps_spawn() {
        let g = DeterminismGuard::new(KernelMode::Verify);
        let e = g.check_spawn_access().unwrap_err();
        assert!(e.to_string().contains("spawn"));
    }

    #[test]
    fn event_stream_hash_deterministic() {
        let store = InMemoryEventStore::new();
        let run_id: RunId = "r1".into();
        store
            .append(
                &run_id,
                &[
                    Event::StateUpdated {
                        step_id: Some("a".into()),
                        payload: serde_json::json!([1]),
                    },
                    Event::Completed,
                ],
            )
            .unwrap();
        let h1 = event_stream_hash(&store, &run_id).unwrap();
        let h2 = event_stream_hash(&store, &run_id).unwrap();
        assert_eq!(h1, h2, "same stream must yield same hash");
    }

    #[test]
    fn verify_event_stream_hash_mismatch_fails() {
        let store = InMemoryEventStore::new();
        let run_id: RunId = "r2".into();
        store.append(&run_id, &[Event::Completed]).unwrap();
        let expected = [0u8; 32];
        let result = verify_event_stream_hash(&store, &run_id, &expected);
        assert!(result.is_err());
    }
}
