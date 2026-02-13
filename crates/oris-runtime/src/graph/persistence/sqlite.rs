#[cfg(feature = "sqlite-persistence")]
use std::marker::PhantomData;
#[cfg(feature = "sqlite-persistence")]
use std::sync::Arc;

#[cfg(feature = "sqlite-persistence")]
use async_trait::async_trait;
#[cfg(feature = "sqlite-persistence")]
use chrono::{DateTime, Utc};
#[cfg(feature = "sqlite-persistence")]
use rusqlite::{params, Connection};
#[cfg(feature = "sqlite-persistence")]
use serde_json::Value;
#[cfg(feature = "sqlite-persistence")]
use tokio::sync::Mutex;

#[cfg(feature = "sqlite-persistence")]
use crate::graph::state::State;

#[cfg(feature = "sqlite-persistence")]
use super::{
    checkpointer::Checkpointer, config::CheckpointConfig, error::PersistenceError,
    snapshot::StateSnapshot,
};

#[cfg(feature = "sqlite-persistence")]
/// SQLite-based checkpointer implementation
///
/// This provides persistent storage for checkpoints using SQLite.
/// Checkpoints are stored in a local database file.
pub struct SqliteSaver<S: State> {
    connection: Arc<Mutex<Connection>>,
    #[allow(dead_code)]
    state: PhantomData<S>,
}

#[cfg(feature = "sqlite-persistence")]
impl<S: State> SqliteSaver<S>
where
    S: serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    /// Create a new SqliteSaver with a database file path
    pub fn new(path: &str) -> Result<Self, PersistenceError> {
        let connection = Connection::open(path)?;
        let saver = Self {
            connection: Arc::new(Mutex::new(connection)),
            state: PhantomData,
        };
        saver.setup()?;
        Ok(saver)
    }

    /// Create a new SqliteSaver with an in-memory database
    pub fn new_in_memory() -> Result<Self, PersistenceError> {
        let connection = Connection::open_in_memory()?;
        let saver = Self {
            connection: Arc::new(Mutex::new(connection)),
            state: PhantomData,
        };
        saver.setup()?;
        Ok(saver)
    }

    /// Setup the database schema
    fn setup(&self) -> Result<(), PersistenceError> {
        let conn = self.connection.blocking_lock();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS checkpoints (
                thread_id TEXT NOT NULL,
                checkpoint_id TEXT PRIMARY KEY,
                checkpoint_ns TEXT,
                parent_checkpoint_id TEXT,
                state_values BLOB NOT NULL,
                next_nodes TEXT NOT NULL,
                metadata TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_thread_id ON checkpoints(thread_id)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_created_at ON checkpoints(created_at)",
            [],
        )?;

        Ok(())
    }
}

