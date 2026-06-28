#[cfg(feature = "postgres")]
use std::marker::PhantomData;
#[cfg(feature = "postgres")]
use std::sync::Arc;

#[cfg(feature = "postgres")]
use async_trait::async_trait;
#[cfg(feature = "postgres")]
use chrono::{DateTime, Utc};
#[cfg(feature = "postgres")]
use sqlx::{postgres::PgPoolOptions, PgPool, Row};

#[cfg(feature = "postgres")]
use crate::graph::state::State;

#[cfg(feature = "postgres")]
use super::{
    checkpointer::Checkpointer, config::CheckpointConfig, error::PersistenceError,
    snapshot::StateSnapshot,
};

#[cfg(feature = "postgres")]
pub struct PostgresCheckpointer<S: State> {
    pool: Arc<PgPool>,
    schema: String,
    _state: PhantomData<S>,
}

#[cfg(feature = "postgres")]
impl<S: State> PostgresCheckpointer<S>
where
    S: serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    pub async fn new(database_url: &str) -> Result<Self, PersistenceError> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .map_err(|e| PersistenceError::DatabaseError(e.to_string()))?;

        let saver = Self {
            pool: Arc::new(pool),
            schema: "public".to_string(),
            _state: PhantomData,
        };
        saver.setup().await?;
        Ok(saver)
    }

    pub async fn with_pool(pool: PgPool) -> Result<Self, PersistenceError> {
        let saver = Self {
            pool: Arc::new(pool),
            schema: "public".to_string(),
            _state: PhantomData,
        };
        saver.setup().await?;
        Ok(saver)
    }

    pub fn with_schema(mut self, schema: impl Into<String>) -> Self {
        self.schema = schema.into();
        self
    }

    async fn setup(&self) -> Result<(), PersistenceError> {
        let sql = format!(
            r#"
            CREATE SCHEMA IF NOT EXISTS "{}";
            CREATE TABLE IF NOT EXISTS "{}".graph_checkpoints (
                thread_id TEXT NOT NULL,
                checkpoint_id TEXT NOT NULL,
                checkpoint_ns TEXT,
                parent_checkpoint_id TEXT,
                state_values JSONB NOT NULL,
                next_nodes JSONB NOT NULL,
                metadata JSONB NOT NULL DEFAULT '{{}}'::jsonb,
                created_at TEXT NOT NULL,
                at_seq BIGINT,
                PRIMARY KEY (thread_id, checkpoint_id)
            );
            CREATE INDEX IF NOT EXISTS idx_graph_checkpoints_thread_created
                ON "{}".graph_checkpoints (thread_id, created_at DESC);
            "#,
            self.schema, self.schema, self.schema
        );

        sqlx::query(&sql)
            .execute(self.pool.as_ref())
            .await
            .map_err(|e| PersistenceError::DatabaseError(e.to_string()))?;

        Ok(())
    }
}

