//! SQLite-based Key Store implementation.

use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection};

use super::error::KeyServiceError;
use super::key_types::{ApiKey, ApiKeyInfo, KeyId, KeyStatus, PublicKey, PublicKeyStatus};
use super::KeyServiceError::{Expired, Revoked};

/// SHA-256 hash of an API key.
fn hash_api_key(api_key: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// SQLite-based Key Store.
pub struct KeyStore {
    conn: Connection,
}

impl KeyStore {
    /// Open a key store at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, KeyServiceError> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    /// Create an in-memory key store (for testing).
    pub fn memory() -> Result<Self, KeyServiceError> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    /// Initialize the database schema.
    fn init_schema(&self) -> Result<(), KeyServiceError> {
        self.conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS api_keys (
                key_id TEXT PRIMARY KEY,
                api_key_hash TEXT NOT NULL UNIQUE,
                agent_id TEXT NOT NULL,
                description TEXT,
                status TEXT NOT NULL DEFAULT 'Active',
                created_at TEXT NOT NULL,
                expires_at TEXT,
                revoked_at TEXT,
                last_used_at TEXT
            )
            "#,
            [],
        )?;

        self.conn.execute(
            r#"
            CREATE INDEX IF NOT EXISTS idx_api_keys_agent_id ON api_keys(agent_id)
            "#,
            [],
        )?;

        self.conn.execute(
            r#"
            CREATE INDEX IF NOT EXISTS idx_api_keys_status ON api_keys(status)
            "#,
            [],
        )?;

        // Initialize public_keys table for PKI with version support
        self.conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS public_keys (
                sender_id TEXT NOT NULL,
                version INTEGER NOT NULL DEFAULT 1,
                public_key_hex TEXT NOT NULL,
                created_at TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'Active',
                PRIMARY KEY (sender_id, version)
            )
            "#,
            [],
        )?;

        // Index for querying latest public key per sender
        self.conn.execute(
            r#"
            CREATE INDEX IF NOT EXISTS idx_public_keys_sender_version
            ON public_keys(sender_id, version DESC)
            "#,
            [],
        )?;

        Ok(())
    }

    /// Create a new API key.
    ///
    /// Returns the raw API key (only time it's visible) and the key info.
    pub fn create_key(
        &self,
        agent_id: impl Into<String>,
        description: Option<String>,
        ttl_days: Option<i64>,
    ) -> Result<(String, ApiKeyInfo), KeyServiceError> {
        let agent_id = agent_id.into();
        let key_id = KeyId::new();
        let raw_key = format!(
            "sk_live_{}",
            uuid::Uuid::new_v4().to_string().replace("-", "")
        );
        let api_key_hash = hash_api_key(&raw_key);
        let now = Utc::now();
        let expires_at = ttl_days.map(|days| now + Duration::days(days));

        let status = KeyStatus::Active;

        self.conn.execute(
            r#"
            INSERT INTO api_keys (key_id, api_key_hash, agent_id, description, status, created_at, expires_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                key_id.0,
                api_key_hash,
                agent_id,
                description,
                status.to_string(),
                now.to_rfc3339(),
                expires_at.map(|dt| dt.to_rfc3339()),
            ],
        )?;

        let api_key = ApiKey {
            key_id: key_id.clone(),
            api_key_hash,
            agent_id,
            description,
            status,
            created_at: now,
            expires_at,
            revoked_at: None,
            last_used_at: None,
        };

        Ok((raw_key, ApiKeyInfo::from(&api_key)))
    }

    /// Verify an API key and return the associated key info.
    pub fn verify_key(&self, api_key: &str) -> Result<ApiKeyInfo, KeyServiceError> {
        let api_key_hash = hash_api_key(api_key);

        let key: ApiKey = self
            .conn
            .query_row(
                r#"
            SELECT key_id, api_key_hash, agent_id, description, status,
                   created_at, expires_at, revoked_at, last_used_at
            FROM api_keys
            WHERE api_key_hash = ?1
            "#,
                params![api_key_hash],
                |row| {
                    let status_str: String = row.get(4)?;
                    let status = match status_str.as_str() {
                        "Active" => KeyStatus::Active,
                        "Revoked" => KeyStatus::Revoked,
                        "Expired" => KeyStatus::Expired,
                        _ => KeyStatus::Active,
                    };

                    let created_at: String = row.get(5)?;
                    let expires_at: Option<String> = row.get(6)?;
                    let revoked_at: Option<String> = row.get(7)?;
                    let last_used_at: Option<String> = row.get(8)?;

                    Ok(ApiKey {
                        key_id: KeyId(row.get(0)?),
                        api_key_hash: row.get(1)?,
                        agent_id: row.get(2)?,
                        description: row.get(3)?,
                        status,
                        created_at: DateTime::parse_from_rfc3339(&created_at)
                            .unwrap_or_default()
                            .with_timezone(&Utc),
                        expires_at: expires_at.and_then(|s| {
                            DateTime::parse_from_rfc3339(&s)
                                .ok()
                                .map(|dt| dt.with_timezone(&Utc))
                        }),
                        revoked_at: revoked_at.and_then(|s| {
                            DateTime::parse_from_rfc3339(&s)
                                .ok()
                                .map(|dt| dt.with_timezone(&Utc))
                        }),
                        last_used_at: last_used_at.and_then(|s| {
                            DateTime::parse_from_rfc3339(&s)
                                .ok()
                                .map(|dt| dt.with_timezone(&Utc))
                        }),
                    })
                },
            )
            .map_err(|_| KeyServiceError::InvalidKey)?;

        // Check status
        match key.status {
            KeyStatus::Revoked => return Err(Revoked),
            KeyStatus::Expired => return Err(Expired),
            KeyStatus::Active => {}
        }

        // Check expiration
        if let Some(expires_at) = key.expires_at {
            if Utc::now() > expires_at {
                return Err(Expired);
            }
        }

        // Update last_used_at
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE api_keys SET last_used_at = ?1 WHERE key_id = ?2",
            params![now, key.key_id.0],
        )?;

        Ok(ApiKeyInfo::from(&key))
    }

    /// List all API keys (without the actual key values).
    pub fn list_keys(&self) -> Result<Vec<ApiKeyInfo>, KeyServiceError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT key_id, api_key_hash, agent_id, description, status,
                   created_at, expires_at, revoked_at, last_used_at
            FROM api_keys
            ORDER BY created_at DESC
            "#,
        )?;

        let keys = stmt.query_map([], |row| {
            let status_str: String = row.get(4)?;
            let status = match status_str.as_str() {
                "Active" => KeyStatus::Active,
                "Revoked" => KeyStatus::Revoked,
                "Expired" => KeyStatus::Expired,
                _ => KeyStatus::Active,
            };

            let created_at: String = row.get(5)?;
            let expires_at: Option<String> = row.get(6)?;
            let _revoked_at: Option<String> = row.get(7)?;
            let last_used_at: Option<String> = row.get(8)?;

            Ok(ApiKeyInfo {
                key_id: KeyId(row.get(0)?),
                agent_id: row.get(2)?,
                description: row.get(3)?,
                status,
                created_at: DateTime::parse_from_rfc3339(&created_at)
                    .unwrap_or_default()
                    .with_timezone(&Utc),
                expires_at: expires_at.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc))
                }),
                last_used_at: last_used_at.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc))
                }),
            })
        })?;

        keys.collect::<Result<Vec<_>, _>>()
            .map_err(KeyServiceError::from)
    }

    /// Revoke an API key.
    pub fn revoke_key(&self, key_id: &KeyId) -> Result<(), KeyServiceError> {
        let now = Utc::now().to_rfc3339();
        let affected = self.conn.execute(
            "UPDATE api_keys SET status = 'Revoked', revoked_at = ?1 WHERE key_id = ?2",
            params![now, key_id.0],
        )?;

        if affected == 0 {
            return Err(KeyServiceError::KeyNotFound);
        }

        Ok(())
    }

    /// Rotate an API key (revoke old, create new).
    ///
    /// Returns the new raw API key.
    pub fn rotate_key(
        &self,
        key_id: &KeyId,
        ttl_days: Option<i64>,
    ) -> Result<(String, ApiKeyInfo), KeyServiceError> {
        // First get the agent_id from the existing key
        let agent_id: String = self
            .conn
            .query_row(
                "SELECT agent_id FROM api_keys WHERE key_id = ?1",
                params![key_id.0],
                |row| row.get(0),
            )
            .map_err(|_| KeyServiceError::KeyNotFound)?;

        // Revoke the old key
        self.revoke_key(key_id)?;

        // Create a new key
        self.create_key(agent_id, None, ttl_days)
    }

    /// Get key info by key_id.
    pub fn get_key_info(&self, key_id: &KeyId) -> Result<ApiKeyInfo, KeyServiceError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT key_id, api_key_hash, agent_id, description, status,
                   created_at, expires_at, revoked_at, last_used_at
            FROM api_keys
            WHERE key_id = ?1
            "#,
        )?;

        stmt.query_row(params![key_id.0], |row| {
            let status_str: String = row.get(4)?;
            let status = match status_str.as_str() {
                "Active" => KeyStatus::Active,
                "Revoked" => KeyStatus::Revoked,
                "Expired" => KeyStatus::Expired,
                _ => KeyStatus::Active,
            };

            let created_at: String = row.get(5)?;
            let expires_at: Option<String> = row.get(6)?;
            let _revoked_at: Option<String> = row.get(7)?;
            let last_used_at: Option<String> = row.get(8)?;

            Ok(ApiKeyInfo {
                key_id: KeyId(row.get(0)?),
                agent_id: row.get(2)?,
                description: row.get(3)?,
                status,
                created_at: DateTime::parse_from_rfc3339(&created_at)
                    .unwrap_or_default()
                    .with_timezone(&Utc),
                expires_at: expires_at.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc))
                }),
                last_used_at: last_used_at.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc))
                }),
            })
        })
        .map_err(|_| KeyServiceError::KeyNotFound)
    }

    // -------------------------------------------------------------------------
    // Public Key Management (PKI)
    // -------------------------------------------------------------------------

    /// Register a public key for a sender.
    ///
    /// If the sender already has a public key, a new version will be created
    /// and the previous version will be marked as 'Revoked'.
    pub fn register_public_key(
        &self,
        sender_id: impl Into<String>,
        public_key_hex: impl Into<String>,
    ) -> Result<PublicKey, KeyServiceError> {
        let sender_id = sender_id.into();
        let public_key_hex = public_key_hex.into();

        // Validate hex format
        if !PublicKey::validate_hex(&public_key_hex) {
            return Err(KeyServiceError::InvalidPublicKey);
        }

        let now = Utc::now();

        // Get the current max version for this sender
        let max_version: Option<i32> = self
            .conn
            .query_row(
                r#"
            SELECT MAX(version) FROM public_keys WHERE sender_id = ?1
            "#,
                params![sender_id],
                |row| row.get(0),
            )
            .ok();

        let new_version = max_version.unwrap_or(0) + 1;

        // Revoke all previous versions
        self.conn.execute(
            r#"
            UPDATE public_keys SET status = 'Revoked' WHERE sender_id = ?1 AND status = 'Active'
            "#,
            params![sender_id],
        )?;

        // Insert the new public key version
        self.conn.execute(
            r#"
            INSERT INTO public_keys (sender_id, version, public_key_hex, created_at, status)
            VALUES (?1, ?2, ?3, ?4, 'Active')
            "#,
            params![sender_id, new_version, public_key_hex, now.to_rfc3339()],
        )?;

        Ok(PublicKey {
            sender_id,
            public_key_hex,
            version: new_version,
            created_at: now,
            status: PublicKeyStatus::Active,
        })
    }

    /// Get a public key by sender_id (returns the latest active version).
    pub fn get_public_key(&self, sender_id: &str) -> Result<Option<PublicKey>, KeyServiceError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT sender_id, public_key_hex, version, created_at, status
            FROM public_keys
            WHERE sender_id = ?1 AND status = 'Active'
            ORDER BY version DESC
            LIMIT 1
            "#,
        )?;

        let result = stmt.query_row(params![sender_id], |row| {
            let status_str: String = row.get(4)?;
            let status = match status_str.as_str() {
                "Active" => PublicKeyStatus::Active,
                "Revoked" => PublicKeyStatus::Revoked,
                _ => PublicKeyStatus::Active,
            };

            let created_at: String = row.get(3)?;

            Ok(PublicKey {
                sender_id: row.get(0)?,
                public_key_hex: row.get(1)?,
                version: row.get(2)?,
                created_at: DateTime::parse_from_rfc3339(&created_at)
                    .unwrap_or_default()
                    .with_timezone(&Utc),
                status,
            })
        });

        match result {
            Ok(public_key) => Ok(Some(public_key)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(KeyServiceError::StoreError(e.to_string())),
        }
    }

    /// Get all public key versions for a sender.
    pub fn get_public_key_versions(
        &self,
        sender_id: &str,
    ) -> Result<Vec<PublicKey>, KeyServiceError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT sender_id, public_key_hex, version, created_at, status
            FROM public_keys
            WHERE sender_id = ?1
            ORDER BY version DESC
            "#,
        )?;

        let keys = stmt.query_map(params![sender_id], |row| {
            let status_str: String = row.get(4)?;
            let status = match status_str.as_str() {
                "Active" => PublicKeyStatus::Active,
                "Revoked" => PublicKeyStatus::Revoked,
                _ => PublicKeyStatus::Active,
            };

            let created_at: String = row.get(3)?;

            Ok(PublicKey {
                sender_id: row.get(0)?,
                public_key_hex: row.get(1)?,
                version: row.get(2)?,
                created_at: DateTime::parse_from_rfc3339(&created_at)
                    .unwrap_or_default()
                    .with_timezone(&Utc),
                status,
            })
        })?;

        keys.collect::<Result<Vec<_>, _>>()
            .map_err(KeyServiceError::from)
    }

    /// Revoke a public key by sender_id.
    pub fn revoke_public_key(&self, sender_id: &str) -> Result<(), KeyServiceError> {
        let affected = self.conn.execute(
            "UPDATE public_keys SET status = 'Revoked' WHERE sender_id = ?1",
            params![sender_id],
        )?;

        if affected == 0 {
            return Err(KeyServiceError::PublicKeyNotFound);
        }

        Ok(())
    }

    /// List all public keys (returns only the latest active version per sender).
    pub fn list_public_keys(&self) -> Result<Vec<PublicKey>, KeyServiceError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT sender_id, public_key_hex, version, created_at, status
            FROM public_keys
            WHERE (sender_id, version) IN (
                SELECT sender_id, MAX(version)
                FROM public_keys
                WHERE status = 'Active'
                GROUP BY sender_id
            )
            ORDER BY created_at DESC
            "#,
        )?;

        let keys = stmt.query_map([], |row| {
            let status_str: String = row.get(4)?;
            let status = match status_str.as_str() {
                "Active" => PublicKeyStatus::Active,
                "Revoked" => PublicKeyStatus::Revoked,
                _ => PublicKeyStatus::Active,
            };

            let created_at: String = row.get(3)?;

            Ok(PublicKey {
                sender_id: row.get(0)?,
                public_key_hex: row.get(1)?,
                version: row.get(2)?,
                created_at: DateTime::parse_from_rfc3339(&created_at)
                    .unwrap_or_default()
                    .with_timezone(&Utc),
                status,
            })
        })?;

        keys.collect::<Result<Vec<_>, _>>()
            .map_err(KeyServiceError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_store() -> KeyStore {
        KeyStore::memory().unwrap()
    }

    #[test]
    fn test_create_and_verify_key() {
        let store = create_test_store();

        let (raw_key, key_info) = store
            .create_key("agent-123", Some("test key".to_string()), Some(30))
            .unwrap();

        assert_eq!(key_info.agent_id, "agent-123");
        assert_eq!(key_info.status, KeyStatus::Active);
        assert!(key_info.expires_at.is_some());

        // Verify the key works
        let verified = store.verify_key(&raw_key).unwrap();
        assert_eq!(verified.key_id, key_info.key_id);
        assert_eq!(verified.agent_id, "agent-123");
    }

    #[test]
    fn test_invalid_key() {
        let store = create_test_store();

        let result = store.verify_key("invalid-key");
        assert!(matches!(result, Err(super::KeyServiceError::InvalidKey)));
    }

    #[test]
    fn test_revoke_key() {
        let store = create_test_store();

        let (raw_key, _) = store.create_key("agent-123", None, None).unwrap();

        // Verify works first
        assert!(store.verify_key(&raw_key).is_ok());

        // Revoke
        let keys = store.list_keys().unwrap();
        let key_id = &keys[0].key_id;
        store.revoke_key(key_id).unwrap();

        // Verify now fails
        let result = store.verify_key(&raw_key);
        assert!(matches!(result, Err(Revoked)));
    }

    #[test]
    fn test_rotate_key() {
        let store = create_test_store();

        let (raw_key, key_info) = store.create_key("agent-123", None, None).unwrap();
        let old_key_id = key_info.key_id.clone();

        // Rotate the key
        let (new_raw_key, _new_key_info) = store.rotate_key(&old_key_id, None).unwrap();

        // Old key should be revoked
        let result = store.verify_key(&raw_key);
        assert!(matches!(result, Err(Revoked)));

        // New key should work
        let verified = store.verify_key(&new_raw_key).unwrap();
        assert_eq!(verified.agent_id, "agent-123");
        assert!(verified.key_id != old_key_id);
    }

    #[test]
    fn test_list_keys() {
        let store = create_test_store();

        store.create_key("agent-1", None, None).unwrap();
        store.create_key("agent-2", None, None).unwrap();

        let keys = store.list_keys().unwrap();
        assert_eq!(keys.len(), 2);
    }

    // =============================================================================
    // Public Key Version Management Tests
    // =============================================================================

    /// Returns a valid 64-character hex string for testing.
    fn valid_pk_hex() -> String {
        "a".repeat(64)
    }

    /// Returns a second valid 64-character hex string for testing.
    fn valid_pk_hex_2() -> String {
        "b".repeat(64)
    }

    /// Returns a third valid 64-character hex string for testing.
    fn valid_pk_hex_3() -> String {
        "c".repeat(64)
    }

    #[test]
    fn test_register_public_key_first_version() {
        let store = create_test_store();

        let pk = store
            .register_public_key("sender-1", valid_pk_hex())
            .unwrap();

        assert_eq!(pk.sender_id, "sender-1");
        assert_eq!(pk.version, 1);
        assert_eq!(pk.status, PublicKeyStatus::Active);
    }

    #[test]
    fn test_register_public_key_increments_version() {
        let store = create_test_store();

        // Register first version
        let pk1 = store
            .register_public_key("sender-1", valid_pk_hex())
            .unwrap();
        assert_eq!(pk1.version, 1);
        assert_eq!(pk1.status, PublicKeyStatus::Active);

        // Register second version
        let pk2 = store
            .register_public_key("sender-1", valid_pk_hex_2())
            .unwrap();
        assert_eq!(pk2.version, 2);
        assert_eq!(pk2.status, PublicKeyStatus::Active);

        // First version should still exist but be revoked
        let versions = store.get_public_key_versions("sender-1").unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].version, 2); // Latest first
        assert_eq!(versions[1].version, 1);
        assert_eq!(versions[1].status, PublicKeyStatus::Revoked);
    }

    #[test]
    fn test_get_public_key_returns_latest_active() {
        let store = create_test_store();

        // Register first version
        store
            .register_public_key("sender-1", valid_pk_hex())
            .unwrap();

        // Register second version
        store
            .register_public_key("sender-1", valid_pk_hex_2())
            .unwrap();

        // get_public_key should return latest active version
        let pk = store.get_public_key("sender-1").unwrap().unwrap();
        assert_eq!(pk.version, 2);
    }

    #[test]
    fn test_list_public_keys_returns_latest_per_sender() {
        let store = create_test_store();

        // Register multiple versions for sender-1
        store
            .register_public_key("sender-1", valid_pk_hex())
            .unwrap();
        store
            .register_public_key("sender-1", valid_pk_hex_2())
            .unwrap();

        // Register one version for sender-2
        store
            .register_public_key("sender-2", valid_pk_hex_3())
            .unwrap();

        // List should return only latest active per sender
        let keys = store.list_public_keys().unwrap();
        assert_eq!(keys.len(), 2);

        // Find sender-1's entry
        let sender1_key = keys.iter().find(|k| k.sender_id == "sender-1").unwrap();
        assert_eq!(sender1_key.version, 2); // Latest version

        // Find sender-2's entry
        let sender2_key = keys.iter().find(|k| k.sender_id == "sender-2").unwrap();
        assert_eq!(sender2_key.version, 1);
    }

    #[test]
    fn test_register_invalid_public_key_hex() {
        let store = create_test_store();

        // Too short
        let result = store.register_public_key("sender-1", "abc");
        assert!(matches!(result, Err(KeyServiceError::InvalidPublicKey)));

        // Invalid characters
        let result = store.register_public_key("sender-1", "g".repeat(64));
        assert!(matches!(result, Err(KeyServiceError::InvalidPublicKey)));
    }

    #[test]
    fn test_get_public_key_not_found() {
        let store = create_test_store();

        let result = store.get_public_key("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_get_public_key_versions_not_found() {
        let store = create_test_store();

        let result = store.get_public_key_versions("nonexistent").unwrap();
        assert!(result.is_empty());
    }
}
