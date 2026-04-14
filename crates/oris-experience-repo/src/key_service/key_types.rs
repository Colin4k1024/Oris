//! Key types for API key management.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Unique identifier for an API key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyId(pub String);

impl KeyId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl Default for KeyId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for KeyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Status of an API key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyStatus {
    /// Key is active and can be used.
    Active,
    /// Key has been revoked and cannot be used.
    Revoked,
    /// Key has expired based on TTL.
    Expired,
}

impl std::fmt::Display for KeyStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyStatus::Active => write!(f, "Active"),
            KeyStatus::Revoked => write!(f, "Revoked"),
            KeyStatus::Expired => write!(f, "Expired"),
        }
    }
}

/// An API key with its metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    /// Unique key identifier.
    pub key_id: KeyId,
    /// SHA-256 hash of the API key (we never store the raw key).
    pub api_key_hash: String,
    /// Agent ID this key belongs to.
    pub agent_id: String,
    /// Human-readable description.
    pub description: Option<String>,
    /// Current status.
    pub status: KeyStatus,
    /// When the key was created.
    pub created_at: DateTime<Utc>,
    /// When the key expires (if TTL is set).
    pub expires_at: Option<DateTime<Utc>>,
    /// When the key was revoked (if applicable).
    pub revoked_at: Option<DateTime<Utc>>,
    /// When the key was last used.
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Public information about an API key (for listing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyInfo {
    /// Unique key identifier.
    pub key_id: KeyId,
    /// Agent ID this key belongs to.
    pub agent_id: String,
    /// Human-readable description.
    pub description: Option<String>,
    /// Current status.
    pub status: KeyStatus,
    /// When the key was created.
    pub created_at: DateTime<Utc>,
    /// When the key expires (if TTL is set).
    pub expires_at: Option<DateTime<Utc>>,
    /// When the key was last used.
    pub last_used_at: Option<DateTime<Utc>>,
}

impl From<&ApiKey> for ApiKeyInfo {
    fn from(key: &ApiKey) -> Self {
        Self {
            key_id: key.key_id.clone(),
            agent_id: key.agent_id.clone(),
            description: key.description.clone(),
            status: key.status,
            created_at: key.created_at,
            expires_at: key.expires_at,
            last_used_at: key.last_used_at,
        }
    }
}

/// Status of a public key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicKeyStatus {
    /// Public key is active and can be used for verification.
    Active,
    /// Public key has been revoked.
    Revoked,
}

impl std::fmt::Display for PublicKeyStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PublicKeyStatus::Active => write!(f, "Active"),
            PublicKeyStatus::Revoked => write!(f, "Revoked"),
        }
    }
}

/// A registered public key for Ed25519 signature verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicKey {
    /// Sender ID this public key belongs to.
    pub sender_id: String,
    /// 32-byte hex-encoded Ed25519 public key.
    pub public_key_hex: String,
    /// Version number for this public key (increments on rotation).
    pub version: i32,
    /// When the public key was registered.
    pub created_at: DateTime<Utc>,
    /// Current status.
    pub status: PublicKeyStatus,
}

impl PublicKey {
    /// Validate that a public key hex string is valid (64 hex chars = 32 bytes).
    pub fn validate_hex(hex: &str) -> bool {
        hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit())
    }
}
