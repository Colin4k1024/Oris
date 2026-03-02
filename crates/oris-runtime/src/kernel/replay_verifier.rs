//! Replay verification API: cryptographically verify run integrity.
//!
//! This module provides [ReplayVerifier] to validate the integrity of a run:
//! - **State hash equality**: verifies event stream hash matches expected.
//! - **Tool checksum**: hashes all tool calls in the run.
//! - **Interrupt consistency**: every Interrupt must have a matching Resumed.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::kernel::determinism_guard::verify_event_stream_hash;
use crate::kernel::event::Event;
use crate::kernel::identity::RunId;
use crate::kernel::EventStore;
use crate::kernel::KernelError;

/// Verification failure reasons.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum VerificationFailure {
    /// State hash mismatch.
    StateHashMismatch { expected: String, actual: String },
    /// Tool checksum mismatch.
    ToolChecksumMismatch { expected: String, actual: String },
    /// Interrupt without a matching Resumed.
    UnmatchedInterrupt { seq: u64, value: serde_json::Value },
    /// Resumed without a matching Interrupt.
    UnmatchedResume { seq: u64 },
    /// Run not found in event store.
    RunNotFound,
}

/// Verification result: Ok(()) if all checks pass, or Err with the failure reason.
pub type VerificationResult = Result<(), VerificationFailure>;

/// Verification config: what to verify.
#[derive(Clone, Debug, Default)]
pub struct VerifyConfig {
    /// Verify state hash equality (requires expected_state_hash).
    pub verify_state_hash: bool,
    /// Expected state hash (32 bytes hex string, if verifying).
    pub expected_state_hash: Option<String>,
    /// Verify tool call checksum.
    pub verify_tool_checksum: bool,
    /// Expected tool checksum (hex string, if verifying).
    pub expected_tool_checksum: Option<String>,
    /// Verify interrupt consistency (every Interrupt has a Resumed).
    pub verify_interrupt_consistency: bool,
}

/// Replay verifier: validates run integrity.
pub struct ReplayVerifier;

impl ReplayVerifier {
    /// Verifies a run's integrity according to the config.
    pub fn verify(
        store: &dyn EventStore,
        run_id: &RunId,
        config: &VerifyConfig,
    ) -> VerificationResult {
        // Check run exists
        let head = store
            .head(run_id)
            .map_err(|e| VerificationFailure::StateHashMismatch {
                expected: "head check".into(),
                actual: e.to_string(),
            })?;
        if head == 0 {
            return Err(VerificationFailure::RunNotFound);
        }

        // 1. State hash equality
        if config.verify_state_hash {
            if let Some(expected) = &config.expected_state_hash {
                let expected_bytes =
                    hex::decode(expected).map_err(|_| VerificationFailure::StateHashMismatch {
                        expected: expected.clone(),
                        actual: "invalid hex".into(),
                    })?;
                let mut expected_arr = [0u8; 32];
                if expected_bytes.len() != 32 {
                    return Err(VerificationFailure::StateHashMismatch {
                        expected: expected.clone(),
                        actual: "wrong length".into(),
                    });
                }
                expected_arr.copy_from_slice(&expected_bytes);

                if let Err(e) = verify_event_stream_hash(store, run_id, &expected_arr) {
                    return Err(VerificationFailure::StateHashMismatch {
                        expected: expected.clone(),
                        actual: format!("verification failed: {}", e),
                    });
                }
            }
        }

        // 2. Tool checksum
        if config.verify_tool_checksum {
            let actual_checksum = compute_tool_checksum(store, run_id).map_err(|e| {
                VerificationFailure::ToolChecksumMismatch {
                    expected: "compute".into(),
                    actual: e.to_string(),
                }
            })?;
            if let Some(expected) = &config.expected_tool_checksum {
                if &actual_checksum != expected {
                    return Err(VerificationFailure::ToolChecksumMismatch {
                        expected: expected.clone(),
                        actual: actual_checksum,
                    });
                }
            }
        }

        // 3. Interrupt consistency
        if config.verify_interrupt_consistency {
            verify_interrupt_consistency(store, run_id)?;
        }

        Ok(())
    }

    /// Returns the tool call checksum for a run (for later verification).
    pub fn tool_checksum(store: &dyn EventStore, run_id: &RunId) -> Result<String, KernelError> {
        Ok(compute_tool_checksum(store, run_id)?)
    }

    /// Returns the state hash for a run (for later verification).
    pub fn state_hash(store: &dyn EventStore, run_id: &RunId) -> Result<String, KernelError> {
        let hash = crate::kernel::determinism_guard::event_stream_hash(store, run_id)?;
        Ok(hex::encode(hash))
    }
}

