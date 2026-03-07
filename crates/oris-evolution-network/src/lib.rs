//! Protocol contracts for the Oris Evolution Network (OEN).

pub mod gossip;

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
    pub content_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublishRequest {
    pub sender_id: String,
    pub assets: Vec<NetworkAsset>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FetchQuery {
    pub sender_id: String,
    pub signals: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FetchResponse {
    pub sender_id: String,
    pub assets: Vec<NetworkAsset>,
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
            content_hash: String::new(),
        };
        envelope.content_hash = envelope.compute_content_hash();
        envelope
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
        );
        let json = serde_json::to_vec(&payload).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(json);
        hex::encode(hasher.finalize())
    }

    pub fn verify_content_hash(&self) -> bool {
        self.compute_content_hash() == self.content_hash
    }
}
