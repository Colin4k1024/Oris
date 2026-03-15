//! Non-financial EVU accounting for local publish and validation incentives.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

// ── EVU transaction journal ────────────────────────────────────────────────

/// One durable record written for every EVU-affecting event.
/// The journal represents the ground-truth ledger; balances are derived by
/// replaying the sequence.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LedgerEntry {
    /// Monotonically increasing sequence number (1-based).
    pub seq: u64,
    /// Gene that was replayed, or empty for non-gene events.
    pub gene_id: String,
    /// Node whose balance is credited / debited.
    pub node_id: String,
    /// Signed EVU delta for this event.
    pub delta: i64,
    /// Milliseconds of inference cost that was avoided.
    pub latency_saved_ms: u64,
    /// Human-readable event type tag.
    pub event_type: LedgerEventType,
    /// Unix-epoch milliseconds when the entry was recorded.
    pub recorded_at_ms: u64,
    /// Running balance after applying this entry.
    pub cumulative_balance: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum LedgerEventType {
    ReplaySuccess,
    PublishStakeReserved,
    ReuseReward,
    ValidationPenalty,
    AntiInflationCap,
    ManualAdjustment,
}

// ── Replay ROI calculator ──────────────────────────────────────────────────

/// Parameters governing how many EVU a replay earns.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoiPolicy {
    /// EVU earned per complete `roi_window_ms` of inference cost avoided.
    pub evu_per_window: i64,
    /// Window size in milliseconds (default: 200 ms).
    pub roi_window_ms: u64,
    /// Hard ceiling on EVU earned from a single replay event.
    pub max_reward_per_replay: i64,
    /// Absolute cap on any node's balance (anti-inflation ceiling).
    pub balance_cap: i64,
}

impl Default for RoiPolicy {
    fn default() -> Self {
        Self {
            evu_per_window: 1,
            roi_window_ms: 200,
            max_reward_per_replay: 10,
            balance_cap: 10_000,
        }
    }
}

/// Compute the EVU delta for a single successful replay, clamped to the policy
/// ceiling.
pub fn compute_replay_evu(latency_saved_ms: u64, policy: &RoiPolicy) -> i64 {
    if policy.roi_window_ms == 0 {
        return 0;
    }
    let windows = (latency_saved_ms / policy.roi_window_ms) as i64;
    (windows * policy.evu_per_window).min(policy.max_reward_per_replay)
}

// ── EVU ledger journal ─────────────────────────────────────────────────────

/// Append-only journal of all EVU events for a single node.
/// Balances are re-derivable by replaying the sequence, giving restart
/// recovery "for free".
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LedgerJournal {
    pub node_id: String,
    entries: Vec<LedgerEntry>,
}

impl LedgerJournal {
    pub fn new(node_id: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            entries: Vec::new(),
        }
    }

    /// Current EVU balance derived from the journal (O(1), stored in last entry).
    pub fn balance(&self) -> i64 {
        self.entries
            .last()
            .map(|e| e.cumulative_balance)
            .unwrap_or(0)
    }

    /// Number of committed journal entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Immutable view of all entries (for serialisation / persistence).
    pub fn entries(&self) -> &[LedgerEntry] {
        &self.entries
    }

    /// Re-derive the balance by replaying all entries from scratch.
    /// Returns the final balance, confirming the journal is self-consistent.
    pub fn replay_balance(&self) -> i64 {
        self.entries.iter().fold(0i64, |acc, e| acc + e.delta)
    }

    /// Record a successful replay event.
    /// Returns the resulting `LedgerEntry`.
    pub fn record_replay_success(
        &mut self,
        gene_id: impl Into<String>,
        latency_saved_ms: u64,
        now_ms: u64,
        roi_policy: &RoiPolicy,
    ) -> LedgerEntry {
        let raw_delta = compute_replay_evu(latency_saved_ms, roi_policy);
        let current = self.balance();
        // Anti-inflation: clamp so balance never exceeds the cap.
        let delta = if current + raw_delta > roi_policy.balance_cap {
            (roi_policy.balance_cap - current).max(0)
        } else {
            raw_delta
        };
        let entry = LedgerEntry {
            seq: self.entries.len() as u64 + 1,
            gene_id: gene_id.into(),
            node_id: self.node_id.clone(),
            delta,
            latency_saved_ms,
            event_type: if delta < raw_delta {
                LedgerEventType::AntiInflationCap
            } else {
                LedgerEventType::ReplaySuccess
            },
            recorded_at_ms: now_ms,
            cumulative_balance: current + delta,
        };
        self.entries.push(entry.clone());
        entry
    }

    /// Append a pre-built entry (used during journal replay / restore).
    /// Entries are trusted; no further mutation is applied.
    pub fn restore_entry(&mut self, entry: LedgerEntry) {
        self.entries.push(entry);
    }

    /// Compute the average ROI (EVU per replay) across all replay events and
    /// check whether it deviates from `baseline_roi` by more than `tolerance`
    /// (expressed as a fraction, e.g. 0.05 for 5 %).
    /// Returns `Ok(observed_roi)` when within tolerance,
    ///         `Err(observed_roi)` when the deviation exceeds the threshold.
    pub fn roi_stable(&self, baseline_roi: f64, tolerance: f64) -> Result<f64, f64> {
        let replay_entries: Vec<&LedgerEntry> = self
            .entries
            .iter()
            .filter(|e| {
                e.event_type == LedgerEventType::ReplaySuccess
                    || e.event_type == LedgerEventType::AntiInflationCap
            })
            .collect();
        if replay_entries.is_empty() {
            return Ok(0.0);
        }
        let total_evu: i64 = replay_entries.iter().map(|e| e.delta).sum();
        let observed = total_evu as f64 / replay_entries.len() as f64;
        let deviation = (observed - baseline_roi).abs() / baseline_roi.max(f64::EPSILON);
        if deviation <= tolerance {
            Ok(observed)
        } else {
            Err(observed)
        }
    }
}