/// Computes SHA-256 of all tool calls in the run.
fn compute_tool_checksum(store: &dyn EventStore, run_id: &RunId) -> Result<String, KernelError> {
    let events = store.scan(run_id, 1)?;
    let mut hasher = Sha256::new();
    for se in &events {
        if let Event::ActionRequested { action_id, payload } = &se.event {
            // Include action_id and payload in checksum
            hasher.update(action_id.as_bytes());
            if let Ok(json) = serde_json::to_string(payload) {
                hasher.update(json.as_bytes());
            }
        }
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Verifies every Interrupt has a matching Resumed.
fn verify_interrupt_consistency(store: &dyn EventStore, run_id: &RunId) -> VerificationResult {
    let events = store
        .scan(run_id, 1)
        .map_err(|e| VerificationFailure::UnmatchedInterrupt {
            seq: 0,
            value: serde_json::json!(e.to_string()),
        })?;
    let mut interrupt_seqs: Vec<(u64, serde_json::Value)> = Vec::new();

    for se in &events {
        match &se.event {
            Event::Interrupted { value } => {
                interrupt_seqs.push((se.seq, value.clone()));
            }
            Event::Resumed { .. } => {
                if let Some((_, _)) = interrupt_seqs.pop() {
                    // matched - OK
                } else {
                    return Err(VerificationFailure::UnmatchedResume { seq: se.seq });
                }
            }
            _ => {}
        }
    }

    // Any unmatched Interrupts left?
    if let Some((seq, value)) = interrupt_seqs.pop() {
        return Err(VerificationFailure::UnmatchedInterrupt { seq, value });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::event_store::InMemoryEventStore;

    #[test]
    fn verify_returns_ok_when_run_not_found() {
        let store = InMemoryEventStore::new();
        let config = VerifyConfig::default();
        let result = ReplayVerifier::verify(&store, &"nonexistent".into(), &config);
        assert!(matches!(result, Err(VerificationFailure::RunNotFound)));
    }

    #[test]
    fn verify_state_hash_mismatch() {
        let store = InMemoryEventStore::new();
        let run_id: RunId = "r1".into();
        store.append(&run_id, &[Event::Completed]).unwrap();
        let config = VerifyConfig {
            verify_state_hash: true,
            expected_state_hash: Some("00".repeat(32)),
            ..Default::default()
        };
        let result = ReplayVerifier::verify(&store, &run_id, &config);
        assert!(matches!(
            result,
            Err(VerificationFailure::StateHashMismatch { .. })
        ));
    }

    #[test]
    fn verify_tool_checksum_mismatch() {
        let store = InMemoryEventStore::new();
        let run_id: RunId = "r2".into();
        store
            .append(
                &run_id,
                &[Event::ActionRequested {
                    action_id: "a1".into(),
                    payload: serde_json::json!({"tool": "foo"}),
                }],
            )
            .unwrap();
        let config = VerifyConfig {
            verify_tool_checksum: true,
            expected_tool_checksum: Some("ff".repeat(32)),
            ..Default::default()
        };
        let result = ReplayVerifier::verify(&store, &run_id, &config);
        assert!(matches!(
            result,
            Err(VerificationFailure::ToolChecksumMismatch { .. })
        ));
    }

    #[test]
    fn verify_interrupt_consistency_unmatched_interrupt() {
        let store = InMemoryEventStore::new();
        let run_id: RunId = "r3".into();
        store
            .append(
                &run_id,
                &[
                    Event::Interrupted {
                        value: serde_json::json!({"reason": "ask"}),
                    },
                    Event::Completed,
                ],
            )
            .unwrap();
        let config = VerifyConfig {
            verify_interrupt_consistency: true,
            ..Default::default()
        };
        let result = ReplayVerifier::verify(&store, &run_id, &config);
        assert!(matches!(
            result,
            Err(VerificationFailure::UnmatchedInterrupt { .. })
        ));
    }

    #[test]
    fn verify_interrupt_consistency_ok() {
        let store = InMemoryEventStore::new();
        let run_id: RunId = "r4".into();
        store
            .append(
                &run_id,
                &[
                    Event::Interrupted {
                        value: serde_json::json!({"reason": "ask"}),
                    },
                    Event::Resumed {
                        value: serde_json::json!("user input"),
                    },
                    Event::Completed,
                ],
            )
            .unwrap();
        let config = VerifyConfig {
            verify_interrupt_consistency: true,
            ..Default::default()
        };
        let result = ReplayVerifier::verify(&store, &run_id, &config);
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
    }

    #[test]
    fn tool_checksum_returns_same_for_same_calls() {
        let store = InMemoryEventStore::new();
        let run_id: RunId = "r5".into();
        store
            .append(
                &run_id,
                &[
                    Event::ActionRequested {
                        action_id: "a1".into(),
                        payload: serde_json::json!({"tool": "foo", "input": 1}),
                    },
                    Event::ActionRequested {
                        action_id: "a2".into(),
                        payload: serde_json::json!({"tool": "bar", "input": 2}),
                    },
                ],
            )
            .unwrap();
        let c1 = ReplayVerifier::tool_checksum(&store, &run_id).unwrap();
        let c2 = ReplayVerifier::tool_checksum(&store, &run_id).unwrap();
        assert_eq!(c1, c2);
    }
}