#[cfg(feature = "sqlite-persistence")]
#[async_trait]
impl<S: State> Checkpointer<S> for SqliteSaver<S>
where
    S: serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    async fn put(
        &self,
        thread_id: &str,
        checkpoint: &StateSnapshot<S>,
    ) -> Result<String, PersistenceError> {
        let checkpoint_id = checkpoint.checkpoint_id().cloned().unwrap_or_else(|| {
            #[cfg(feature = "uuid")]
            {
                uuid::Uuid::new_v4().to_string()
            }
            #[cfg(not(feature = "uuid"))]
            {
                use std::time::{SystemTime, UNIX_EPOCH};
                format!(
                    "checkpoint-{}",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                )
            }
        });

        // Serialize state using serde_json
        let state_bytes =
            serde_json::to_vec(&checkpoint.values).map_err(PersistenceError::SerializationError)?;

        // Serialize next nodes and metadata
        let next_nodes_json = serde_json::to_string(&checkpoint.next)?;
        let metadata_json = serde_json::to_string(&checkpoint.metadata)?;

        let created_at = checkpoint.created_at.to_rfc3339();
        let parent_checkpoint_id = checkpoint
            .parent_config
            .as_ref()
            .and_then(|c| c.checkpoint_id.as_ref())
            .map(|s| s.as_str());

        let conn = self.connection.lock().await;
        conn.execute(
            "INSERT INTO checkpoints (
                thread_id, checkpoint_id, checkpoint_ns, parent_checkpoint_id,
                state_values, next_nodes, metadata, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                thread_id,
                checkpoint_id,
                checkpoint.config.checkpoint_ns,
                parent_checkpoint_id,
                state_bytes,
                next_nodes_json,
                metadata_json,
                created_at,
            ],
        )?;

        Ok(checkpoint_id)
    }

    async fn get(
        &self,
        thread_id: &str,
        checkpoint_id: Option<&str>,
    ) -> Result<Option<StateSnapshot<S>>, PersistenceError> {
        let conn = self.connection.lock().await;

        let query = if let Some(_cp_id) = checkpoint_id {
            "SELECT checkpoint_id, checkpoint_ns, parent_checkpoint_id, state_values, 
                    next_nodes, metadata, created_at
             FROM checkpoints 
             WHERE thread_id = ?1 AND checkpoint_id = ?2
             ORDER BY created_at DESC LIMIT 1"
        } else {
            "SELECT checkpoint_id, checkpoint_ns, parent_checkpoint_id, state_values, 
                    next_nodes, metadata, created_at
             FROM checkpoints 
             WHERE thread_id = ?1
             ORDER BY created_at DESC LIMIT 1"
        };

        let mut stmt = conn.prepare(query)?;
        let params = if checkpoint_id.is_some() {
            params![thread_id, checkpoint_id]
        } else {
            params![thread_id]
        };

        let result = stmt.query_row(params, |row| {
            let checkpoint_id: String = row.get(0)?;
            let checkpoint_ns: Option<String> = row.get(1)?;
            let parent_checkpoint_id: Option<String> = row.get(2)?;
            let state_bytes: Vec<u8> = row.get(3)?;
            let next_nodes_json: String = row.get(4)?;
            let metadata_json: String = row.get(5)?;
            let created_at_str: String = row.get(6)?;

            // Deserialize state using serde_json (map to rusqlite::Error for closure return type)
            let values: S = serde_json::from_slice(&state_bytes).map_err(|_e| {
                rusqlite::Error::InvalidColumnType(
                    3,
                    "state_values".to_string(),
                    rusqlite::types::Type::Blob,
                )
            })?;

            // Deserialize next nodes and metadata
            let next: Vec<String> = serde_json::from_str(&next_nodes_json).map_err(|_e| {
                rusqlite::Error::InvalidColumnType(
                    4,
                    "next_nodes".to_string(),
                    rusqlite::types::Type::Text,
                )
            })?;
            let metadata: std::collections::HashMap<String, Value> =
                serde_json::from_str(&metadata_json).map_err(|_e| {
                    rusqlite::Error::InvalidColumnType(
                        5,
                        "metadata".to_string(),
                        rusqlite::types::Type::Text,
                    )
                })?;

            // Parse created_at
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|_e| {
                    rusqlite::Error::InvalidColumnType(
                        7,
                        "created_at".to_string(),
                        rusqlite::types::Type::Text,
                    )
                })?
                .with_timezone(&Utc);

            // Build config
            let config = CheckpointConfig {
                thread_id: thread_id.to_string(),
                checkpoint_id: Some(checkpoint_id.clone()),
                checkpoint_ns,
            };

            // Build parent config if exists
            let parent_config = parent_checkpoint_id.map(|parent_id| CheckpointConfig {
                thread_id: thread_id.to_string(),
                checkpoint_id: Some(parent_id),
                checkpoint_ns: None,
            });

            Ok(StateSnapshot {
                values,
                next,
                config,
                metadata,
                created_at,
                parent_config,
            })
        });

        match result {
            Ok(snapshot) => Ok(Some(snapshot)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(PersistenceError::DatabaseError(e.to_string())),
        }
    }

    async fn list(
        &self,
        thread_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<StateSnapshot<S>>, PersistenceError> {
        let conn = self.connection.lock().await;

        let limit_clause = limit.map(|l| format!("LIMIT {}", l)).unwrap_or_default();
        let query = format!(
            "SELECT checkpoint_id, checkpoint_ns, parent_checkpoint_id, state_values, 
                    next_nodes, metadata, created_at
             FROM checkpoints 
             WHERE thread_id = ?1
             ORDER BY created_at ASC {}",
            limit_clause
        );

        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(params![thread_id], |row| {
            let checkpoint_id: String = row.get(0)?;
            let checkpoint_ns: Option<String> = row.get(1)?;
            let parent_checkpoint_id: Option<String> = row.get(2)?;
            let state_bytes: Vec<u8> = row.get(3)?;
            let next_nodes_json: String = row.get(4)?;
            let metadata_json: String = row.get(5)?;
            let created_at_str: String = row.get(6)?;

            // Deserialize state using serde_json (map to rusqlite::Error for closure return type)
            let values: S = serde_json::from_slice(&state_bytes).map_err(|_e| {
                rusqlite::Error::InvalidColumnType(
                    3,
                    "state_values".to_string(),
                    rusqlite::types::Type::Blob,
                )
            })?;

            // Deserialize next nodes and metadata
            let next: Vec<String> = serde_json::from_str(&next_nodes_json).map_err(|_e| {
                rusqlite::Error::InvalidColumnType(
                    4,
                    "next_nodes".to_string(),
                    rusqlite::types::Type::Text,
                )
            })?;
            let metadata: std::collections::HashMap<String, Value> =
                serde_json::from_str(&metadata_json).map_err(|_e| {
                    rusqlite::Error::InvalidColumnType(
                        5,
                        "metadata".to_string(),
                        rusqlite::types::Type::Text,
                    )
                })?;

            // Parse created_at
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|_e| {
                    rusqlite::Error::InvalidColumnType(
                        7,
                        "created_at".to_string(),
                        rusqlite::types::Type::Text,
                    )
                })?
                .with_timezone(&Utc);

            // Build config
            let config = CheckpointConfig {
                thread_id: thread_id.to_string(),
                checkpoint_id: Some(checkpoint_id.clone()),
                checkpoint_ns,
            };

            // Build parent config if exists
            let parent_config = parent_checkpoint_id.map(|parent_id| CheckpointConfig {
                thread_id: thread_id.to_string(),
                checkpoint_id: Some(parent_id),
                checkpoint_ns: None,
            });

            Ok(StateSnapshot {
                values,
                next,
                config,
                metadata,
                created_at,
                parent_config,
            })
        })?;

        let mut snapshots = Vec::new();
        for row in rows {
            snapshots.push(row.map_err(|e| PersistenceError::DatabaseError(e.to_string()))?);
        }

        Ok(snapshots)
    }
}

