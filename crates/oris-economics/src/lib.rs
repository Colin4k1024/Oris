//! Non-financial EVU accounting for local publish and validation incentives.

use std::collections::BTreeMap;

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
}
