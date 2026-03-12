//! Protocol contracts for the Oris Evolution Network (OEN).

pub mod gossip;

use std::collections::BTreeSet;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use oris_evolution::{Capsule, EvolutionEvent, Gene};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MessageType {
    Publish,
    Fetch,
    Report,
    Revoke,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NetworkAsset {
    Gene { gene: Gene },
    Capsule { capsule: Capsule },
    EvolutionEvent { event: EvolutionEvent },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvolutionEnvelope {
    pub protocol: String,
    pub protocol_version: String,
    pub message_type: MessageType,
    pub message_id: String,
    pub sender_id: String,
    pub timestamp: String,
    pub assets: Vec<NetworkAsset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<EnvelopeManifest>,
    pub content_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvelopeManifest {
    pub publisher: String,
    pub sender_id: String,
    pub asset_ids: Vec<String>,
    pub asset_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublishRequest {
    pub sender_id: String,
    pub assets: Vec<NetworkAsset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_token: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FetchQuery {
    pub sender_id: String,
    pub signals: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_token: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncAudit {
    pub batch_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_cursor: Option<String>,
    pub scanned_count: usize,
    pub applied_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failure_reasons: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FetchResponse {
    pub sender_id: String,
    pub assets: Vec<NetworkAsset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_token: Option<String>,
    #[serde(default)]
    pub sync_audit: SyncAudit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RevokeNotice {
    pub sender_id: String,
    pub asset_ids: Vec<String>,
    pub reason: String,
}

impl EvolutionEnvelope {
    pub fn publish(sender_id: impl Into<String>, assets: Vec<NetworkAsset>) -> Self {
        let sender_id = sender_id.into();
        let manifest = Some(Self::build_manifest(&sender_id, &assets));
        let mut envelope = Self {
            protocol: "oen".into(),
            protocol_version: "0.1".into(),
            message_type: MessageType::Publish,
            message_id: format!(
                "msg-{:x}",
                Utc::now().timestamp_nanos_opt().unwrap_or_default()
            ),
            sender_id,
            timestamp: Utc::now().to_rfc3339(),
            assets,
            manifest,
            content_hash: String::new(),
        };
        envelope.content_hash = envelope.compute_content_hash();
        envelope
    }

    fn build_manifest(sender_id: &str, assets: &[NetworkAsset]) -> EnvelopeManifest {
        EnvelopeManifest {
            publisher: sender_id.to_string(),
            sender_id: sender_id.to_string(),
            asset_ids: Self::manifest_asset_ids(assets),
            asset_hash: Self::compute_assets_hash(assets),
        }
    }

    fn normalize_manifest_ids(asset_ids: &[String]) -> Vec<String> {
        let normalized = asset_ids
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect::<BTreeSet<_>>();
        normalized.into_iter().collect()
    }

    pub fn manifest_asset_ids(assets: &[NetworkAsset]) -> Vec<String> {
        let ids = assets
            .iter()
            .map(Self::manifest_asset_id)
            .collect::<BTreeSet<_>>();
        ids.into_iter().collect()
    }

    fn manifest_asset_id(asset: &NetworkAsset) -> String {
        match asset {
            NetworkAsset::Gene { gene } => format!("gene:{}", gene.id),
            NetworkAsset::Capsule { capsule } => format!("capsule:{}", capsule.id),
            NetworkAsset::EvolutionEvent { event } => {
                let payload = serde_json::to_vec(event).unwrap_or_default();
                let mut hasher = Sha256::new();
                hasher.update(payload);
                format!("event:{}", hex::encode(hasher.finalize()))
            }
        }
    }

    pub fn compute_assets_hash(assets: &[NetworkAsset]) -> String {
        let payload = serde_json::to_vec(assets).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(payload);
        hex::encode(hasher.finalize())
    }

    pub fn compute_content_hash(&self) -> String {
        let payload = (
            &self.protocol,
            &self.protocol_version,
            &self.message_type,
            &self.message_id,
            &self.sender_id,
            &self.timestamp,
            &self.assets,
            &self.manifest,
        );
        let json = serde_json::to_vec(&payload).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(json);
        hex::encode(hasher.finalize())
    }

    pub fn verify_content_hash(&self) -> bool {
        self.compute_content_hash() == self.content_hash
    }

    pub fn verify_manifest(&self) -> Result<(), String> {
        let Some(manifest) = self.manifest.as_ref() else {
            return Err("missing manifest".into());
        };
        if manifest.publisher.trim().is_empty() {
            return Err("missing manifest publisher".into());
        }
        if manifest.sender_id.trim().is_empty() {
            return Err("missing manifest sender_id".into());
        }
        if manifest.sender_id.trim() != self.sender_id.trim() {
            return Err("manifest sender_id mismatch".into());
        }

        let expected_asset_ids = Self::manifest_asset_ids(&self.assets);
        let actual_asset_ids = Self::normalize_manifest_ids(&manifest.asset_ids);
        if expected_asset_ids != actual_asset_ids {
            return Err("manifest asset_ids mismatch".into());
        }

        let expected_hash = Self::compute_assets_hash(&self.assets);
        if manifest.asset_hash != expected_hash {
            return Err("manifest asset_hash mismatch".into());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oris_evolution::{AssetState, Gene};

    fn sample_gene(id: &str) -> Gene {
        Gene {
            id: id.to_string(),
            signals: vec!["docs.fix".to_string()],
            strategy: vec!["summary=docs fix".to_string()],
            validation: vec!["cargo test".to_string()],
            state: AssetState::Promoted,
        }
    }

    #[test]
    fn publish_populates_manifest_and_verifies() {
        let envelope = EvolutionEnvelope::publish(
            "node-a",
            vec![NetworkAsset::Gene {
                gene: sample_gene("gene-a"),
            }],
        );
        assert!(envelope.verify_content_hash());
        assert!(envelope.verify_manifest().is_ok());
        assert!(envelope.manifest.is_some());
    }

    #[test]
    fn verify_manifest_detects_sender_mismatch() {
        let mut envelope = EvolutionEnvelope::publish(
            "node-a",
            vec![NetworkAsset::Gene {
                gene: sample_gene("gene-a"),
            }],
        );
        envelope.sender_id = "node-b".to_string();
        assert!(envelope.verify_manifest().is_err());
    }

    #[test]
    fn verify_manifest_detects_asset_hash_drift() {
        let mut envelope = EvolutionEnvelope::publish(
            "node-a",
            vec![NetworkAsset::Gene {
                gene: sample_gene("gene-a"),
            }],
        );
        if let Some(NetworkAsset::Gene { gene }) = envelope.assets.first_mut() {
            gene.id = "gene-b".to_string();
        }
        assert!(envelope.verify_manifest().is_err());
    }
}