#[cfg(all(test, feature = "sqlite-persistence"))]
mod tests {
    use super::*;
    use crate::graph::state::MessagesState;
    use crate::schemas::messages::Message;
    use std::fs;

    #[tokio::test]
    #[ignore = "SqliteSaver::new blocks; cannot run inside tokio runtime"]
    async fn test_sqlite_saver_file() {
        let db_path = "test_checkpoints.db";
        let _ = fs::remove_file(db_path);

        let saver = SqliteSaver::<MessagesState>::new(db_path).unwrap();

        let state = MessagesState::new();
        let config = CheckpointConfig::new("thread-1");
        let snapshot = StateSnapshot::new(state, vec!["node1".to_string()], config);

        let checkpoint_id = saver.put("thread-1", &snapshot).await.unwrap();
        assert!(!checkpoint_id.is_empty());

        let retrieved = saver.get("thread-1", None).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().thread_id(), "thread-1");

        let list = saver.list("thread-1", None).await.unwrap();
        assert_eq!(list.len(), 1);

        let _ = fs::remove_file(db_path);
    }

    /// Put, get latest, get by id, list. Run with: cargo test -p oris-runtime --features sqlite-persistence -- --ignored
    #[tokio::test]
    #[ignore = "SQLite uses blocking I/O; run with --ignored"]
    async fn test_sqlite_saver_put_get_list() {
        let saver = SqliteSaver::<MessagesState>::new_in_memory().unwrap();
        let state = MessagesState::with_messages(vec![Message::new_ai_message("hello")]);
        let config = CheckpointConfig::new("thread-sqlite");
        let snapshot = StateSnapshot::new(state, vec!["node1".to_string()], config);

        let id1 = saver.put("thread-sqlite", &snapshot).await.unwrap();
        assert!(!id1.is_empty());

        let latest = saver.get("thread-sqlite", None).await.unwrap();
        assert!(latest.is_some());
        assert_eq!(latest.as_ref().unwrap().thread_id(), "thread-sqlite");
        assert_eq!(
            latest.unwrap().config.checkpoint_id.as_deref(),
            Some(id1.as_str())
        );

        let by_id = saver.get("thread-sqlite", Some(&id1)).await.unwrap();
        assert!(by_id.is_some());

        let list = saver.list("thread-sqlite", None).await.unwrap();
        assert_eq!(list.len(), 1);
    }

    #[tokio::test]
    #[ignore = "SQLite uses blocking I/O; run with --ignored"]
    async fn test_sqlite_saver_multiple_checkpoints_and_get_by_id() {
        let saver = SqliteSaver::<MessagesState>::new_in_memory().unwrap();
        let config = CheckpointConfig::new("thread-multi");

        let snap1 = StateSnapshot::new(
            MessagesState::with_messages(vec![Message::new_ai_message("one")]),
            vec!["node2".to_string()],
            config.clone(),
        );
        let id1 = saver.put("thread-multi", &snap1).await.unwrap();

        let mut config2 = config.clone();
        config2.checkpoint_id = Some(id1.clone());
        let snap2 = StateSnapshot::new(
            MessagesState::with_messages(vec![
                Message::new_ai_message("one"),
                Message::new_ai_message("two"),
            ]),
            vec!["END".to_string()],
            config2,
        );
        let id2 = saver.put("thread-multi", &snap2).await.unwrap();

        let list = saver.list("thread-multi", None).await.unwrap();
        assert_eq!(list.len(), 2);

        let latest = saver.get("thread-multi", None).await.unwrap().unwrap();
        assert_eq!(latest.config.checkpoint_id.as_deref(), Some(id2.as_str()));

        let first = saver
            .get("thread-multi", Some(&id1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.values.messages.len(), 1);
    }
}