#[cfg(feature = "postgres")]
#[async_trait]
impl<S: State> Checkpointer<S> for PostgresCheckpointer<S>
where
    S: serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    async fn put(
        &self,
        thread_id: &str,
        checkpoint: &StateSnapshot<S>,
    ) -> Result<String, PersistenceError> {
        let checkpoint_id = checkpoint
            .checkpoint_id()
            .cloned()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let state_json = serde_json::to_value(&checkpoint.values)?;
        let next_json = serde_json::to_value(&checkpoint.next)?;
        let metadata_json = serde_json::to_value(&checkpoint.metadata)?;
        let parent_checkpoint_id = checkpoint
            .parent_config
            .as_ref()
            .and_then(|c| c.checkpoint_id.as_ref())
            .cloned();

        let sql = format!(
            r#"INSERT INTO "{}".graph_checkpoints
               (thread_id, checkpoint_id, checkpoint_ns, parent_checkpoint_id,
                state_values, next_nodes, metadata, created_at, at_seq)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
               ON CONFLICT (thread_id, checkpoint_id)
               DO UPDATE SET state_values = EXCLUDED.state_values,
                             next_nodes = EXCLUDED.next_nodes,
                             metadata = EXCLUDED.metadata,
                             created_at = EXCLUDED.created_at,
                             at_seq = EXCLUDED.at_seq"#,
            self.schema
        );

        let created_at_str = checkpoint.created_at.to_rfc3339();

        sqlx::query(&sql)
            .bind(thread_id)
            .bind(&checkpoint_id)
            .bind(&checkpoint.config.checkpoint_ns)
            .bind(&parent_checkpoint_id)
            .bind(&state_json)
            .bind(&next_json)
            .bind(&metadata_json)
            .bind(&created_at_str)
            .bind(checkpoint.at_seq.map(|s| s as i64))
            .execute(self.pool.as_ref())
            .await
            .map_err(|e| PersistenceError::DatabaseError(e.to_string()))?;

        Ok(checkpoint_id)
    }

    async fn get(
        &self,
        thread_id: &str,
        checkpoint_id: Option<&str>,
    ) -> Result<Option<StateSnapshot<S>>, PersistenceError> {
        let (sql, bind_checkpoint_id);

        if let Some(cp_id) = checkpoint_id {
            bind_checkpoint_id = Some(cp_id.to_string());
            sql = format!(
                r#"SELECT checkpoint_id, checkpoint_ns, parent_checkpoint_id,
                          state_values, next_nodes, metadata, created_at, at_seq
                   FROM "{}".graph_checkpoints
                   WHERE thread_id = $1 AND checkpoint_id = $2
                   LIMIT 1"#,
                self.schema
            );
        } else {
            bind_checkpoint_id = None;
            sql = format!(
                r#"SELECT checkpoint_id, checkpoint_ns, parent_checkpoint_id,
                          state_values, next_nodes, metadata, created_at, at_seq
                   FROM "{}".graph_checkpoints
                   WHERE thread_id = $1
                   ORDER BY created_at DESC
                   LIMIT 1"#,
                self.schema
            );
        }

        let row = if let Some(ref cp_id) = bind_checkpoint_id {
            sqlx::query(&sql)
                .bind(thread_id)
                .bind(cp_id)
                .fetch_optional(self.pool.as_ref())
                .await
        } else {
            sqlx::query(&sql)
                .bind(thread_id)
                .fetch_optional(self.pool.as_ref())
                .await
        }
        .map_err(|e| PersistenceError::DatabaseError(e.to_string()))?;

        let Some(row) = row else {
            return Ok(None);
        };

        let snapshot = self.row_to_snapshot(thread_id, &row)?;
        Ok(Some(snapshot))
    }

    async fn list(
        &self,
        thread_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<StateSnapshot<S>>, PersistenceError> {
        let sql = if let Some(limit) = limit {
            format!(
                r#"SELECT checkpoint_id, checkpoint_ns, parent_checkpoint_id,
                          state_values, next_nodes, metadata, created_at, at_seq
                   FROM "{}".graph_checkpoints
                   WHERE thread_id = $1
                   ORDER BY created_at ASC
                   LIMIT {}"#,
                self.schema, limit
            )
        } else {
            format!(
                r#"SELECT checkpoint_id, checkpoint_ns, parent_checkpoint_id,
                          state_values, next_nodes, metadata, created_at, at_seq
                   FROM "{}".graph_checkpoints
                   WHERE thread_id = $1
                   ORDER BY created_at ASC"#,
                self.schema
            )
        };

        let rows = sqlx::query(&sql)
            .bind(thread_id)
            .fetch_all(self.pool.as_ref())
            .await
            .map_err(|e| PersistenceError::DatabaseError(e.to_string()))?;

        let mut snapshots = Vec::with_capacity(rows.len());
        for row in &rows {
            snapshots.push(self.row_to_snapshot(thread_id, row)?);
        }

        Ok(snapshots)
    }
}

#[cfg(feature = "postgres")]
impl<S: State> PostgresCheckpointer<S>
where
    S: serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    fn row_to_snapshot(
        &self,
        thread_id: &str,
        row: &sqlx::postgres::PgRow,
    ) -> Result<StateSnapshot<S>, PersistenceError> {
        let checkpoint_id: String = row
            .try_get("checkpoint_id")
            .map_err(|e| PersistenceError::DatabaseError(e.to_string()))?;
        let checkpoint_ns: Option<String> = row
            .try_get("checkpoint_ns")
            .map_err(|e| PersistenceError::DatabaseError(e.to_string()))?;
        let parent_checkpoint_id: Option<String> = row
            .try_get("parent_checkpoint_id")
            .map_err(|e| PersistenceError::DatabaseError(e.to_string()))?;
        let state_json: serde_json::Value = row
            .try_get("state_values")
            .map_err(|e| PersistenceError::DatabaseError(e.to_string()))?;
        let next_json: serde_json::Value = row
            .try_get("next_nodes")
            .map_err(|e| PersistenceError::DatabaseError(e.to_string()))?;
        let metadata_json: serde_json::Value = row
            .try_get("metadata")
            .map_err(|e| PersistenceError::DatabaseError(e.to_string()))?;
        let created_at_str: String = row
            .try_get("created_at")
            .map_err(|e| PersistenceError::DatabaseError(e.to_string()))?;
        let created_at: DateTime<Utc> = DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        let at_seq: Option<i64> = row
            .try_get("at_seq")
            .map_err(|e| PersistenceError::DatabaseError(e.to_string()))?;

        let values: S =
            serde_json::from_value(state_json).map_err(PersistenceError::SerializationError)?;
        let next: Vec<String> =
            serde_json::from_value(next_json).map_err(PersistenceError::SerializationError)?;
        let metadata: std::collections::HashMap<String, serde_json::Value> =
            serde_json::from_value(metadata_json).map_err(PersistenceError::SerializationError)?;

        let config = CheckpointConfig {
            thread_id: thread_id.to_string(),
            checkpoint_id: Some(checkpoint_id),
            checkpoint_ns,
        };

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
            at_seq: at_seq.map(|s| s as u64),
        })
    }
}