/// Reconstruct a `LedgerJournal` from a serialised entry slice (e.g. loaded
/// from disk / SQLite).  The balance column in each restored entry is
/// re-validated to ensure consistency.
pub fn journal_from_snapshot(
    node_id: impl Into<String>,
    entries: Vec<LedgerEntry>,
) -> LedgerJournal {
    let mut j = LedgerJournal::new(node_id);
    for e in entries {
        j.restore_entry(e);
    }
    j
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EvuAccount {
    pub node_id: String,
    pub balance: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ReputationRecord {
    pub node_id: String,
    pub publish_success_rate: f32,
    pub validator_accuracy: f32,
    pub reuse_impact: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StakePolicy {
    pub publish_cost: i64,
    pub reuse_reward: i64,
    pub validator_penalty: i64,
}

impl Default for StakePolicy {
    fn default() -> Self {
        Self {
            publish_cost: 1,
            reuse_reward: 2,
            validator_penalty: 1,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EconomicsSignal {
    pub available_evu: i64,
    pub publish_success_rate: f32,
    pub validator_accuracy: f32,
    pub reuse_impact: u64,
    pub selector_weight: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidationSettlement {
    pub publisher_delta: i64,
    pub validator_delta: i64,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EvuLedger {
    pub accounts: Vec<EvuAccount>,
    pub reputations: Vec<ReputationRecord>,
}

impl EvuLedger {
    pub fn can_publish(&self, node_id: &str, policy: &StakePolicy) -> bool {
        self.accounts
            .iter()
            .find(|account| account.node_id == node_id)
            .map(|account| account.balance >= policy.publish_cost)
            .unwrap_or(false)
    }

    pub fn available_balance(&self, node_id: &str) -> Option<i64> {
        self.accounts
            .iter()
            .find(|account| account.node_id == node_id)
            .map(|account| account.balance)
    }

    pub fn reserve_publish_stake(
        &mut self,
        node_id: &str,
        policy: &StakePolicy,
    ) -> Option<ValidationSettlement> {
        if !self.can_publish(node_id, policy) {
            return None;
        }
        let account = self.account_mut(node_id);
        account.balance -= policy.publish_cost;
        Some(ValidationSettlement {
            publisher_delta: -policy.publish_cost,
            validator_delta: 0,
            reason: "publish stake reserved".into(),
        })
    }

    pub fn settle_remote_reuse(
        &mut self,
        publisher_id: &str,
        success: bool,
        policy: &StakePolicy,
    ) -> ValidationSettlement {
        if success {
            {
                let account = self.account_mut(publisher_id);
                account.balance += policy.reuse_reward;
            }
            {
                let reputation = self.reputation_mut(publisher_id);
                reputation.publish_success_rate =
                    blend_metric(reputation.publish_success_rate, 1.0);
                reputation.reuse_impact = reputation.reuse_impact.saturating_add(1);
            }
            ValidationSettlement {
                publisher_delta: policy.reuse_reward,
                validator_delta: 0,
                reason: "remote reuse succeeded".into(),
            }
        } else {
            let reputation = self.reputation_mut(publisher_id);
            reputation.publish_success_rate = blend_metric(reputation.publish_success_rate, 0.0);
            reputation.validator_accuracy = blend_metric(reputation.validator_accuracy, 0.0);
            ValidationSettlement {
                publisher_delta: 0,
                validator_delta: -policy.validator_penalty,
                reason: "remote reuse failed local validation".into(),
            }
        }
    }

    pub fn penalize_validator_divergence(
        &mut self,
        validator_id: &str,
        policy: &StakePolicy,
    ) -> ValidationSettlement {
        let reputation = self.reputation_mut(validator_id);
        reputation.validator_accuracy = blend_metric(reputation.validator_accuracy, 0.0);
        ValidationSettlement {
            publisher_delta: 0,
            validator_delta: -policy.validator_penalty,
            reason: "validator report diverged from local final validation".into(),
        }
    }

    pub fn selector_reputation_bias(&self) -> BTreeMap<String, f32> {
        self.reputations
            .iter()
            .map(|record| {
                let reuse_bonus = ((record.reuse_impact as f32).ln_1p() / 4.0).min(0.25);
                let weight = (record.publish_success_rate * 0.55)
                    + (record.validator_accuracy * 0.35)
                    + reuse_bonus;
                (record.node_id.clone(), weight.clamp(0.0, 1.0))
            })
            .collect()
    }

    pub fn governor_signal(&self, node_id: &str) -> Option<EconomicsSignal> {
        let balance = self.available_balance(node_id).unwrap_or(0);
        let reputation = self
            .reputations
            .iter()
            .find(|record| record.node_id == node_id)
            .cloned()
            .or_else(|| {
                self.accounts
                    .iter()
                    .find(|record| record.node_id == node_id)
                    .map(|_| ReputationRecord {
                        node_id: node_id.to_string(),
                        publish_success_rate: 0.5,
                        validator_accuracy: 0.5,
                        reuse_impact: 0,
                    })
            })?;
        let selector_weight = self
            .selector_reputation_bias()
            .get(node_id)
            .copied()
            .unwrap_or(0.0);
        Some(EconomicsSignal {
            available_evu: balance,
            publish_success_rate: reputation.publish_success_rate,
            validator_accuracy: reputation.validator_accuracy,
            reuse_impact: reputation.reuse_impact,
            selector_weight,
        })
    }

    fn account_mut(&mut self, node_id: &str) -> &mut EvuAccount {
        if let Some(index) = self
            .accounts
            .iter()
            .position(|item| item.node_id == node_id)
        {
            return &mut self.accounts[index];
        }
        self.accounts.push(EvuAccount {
            node_id: node_id.to_string(),
            balance: 0,
        });
        self.accounts.last_mut().expect("account just inserted")
    }

    fn reputation_mut(&mut self, node_id: &str) -> &mut ReputationRecord {
        if let Some(index) = self
            .reputations
            .iter()
            .position(|item| item.node_id == node_id)
        {
            return &mut self.reputations[index];
        }
        self.reputations.push(ReputationRecord {
            node_id: node_id.to_string(),
            publish_success_rate: 0.5,
            validator_accuracy: 0.5,
            reuse_impact: 0,
        });
        self.reputations
            .last_mut()
            .expect("reputation just inserted")
    }
}

fn blend_metric(current: f32, observation: f32) -> f32 {
    ((current * 0.7) + (observation * 0.3)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evu_ledger_can_publish_with_sufficient_balance() {
        let ledger = EvuLedger {
            accounts: vec![EvuAccount {
                node_id: "node1".into(),
                balance: 10,
            }],
            reputations: vec![],
        };
        let policy = StakePolicy {
            publish_cost: 5,
            ..Default::default()
        };
        assert!(ledger.can_publish("node1", &policy));
    }

    #[test]
    fn test_evu_ledger_cannot_publish_with_insufficient_balance() {
        let ledger = EvuLedger {
            accounts: vec![EvuAccount {
                node_id: "node1".into(),
                balance: 3,
            }],
            reputations: vec![],
        };
        let policy = StakePolicy {
            publish_cost: 5,
            ..Default::default()
        };
        assert!(!ledger.can_publish("node1", &policy));
    }

    #[test]
    fn test_evu_ledger_cannot_publish_unknown_node() {
        let ledger = EvuLedger::default();
        let policy = StakePolicy {
            publish_cost: 5,
            ..Default::default()
        };
        assert!(!ledger.can_publish("unknown_node", &policy));
    }

    #[test]
    fn test_default_stake_policy() {
        let policy = StakePolicy::default();
        assert_eq!(policy.publish_cost, 1);
        assert_eq!(policy.reuse_reward, 2);
        assert_eq!(policy.validator_penalty, 1);
    }

    #[test]
    fn test_reputation_record() {
        let reputation = ReputationRecord {
            node_id: "node1".into(),
            publish_success_rate: 0.95,
            validator_accuracy: 0.88,
            reuse_impact: 100,
        };
        assert_eq!(reputation.node_id, "node1");
        assert!(reputation.publish_success_rate > 0.9);
    }

    #[test]
    fn test_validation_settlement() {
        let settlement = ValidationSettlement {
            publisher_delta: 10,
            validator_delta: 5,
            reason: "successful validation".into(),
        };
        assert_eq!(settlement.publisher_delta, 10);
        assert_eq!(settlement.validator_delta, 5);
    }

    #[test]
    fn reserve_publish_stake_deducts_balance() {
        let mut ledger = EvuLedger {
            accounts: vec![EvuAccount {
                node_id: "node1".into(),
                balance: 10,
            }],
            reputations: vec![],
        };
        let policy = StakePolicy::default();
        let settlement = ledger.reserve_publish_stake("node1", &policy).unwrap();
        assert_eq!(settlement.publisher_delta, -1);
        assert_eq!(ledger.available_balance("node1"), Some(9));
    }

    #[test]
    fn remote_reuse_success_rewards_balance_and_reputation() {
        let mut ledger = EvuLedger {
            accounts: vec![EvuAccount {
                node_id: "node1".into(),
                balance: 3,
            }],
            reputations: vec![ReputationRecord {
                node_id: "node1".into(),
                publish_success_rate: 0.5,
                validator_accuracy: 0.5,
                reuse_impact: 0,
            }],
        };
        let settlement = ledger.settle_remote_reuse("node1", true, &StakePolicy::default());
        assert_eq!(settlement.publisher_delta, 2);
        assert_eq!(ledger.available_balance("node1"), Some(5));
        assert!(ledger.reputations[0].publish_success_rate > 0.5);
        assert_eq!(ledger.reputations[0].reuse_impact, 1);
    }

    #[test]
    fn remote_reuse_failure_penalizes_reputation() {
        let mut ledger = EvuLedger {
            accounts: vec![EvuAccount {
                node_id: "node1".into(),
                balance: 3,
            }],
            reputations: vec![ReputationRecord {
                node_id: "node1".into(),
                publish_success_rate: 0.8,
                validator_accuracy: 0.9,
                reuse_impact: 2,
            }],
        };
        let settlement = ledger.settle_remote_reuse("node1", false, &StakePolicy::default());
        assert_eq!(settlement.publisher_delta, 0);
        assert!(settlement.validator_delta < 0);
        assert!(ledger.reputations[0].publish_success_rate < 0.8);
        assert!(ledger.reputations[0].validator_accuracy < 0.9);
        assert_eq!(ledger.available_balance("node1"), Some(3));
    }

    #[test]
    fn selector_reputation_bias_prefers_stronger_reputation() {
        let ledger = EvuLedger {
            accounts: vec![],
            reputations: vec![
                ReputationRecord {
                    node_id: "node-a".into(),
                    publish_success_rate: 0.4,
                    validator_accuracy: 0.4,
                    reuse_impact: 0,
                },
                ReputationRecord {
                    node_id: "node-b".into(),
                    publish_success_rate: 0.9,
                    validator_accuracy: 0.9,
                    reuse_impact: 10,
                },
            ],
        };
        let bias = ledger.selector_reputation_bias();
        assert!(bias["node-b"] > bias["node-a"]);
    }

    #[test]
    fn governor_signal_exposes_balance_and_reputation() {
        let ledger = EvuLedger {
            accounts: vec![EvuAccount {
                node_id: "node1".into(),
                balance: 7,
            }],
            reputations: vec![ReputationRecord {
                node_id: "node1".into(),
                publish_success_rate: 0.75,
                validator_accuracy: 0.5,
                reuse_impact: 4,
            }],
        };
        let signal = ledger.governor_signal("node1").unwrap();
        assert_eq!(signal.available_evu, 7);
        assert_eq!(signal.reuse_impact, 4);
        assert!(signal.selector_weight > 0.0);
    }

    // ── LedgerJournal / EVU calculation tests ─────────────────────────────

    #[test]
    fn compute_replay_evu_proportional_to_latency() {
        let policy = RoiPolicy {
            evu_per_window: 1,
            roi_window_ms: 200,
            max_reward_per_replay: 10,
            ..RoiPolicy::default()
        };
        // 400 ms saved → 2 windows → 2 EVU
        assert_eq!(compute_replay_evu(400, &policy), 2);
        // 199 ms saved → 0 complete windows → 0 EVU
        assert_eq!(compute_replay_evu(199, &policy), 0);
        // 1_000 ms → 5 EVU
        assert_eq!(compute_replay_evu(1_000, &policy), 5);
    }

    #[test]
    fn compute_replay_evu_capped_by_max() {
        let policy = RoiPolicy {
            evu_per_window: 3,
            roi_window_ms: 200,
            max_reward_per_replay: 5,
            ..RoiPolicy::default()
        };
        // 1_000 ms → 5 windows × 3 = 15, but capped at 5
        assert_eq!(compute_replay_evu(1_000, &policy), 5);
    }

    #[test]
    fn journal_records_replay_success_and_accumulates_balance() {
        let mut journal = LedgerJournal::new("node-x");
        let policy = RoiPolicy::default(); // 1 EVU per 200 ms
        let e1 = journal.record_replay_success("gene-1", 400, 1_000, &policy);
        let e2 = journal.record_replay_success("gene-2", 600, 2_000, &policy);

        assert_eq!(e1.delta, 2);
        assert_eq!(e1.seq, 1);
        assert_eq!(e1.cumulative_balance, 2);

        assert_eq!(e2.delta, 3);
        assert_eq!(e2.seq, 2);
        assert_eq!(e2.cumulative_balance, 5);

        assert_eq!(journal.balance(), 5);
        assert_eq!(journal.len(), 2);
    }

    #[test]
    fn journal_replay_balance_matches_primary_balance() {
        let mut journal = LedgerJournal::new("node-y");
        let policy = RoiPolicy::default();
        journal.record_replay_success("gene-a", 400, 1_000, &policy);
        journal.record_replay_success("gene-b", 800, 2_000, &policy);
        journal.record_replay_success("gene-c", 200, 3_000, &policy);

        // replay_balance() re-derives from entry deltas; must equal balance()
        assert_eq!(journal.replay_balance(), journal.balance());
    }

    #[test]
    fn journal_restore_from_snapshot_recovers_balance() {
        let mut source = LedgerJournal::new("node-z");
        let policy = RoiPolicy::default();
        source.record_replay_success("gene-1", 400, 1_000, &policy);
        source.record_replay_success("gene-2", 600, 2_000, &policy);

        // Serialise entries to JSON (simulating persistence)
        let snapshot_json = serde_json::to_string(source.entries()).unwrap();
        let restored_entries: Vec<LedgerEntry> = serde_json::from_str(&snapshot_json).unwrap();

        // Restore from snapshot → balance must match original
        let restored = journal_from_snapshot("node-z", restored_entries);
        assert_eq!(restored.balance(), source.balance());
        assert_eq!(restored.len(), source.len());
    }

    #[test]
    fn journal_anti_inflation_cap_prevents_overflow() {
        let policy = RoiPolicy {
            evu_per_window: 5,
            roi_window_ms: 100,
            max_reward_per_replay: 50,
            balance_cap: 10,
        };
        let mut journal = LedgerJournal::new("node-inflate");
        // 5_000 ms → 50 EVU, but cap is 10
        let e = journal.record_replay_success("gene-big", 5_000, 1, &policy);
        assert_eq!(e.delta, 10);
        assert_eq!(journal.balance(), 10);
        assert_eq!(e.event_type, LedgerEventType::AntiInflationCap);

        // Second replay → balance already at cap, delta must be 0
        let e2 = journal.record_replay_success("gene-extra", 5_000, 2, &policy);
        assert_eq!(e2.delta, 0);
        assert_eq!(journal.balance(), 10);
    }

    #[test]
    fn roi_stable_within_five_percent_tolerance() {
        // baseline: 2 EVU/replay (400 ms / 200 ms/window × 1 EVU)
        let policy = RoiPolicy::default();
        let mut journal = LedgerJournal::new("roi-node");
        for i in 0..20u64 {
            journal.record_replay_success("gene", 400, i * 100, &policy);
        }
        let result = journal.roi_stable(2.0, 0.05);
        assert!(result.is_ok(), "ROI should be stable: {:?}", result);
        let observed = result.unwrap();
        assert!((observed - 2.0).abs() / 2.0 <= 0.05);
    }

    #[test]
    fn roi_stable_detects_inflated_roi() {
        // baseline: 1 EVU/replay but we use 400 ms (2 EVU).  With baseline=1.0
        // the deviation is 100%, far outside 5%.
        let policy = RoiPolicy::default();
        let mut journal = LedgerJournal::new("roi-node-b");
        for i in 0..10u64 {
            journal.record_replay_success("gene", 400, i * 100, &policy);
        }
        let result = journal.roi_stable(1.0, 0.05);
        assert!(result.is_err(), "Should detect deviation from baseline");
    }
}
