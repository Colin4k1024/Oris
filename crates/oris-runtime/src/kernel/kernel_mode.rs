//! Kernel execution mode: controls determinism and replay/verify behavior.
//!
//! In **Replay** and **Verify** modes, nondeterministic operations (clock, randomness,
//! thread spawn) must be trapped so the same run yields an identical event stream.

use serde::{Deserialize, Serialize};

/// Runtime mode for the kernel: determines whether nondeterministic operations are allowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KernelMode {
    /// Normal execution: no restrictions.
    Normal,
    /// Recording a run for later replay; event stream is the source of truth.
    Record,
    /// Replaying from the event log; no live side effects, traps on clock/random/spawn.
    Replay,
    /// Verifying: same as Replay but also check event stream hash matches expected.
    Verify,
}

impl Default for KernelMode {
    fn default() -> Self {
        KernelMode::Normal
    }
}

impl KernelMode {
    /// Returns true if clock access, hardware randomness, and thread spawn must be trapped.
    pub fn traps_nondeterminism(self) -> bool {
        matches!(self, KernelMode::Replay | KernelMode::Verify)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_does_not_trap() {
        assert!(!KernelMode::Normal.traps_nondeterminism());
    }

    #[test]
    fn record_does_not_trap() {
        assert!(!KernelMode::Record.traps_nondeterminism());
    }

    #[test]
    fn replay_traps() {
        assert!(KernelMode::Replay.traps_nondeterminism());
    }

    #[test]
    fn verify_traps() {
        assert!(KernelMode::Verify.traps_nondeterminism());
    }
}