#[cfg(all(test, feature = "postgres"))]
mod tests {
    use super::*;
    use crate::graph::persistence::Checkpointer;
    use crate::graph::state::MessagesState;
    use crate::schemas::messages::Message;

    fn test_db_url() -> Option<String> {
        std::env::var("ORIS_TEST_POSTGRES_URL").ok()
    }

    #[tokio::test]
    async fn postgres_checkpointer_put_get_roundtrip() {
        let Some(db_url) = test_db_url() else {
            return;
        };
        let saver = PostgresCheckpointer::<MessagesState>::new(&db_url)
            .await
            .unwrap();

        let state = MessagesState::with_messages(vec![Message::new_ai_message("hello")]);
        let config = CheckpointConfig::new("pg-thread-1");
        let snapshot = StateSnapshot::new(state, vec!["node1".to_string()], config).with_at_seq(42);

        let checkpoint_id = saver.put("pg-thread-1", &snapshot).await.unwrap();
        assert!(!checkpoint_id.is_empty());

        let loaded = saver
            .get("pg-thread-1", Some(&checkpoint_id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.at_seq, Some(42));
        assert_eq!(loaded.next, vec!["node1"]);
        assert_eq!(loaded.thread_id(), "pg-thread-1");
    }

    #[tokio::test]
    async fn postgres_checkpointer_get_latest() {
        let Some(db_url) = test_db_url() else {
            return;
        };
        let saver = PostgresCheckpointer::<MessagesState>::new(&db_url)
            .await
            .unwrap();

        let thread = "pg-thread-latest";
        let config = CheckpointConfig::new(thread);

        let snap1 = StateSnapshot::new(
            MessagesState::with_messages(vec![Message::new_ai_message("one")]),
            vec!["node1".to_string()],
            config.clone(),
        );
        saver.put(thread, &snap1).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let snap2 = StateSnapshot::new(
            MessagesState::with_messages(vec![
                Message::new_ai_message("one"),
                Message::new_ai_message("two"),
            ]),
            vec!["node2".to_string()],
            config,
        );
        saver.put(thread, &snap2).await.unwrap();

        let latest = saver.get(thread, None).await.unwrap().unwrap();
        assert_eq!(latest.next, vec!["node2"]);
        assert_eq!(latest.values.messages.len(), 2);
    }

    #[tokio::test]
    async fn postgres_checkpointer_list() {
        let Some(db_url) = test_db_url() else {
            return;
        };
        let saver = PostgresCheckpointer::<MessagesState>::new(&db_url)
            .await
            .unwrap();

        let thread = "pg-thread-list";
        let config = CheckpointConfig::new(thread);

        for i in 0..3 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            let snap = StateSnapshot::new(
                MessagesState::with_messages(vec![Message::new_ai_message(&format!("msg-{i}"))]),
                vec![format!("node-{i}")],
                config.clone(),
            );
            saver.put(thread, &snap).await.unwrap();
        }

        let all = saver.list(thread, None).await.unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].next, vec!["node-0"]);
        assert_eq!(all[2].next, vec!["node-2"]);

        let limited = saver.list(thread, Some(2)).await.unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[tokio::test]
    async fn postgres_checkpointer_nonexistent_thread_returns_none() {
        let Some(db_url) = test_db_url() else {
            return;
        };
        let saver = PostgresCheckpointer::<MessagesState>::new(&db_url)
            .await
            .unwrap();

        let result = saver.get("nonexistent-thread", None).await.unwrap();
        assert!(result.is_none());

        let list = saver.list("nonexistent-thread", None).await.unwrap();
        assert!(list.is_empty());
    }
}
