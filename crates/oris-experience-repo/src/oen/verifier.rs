//! OEN Envelope verifier for validating signed envelopes.

use std::time::Duration;

use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::error::OenError;

/// OEN Envelope message types supported by Experience Repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    Publish,
    Fetch,
    Feedback,
}

impl std::fmt::Display for MessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageType::Publish => write!(f, "Publish"),
            MessageType::Fetch => write!(f, "Fetch"),
            MessageType::Feedback => write!(f, "Feedback"),
        }
    }
}

/// A simplified OEN Envelope for Experience Repository.
///
/// This is a simplified version of the full OEN Envelope, focused on
/// the Publish use case for external agents sharing experiences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OenEnvelope {
    /// Sender identifier (agent ID).
    pub sender_id: String,
    /// Message type.
    pub message_type: MessageType,
    /// The payload (Gene or Capsule data).
    pub payload: serde_json::Value,
    /// Ed25519 signature (base64 encoded).
    pub signature: String,
    /// Timestamp (RFC3339 format).
    pub timestamp: String,
}

impl OenEnvelope {
    /// Parse an envelope from JSON.
    pub fn from_json(json: &str) -> Result<Self, OenError> {
        serde_json::from_str(json).map_err(|e| OenError::ParseError(e.to_string()))
    }

    /// Convert to JSON string.
    pub fn to_json(&self) -> Result<String, OenError> {
        serde_json::to_string(self).map_err(|e| OenError::ParseError(e.to_string()))
    }
}

/// OEN Envelope verifier with caching support.
#[derive(Clone)]
pub struct OenVerifier {
    /// Maximum age of envelope timestamp (in seconds).
    timestamp_tolerance_secs: i64,
    /// Signature cache (sender_id -> (signature, verified_at)).
    /// This is a simple in-memory cache for demo purposes.
    signature_cache: std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<String, Instant>>>,
}

/// Cache entry with verification timestamp.
struct Instant {
    at: std::time::Instant,
}

impl Default for Instant {
    fn default() -> Self {
        Self {
            at: std::time::Instant::now(),
        }
    }
}

impl OenVerifier {
    /// Create a new verifier with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a verifier with custom timestamp tolerance.
    pub fn with_timestamp_tolerance(mut self, secs: i64) -> Self {
        self.timestamp_tolerance_secs = secs;
        self
    }

    /// Verify an OEN Envelope.
    ///
    /// This verifies:
    /// 1. The message type is Publish
    /// 2. The sender_id matches the expected agent
    /// 3. The timestamp is within the tolerance window
    /// 4. The Ed25519 signature is valid
    pub async fn verify_envelope(
        &self,
        envelope: &OenEnvelope,
        expected_agent_id: &str,
        public_key_hex: &str,
    ) -> Result<(), OenError> {
        // 1. Check message type
        if envelope.message_type != MessageType::Publish {
            return Err(OenError::InvalidMessageType {
                expected: MessageType::Publish.to_string(),
                actual: envelope.message_type.to_string(),
            });
        }

        // 2. Check sender_id matches
        if envelope.sender_id != expected_agent_id {
            return Err(OenError::SenderMismatch {
                expected: expected_agent_id.to_string(),
                actual: envelope.sender_id.clone(),
            });
        }

        // 3. Check timestamp
        let timestamp: DateTime<Utc> = envelope
            .timestamp
            .parse()
            .map_err(|_| OenError::ParseError("invalid timestamp format".to_string()))?;

        let now = Utc::now();
        let diff = (now - timestamp).num_seconds().abs();

        if diff > self.timestamp_tolerance_secs {
            return Err(OenError::TimestampExpired {
                seconds: diff,
                max: self.timestamp_tolerance_secs,
            });
        }

        // 4. Verify signature (with cache)
        let cache_key = format!("{}:{}", envelope.sender_id, envelope.signature);

        {
            let cache = self.signature_cache.lock().await;
            if let Some(instant) = cache.get(&cache_key) {
                // Cache hit - signature was verified within TTL
                if instant.at.elapsed() < Duration::from_secs(300) {
                    return Ok(());
                }
            }
        }

        // Verify the signature
        // For the simplified Envelope, we create a mock EvolutionEnvelope-like structure
        // and use the signing module's verify function
        let payload_bytes = serde_json::to_vec(&envelope.payload)
            .map_err(|e| OenError::ParseError(e.to_string()))?;

        // Use the sender_id as the public key identifier
        // In production, this would look up the public key from a key service
        let signature_bytes =
            base64_decode(&envelope.signature).map_err(|_| OenError::SignatureFailed)?;

        // Verify using ed25519_dalek
        use ed25519_dalek::{Signature, Verifier};

        let signature_bytes: [u8; 64] = signature_bytes
            .try_into()
            .map_err(|_| OenError::SignatureFailed)?;

        let signature = Signature::from_bytes(&signature_bytes);

        let public_key_bytes = hex::decode(public_key_hex)
            .map_err(|_| OenError::SigningError("invalid public key hex".to_string()))?;

        let public_key_bytes: [u8; 32] = public_key_bytes
            .try_into()
            .map_err(|_| OenError::SigningError("expected 32-byte public key".to_string()))?;

        let public_key = ed25519_dalek::VerifyingKey::from_bytes(&public_key_bytes)
            .map_err(|_| OenError::SigningError("invalid public key".to_string()))?;

        public_key
            .verify(&payload_bytes, &signature)
            .map_err(|_| OenError::SignatureFailed)?;

        // Cache the verified signature
        {
            let mut cache = self.signature_cache.lock().await;
            cache.insert(cache_key, Instant::default());
        }

        Ok(())
    }
}

impl Default for OenVerifier {
    fn default() -> Self {
        Self {
            timestamp_tolerance_secs: 300, // 5 minutes
            signature_cache: std::sync::Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
        }
    }
}

/// Decode base64 string to bytes.
fn base64_decode(input: &str) -> Result<Vec<u8>, ()> {
    // Use standard base64 decoding with the base64 crate
    base64::engine::general_purpose::STANDARD
        .decode(input)
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_type_serialization() {
        let json = r#""publish""#;
        let msg_type: MessageType = serde_json::from_str(json).unwrap();
        assert_eq!(msg_type, MessageType::Publish);

        let back_to_json = serde_json::to_string(&msg_type).unwrap();
        assert_eq!(back_to_json, "\"publish\"");
    }

    #[tokio::test]
    async fn test_envelope_parsing() {
        let json = r#"{
            "sender_id": "agent-123",
            "message_type": "publish",
            "payload": {"gene": {}},
            "signature": "abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz01234567",
            "timestamp": "2026-04-14T10:00:00Z"
        }"#;

        let envelope = OenEnvelope::from_json(json).unwrap();
        assert_eq!(envelope.sender_id, "agent-123");
        assert_eq!(envelope.message_type, MessageType::Publish);
    }
}
