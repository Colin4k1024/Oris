//! Integration tests for the oris-economics crate.

use oris_economics::{
    compute_replay_evu, journal_from_snapshot, EvuAccount, EvuLedger, LedgerEntry, LedgerJournal,
    ReputationRecord, RoiPolicy, StakePolicy,
};

#[test]
fn reserve_publish_stake_insufficient_balance_returns_none() {
    let mut ledger = EvuLedger {
        accounts: vec![EvuAccount {
            node_id: "node-a".into(),
            balance: 0, // insufficient balance
        }],
        reputations: vec![],
    };
    let policy = StakePolicy::default();
    let result = ledger.reserve_publish_stake("node-a", &policy);
    assert!(result.is_none());
    assert_eq!(ledger.available_balance("node-a"), Some(0));
}

#[test]
fn reserve_publish_stake_unknown_node_returns_none() {
    let mut ledger = EvuLedger::default();
    let policy = StakePolicy::default();
    let result = ledger.reserve_publish_stake("unknown-node", &policy);
    assert!(result.is_none());
}

#[test]
fn penalize_validator_divergence_reduces_reputation() {
    let mut ledger = EvuLedger {
        accounts: vec![EvuAccount {
            node_id: "validator-1".into(),
            balance: 10,
        }],
        reputations: vec![ReputationRecord {
            node_id: "validator-1".into(),
            publish_success_rate: 0.8,
            validator_accuracy: 0.9,
            reuse_impact: 5,
        }],
    };
    let policy = StakePolicy::default();

    let _ = ledger.settle_remote_reuse("validator-1", true, &policy);
    let accuracy_before = ledger
        .reputations
        .iter()
        .find(|r| r.node_id == "validator-1")
        .map(|r| r.validator_accuracy)
        .unwrap();

    let settlement = ledger.penalize_validator_divergence("validator-1", &policy);
    assert_eq!(settlement.validator_delta, -policy.validator_penalty);
    let accuracy_after = ledger
        .reputations
        .iter()
        .find(|r| r.node_id == "validator-1")
        .map(|r| r.validator_accuracy)
        .unwrap();

    assert!(accuracy_after < accuracy_before);
}

#[test]
fn multi_node_publish_reuse_cycle() {
    let mut ledger = EvuLedger {
        accounts: vec![
            EvuAccount {
                node_id: "node-a".into(),
                balance: 10,
            },
            EvuAccount {
                node_id: "node-b".into(),
                balance: 5,
            },
        ],
        reputations: vec![
            ReputationRecord {
                node_id: "node-a".into(),
                publish_success_rate: 0.5,
                validator_accuracy: 0.5,
                reuse_impact: 0,
            },
            ReputationRecord {
                node_id: "node-b".into(),
                publish_success_rate: 0.5,
                validator_accuracy: 0.5,
                reuse_impact: 0,
            },
        ],
    };
    let policy = StakePolicy::default();

    let settlement = ledger.reserve_publish_stake("node-a", &policy).unwrap();
    assert_eq!(settlement.publisher_delta, -policy.publish_cost);
    assert_eq!(ledger.available_balance("node-a"), Some(9));

    let settlement = ledger.settle_remote_reuse("node-a", true, &policy);
    assert_eq!(settlement.publisher_delta, policy.reuse_reward);
    assert_eq!(ledger.available_balance("node-a"), Some(11));
}

#[test]
fn compute_replay_evu_zero_roi_window() {
    let policy = RoiPolicy {
        evu_per_window: 1,
        roi_window_ms: 0,
        max_reward_per_replay: 10,
        balance_cap: 10_000,
    };
    let result = compute_replay_evu(400, &policy);
    assert_eq!(result, 0);
}

#[test]
fn journal_serde_roundtrip() {
    let mut journal = LedgerJournal::new("node-x");
    let policy = RoiPolicy::default();
    journal.record_replay_success("gene-1", 400, 1_000, &policy);
    journal.record_replay_success("gene-2", 600, 2_000, &policy);

    let json = serde_json::to_string(journal.entries()).unwrap();
    let restored_entries: Vec<LedgerEntry> = serde_json::from_str(&json).unwrap();
    let restored = journal_from_snapshot("node-x", restored_entries);

    assert_eq!(restored.node_id, journal.node_id);
    assert_eq!(restored.len(), journal.len());
    assert_eq!(restored.balance(), journal.balance());
    assert_eq!(restored.entries(), journal.entries());
}

#[test]
fn roi_stable_empty_journal() {
    let journal = LedgerJournal::new("empty-node");
    let result = journal.roi_stable(1.0, 0.05);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0.0);
}

#[test]
fn repeated_settle_remote_reuse_reputation_convergence() {
    let mut ledger = EvuLedger {
        accounts: vec![EvuAccount {
            node_id: "node-converge".into(),
            balance: 100,
        }],
        reputations: vec![ReputationRecord {
            node_id: "node-converge".into(),
            publish_success_rate: 0.5,
            validator_accuracy: 0.5,
            reuse_impact: 0,
        }],
    };
    let policy = StakePolicy::default();

    for _ in 0..10 {
        ledger.settle_remote_reuse("node-converge", true, &policy);
    }

    let reputation = ledger
        .reputations
        .iter()
        .find(|r| r.node_id == "node-converge")
        .unwrap();

    // With 70% weight on current and 30% on observation (1.0),
    // after 10 observations: 0.5 * 0.7^10 + 1.0 * (1 - 0.7^10)
    // 0.7^10 ≈ 0.0282, so: 0.5 * 0.0282 + 1.0 * 0.9718 ≈ 0.986
    // Reputation should converge toward 1.0
    assert!(reputation.publish_success_rate > 0.9);
}
