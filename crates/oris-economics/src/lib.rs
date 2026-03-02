//! Non-financial EVU accounting for local publish and validation incentives.

use serde::{Deserialize, Serialize};

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
}

impl Default for StakePolicy {
    fn default() -> Self {
        Self { publish_cost: 1 }
    }
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
        let policy = StakePolicy { publish_cost: 5 };
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
        let policy = StakePolicy { publish_cost: 5 };
        assert!(!ledger.can_publish("node1", &policy));
    }

    #[test]
    fn test_evu_ledger_cannot_publish_unknown_node() {
        let ledger = EvuLedger::default();
        let policy = StakePolicy { publish_cost: 5 };
        assert!(!ledger.can_publish("unknown_node", &policy));
    }

    #[test]
    fn test_default_stake_policy() {
        let policy = StakePolicy::default();
        assert_eq!(policy.publish_cost, 1);
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
}
