//! Postgres-backed kernel stores for event log and snapshots.
//!
//! This module is feature-gated behind `kernel-postgres`.

#[cfg(feature = "kernel-postgres")]
use std::marker::PhantomData;
#[cfg(feature = "kernel-postgres")]
use std::sync::{Arc, OnceLock};

#[cfg(feature = "kernel-postgres")]
use serde::{de::DeserializeOwned, Serialize};
#[cfg(feature = "kernel-postgres")]
use sqlx::{postgres::PgPoolOptions, PgPool};

#[cfg(feature = "kernel-postgres")]
use crate::kernel::event::{Event, EventStore, KernelError, SequencedEvent};
#[cfg(feature = "kernel-postgres")]
use crate::kernel::identity::{RunId, Seq};
#[cfg(feature = "kernel-postgres")]
use crate::kernel::snapshot::{Snapshot, SnapshotStore};

#[cfg(feature = "kernel-postgres")]
fn is_valid_schema_ident(schema: &str) -> bool {
    !schema.is_empty()
        && schema
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(feature = "kernel-postgres")]
fn new_db_runtime() -> Result<Arc<tokio::runtime::Runtime>, String> {
    static DB_RT: OnceLock<Result<Arc<tokio::runtime::Runtime>, String>> = OnceLock::new();
    DB_RT
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(1)
                .thread_name("oris-kernel-pg")
                .build()
                .map(Arc::new)
                .map_err(|e| e.to_string())
        })
        .clone()
}

#[cfg(feature = "kernel-postgres")]
fn map_event_err(prefix: &str, e: impl std::fmt::Display) -> KernelError {
    KernelError::EventStore(format!("{prefix}: {e}"))
}

#[cfg(feature = "kernel-postgres")]
fn map_snapshot_err(prefix: &str, e: impl std::fmt::Display) -> KernelError {
    KernelError::SnapshotStore(format!("{prefix}: {e}"))
}

/// Postgres-backed event log store.
#[cfg(feature = "kernel-postgres")]
pub struct PostgresEventStore {
    pool: Option<PgPool>,
    schema: String,
    init_error: Option<String>,
    db_runtime: Option<Arc<tokio::runtime::Runtime>>,
    schema_ready: OnceLock<Result<(), String>>,
}

#[cfg(feature = "kernel-postgres")]
impl PostgresEventStore {
    pub fn new(database_url: impl Into<String>) -> Self {
        let database_url = database_url.into();
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_lazy(&database_url)
            .ok();
        let init_error = if pool.is_some() {
            None
        } else {
            Some("failed to initialize lazy postgres pool".to_string())
        };
        let db_runtime = new_db_runtime().ok();

        Self {
            pool,
            schema: "public".to_string(),
            init_error,
            db_runtime,
            schema_ready: OnceLock::new(),
        }
    }

    pub fn with_pool(pool: PgPool) -> Self {
        Self {
            pool: Some(pool),
            schema: "public".to_string(),
            init_error: None,
            db_runtime: new_db_runtime().ok(),
            schema_ready: OnceLock::new(),
        }
    }

    pub fn with_schema(mut self, schema: impl Into<String>) -> Self {
        self.schema = schema.into();
        self
    }

    fn runtime(&self) -> Result<&tokio::runtime::Runtime, KernelError> {
        if let Some(err) = &self.init_error {
            return Err(map_event_err("postgres init error", err));
        }
        self.db_runtime
            .as_deref()
            .ok_or_else(|| map_event_err("runtime not available", "no db runtime"))
    }

    fn pool(&self) -> Result<&PgPool, KernelError> {
        self.pool
            .as_ref()
            .ok_or_else(|| map_event_err("pool not available", "no postgres pool"))
    }

    fn ensure_schema(&self) -> Result<(), KernelError> {
        if !is_valid_schema_ident(&self.schema) {
            return Err(map_event_err("invalid schema", &self.schema));
        }

        let result = self.schema_ready.get_or_init(|| {
            let schema = self.schema.clone();
            let sql_schema = format!("CREATE SCHEMA IF NOT EXISTS \"{}\"", schema);
            let sql_events = format!(
                "CREATE TABLE IF NOT EXISTS \"{}\".kernel_events (
                    run_id TEXT NOT NULL,
                    seq BIGINT NOT NULL,
                    event_json JSONB NOT NULL,
                    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                    PRIMARY KEY (run_id, seq)
                )",
                schema
            );
            let sql_idx = format!(
                "CREATE INDEX IF NOT EXISTS idx_kernel_events_run_created
                 ON \"{}\".kernel_events (run_id, created_at)",
                schema
            );

            let pool = match self.pool() {
                Ok(p) => p.clone(),
                Err(e) => return Err(e.to_string()),
            };
            let rt = match self.runtime() {
                Ok(r) => r,
                Err(e) => return Err(e.to_string()),
            };

            rt.block_on(async {
                sqlx::query(&sql_schema).execute(&pool).await?;
                sqlx::query(&sql_events).execute(&pool).await?;
                sqlx::query(&sql_idx).execute(&pool).await?;
                Ok::<(), sqlx::Error>(())
            })
            .map_err(|e| e.to_string())
        });

        result
            .clone()
            .map_err(|e| map_event_err("schema bootstrap", e))
    }
}

#[cfg(feature = "kernel-postgres")]
impl EventStore for PostgresEventStore {
    fn append(&self, run_id: &RunId, events: &[Event]) -> Result<Seq, KernelError> {
        self.ensure_schema()?;

        if events.is_empty() {
            return self.head(run_id);
        }

        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let run_id = run_id.clone();
        let events_to_write = events.to_vec();

        rt.block_on(async move {
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| map_event_err("begin tx", e))?;

            // Serialize append per run_id to preserve contiguous seq assignment.
            sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
                .bind(&run_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| map_event_err("advisory lock", e))?;

            let head_sql = format!(
                "SELECT COALESCE(MAX(seq), 0) FROM \"{}\".kernel_events WHERE run_id = $1",
                schema
            );
            let current_head: i64 = sqlx::query_scalar(&head_sql)
                .bind(&run_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| map_event_err("read head", e))?;

            let insert_sql = format!(
                "INSERT INTO \"{}\".kernel_events (run_id, seq, event_json) VALUES ($1, $2, $3)",
                schema
            );

            let mut last_seq = current_head as Seq;
            for (i, event) in events_to_write.iter().enumerate() {
                let seq = current_head + i as i64 + 1;
                sqlx::query(&insert_sql)
                    .bind(&run_id)
                    .bind(seq)
                    .bind(sqlx::types::Json(event))
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| map_event_err("insert event", e))?;
                last_seq = seq as Seq;
            }

            tx.commit()
                .await
                .map_err(|e| map_event_err("commit tx", e))?;
            Ok(last_seq)
        })
    }

    fn scan(&self, run_id: &RunId, from: Seq) -> Result<Vec<SequencedEvent>, KernelError> {
        self.ensure_schema()?;

        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let run_id = run_id.clone();

        rt.block_on(async move {
            let sql = format!(
                "SELECT seq, event_json
                 FROM \"{}\".kernel_events
                 WHERE run_id = $1 AND seq >= $2
                 ORDER BY seq ASC",
                schema
            );

            let rows: Vec<(i64, sqlx::types::Json<Event>)> = sqlx::query_as(&sql)
                .bind(&run_id)
                .bind(from as i64)
                .fetch_all(&pool)
                .await
                .map_err(|e| map_event_err("scan events", e))?;

            Ok(rows
                .into_iter()
                .map(|(seq, evt)| SequencedEvent {
                    seq: seq as Seq,
                    event: evt.0,
                })
                .collect())
        })
    }

    fn head(&self, run_id: &RunId) -> Result<Seq, KernelError> {
        self.ensure_schema()?;

        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let run_id = run_id.clone();

        rt.block_on(async move {
            let sql = format!(
                "SELECT COALESCE(MAX(seq), 0) FROM \"{}\".kernel_events WHERE run_id = $1",
                schema
            );
            let head: i64 = sqlx::query_scalar(&sql)
                .bind(&run_id)
                .fetch_one(&pool)
                .await
                .map_err(|e| map_event_err("head query", e))?;
            Ok(head as Seq)
        })
    }
}

/// Postgres-backed snapshot store.
#[cfg(feature = "kernel-postgres")]
pub struct PostgresSnapshotStore<S> {
    pool: Option<PgPool>,
    schema: String,
    init_error: Option<String>,
    db_runtime: Option<Arc<tokio::runtime::Runtime>>,
    schema_ready: OnceLock<Result<(), String>>,
    _state: PhantomData<S>,
}

#[cfg(feature = "kernel-postgres")]
impl<S> PostgresSnapshotStore<S> {
    pub fn new(database_url: impl Into<String>) -> Self {
        let database_url = database_url.into();
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_lazy(&database_url)
            .ok();
        let init_error = if pool.is_some() {
            None
        } else {
            Some("failed to initialize lazy postgres pool".to_string())
        };
        let db_runtime = new_db_runtime().ok();

        Self {
            pool,
            schema: "public".to_string(),
            init_error,
            db_runtime,
            schema_ready: OnceLock::new(),
            _state: PhantomData,
        }
    }

    pub fn with_pool(pool: PgPool) -> Self {
        Self {
            pool: Some(pool),
            schema: "public".to_string(),
            init_error: None,
            db_runtime: new_db_runtime().ok(),
            schema_ready: OnceLock::new(),
            _state: PhantomData,
        }
    }

    pub fn with_schema(mut self, schema: impl Into<String>) -> Self {
        self.schema = schema.into();
        self
    }

    fn runtime(&self) -> Result<&tokio::runtime::Runtime, KernelError> {
        if let Some(err) = &self.init_error {
            return Err(map_snapshot_err("postgres init error", err));
        }
        self.db_runtime
            .as_deref()
            .ok_or_else(|| map_snapshot_err("runtime not available", "no db runtime"))
    }

    fn pool(&self) -> Result<&PgPool, KernelError> {
        self.pool
            .as_ref()
            .ok_or_else(|| map_snapshot_err("pool not available", "no postgres pool"))
    }

    fn ensure_schema(&self) -> Result<(), KernelError> {
        if !is_valid_schema_ident(&self.schema) {
            return Err(map_snapshot_err("invalid schema", &self.schema));
        }

        let result = self.schema_ready.get_or_init(|| {
            let schema = self.schema.clone();
            let sql_schema = format!("CREATE SCHEMA IF NOT EXISTS \"{}\"", schema);
            let sql_snapshots = format!(
                "CREATE TABLE IF NOT EXISTS \"{}\".kernel_snapshots (
                    run_id TEXT NOT NULL,
                    at_seq BIGINT NOT NULL,
                    state_json JSONB NOT NULL,
                    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                    PRIMARY KEY (run_id, at_seq)
                )",
                schema
            );
            let sql_idx = format!(
                "CREATE INDEX IF NOT EXISTS idx_kernel_snapshots_run_created
                 ON \"{}\".kernel_snapshots (run_id, created_at DESC)",
                schema
            );

            let pool = match self.pool() {
                Ok(p) => p.clone(),
                Err(e) => return Err(e.to_string()),
            };
            let rt = match self.runtime() {
                Ok(r) => r,
                Err(e) => return Err(e.to_string()),
            };

            rt.block_on(async {
                sqlx::query(&sql_schema).execute(&pool).await?;
                sqlx::query(&sql_snapshots).execute(&pool).await?;
                sqlx::query(&sql_idx).execute(&pool).await?;
                Ok::<(), sqlx::Error>(())
            })
            .map_err(|e| e.to_string())
        });

        result
            .clone()
            .map_err(|e| map_snapshot_err("schema bootstrap", e))
    }
}

#[cfg(feature = "kernel-postgres")]
impl<S> SnapshotStore<S> for PostgresSnapshotStore<S>
where
    S: Clone + Send + Sync + Serialize + DeserializeOwned + Unpin + 'static,
{
    fn load_latest(&self, run_id: &RunId) -> Result<Option<Snapshot<S>>, KernelError> {
        self.ensure_schema()?;

        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let run_id = run_id.clone();

        rt.block_on(async move {
            let sql = format!(
                "SELECT run_id, at_seq, state_json
                 FROM \"{}\".kernel_snapshots
                 WHERE run_id = $1
                 ORDER BY at_seq DESC
                 LIMIT 1",
                schema
            );
            let row: Option<(String, i64, sqlx::types::Json<S>)> = sqlx::query_as(&sql)
                .bind(&run_id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| map_snapshot_err("load latest snapshot", e))?;

            Ok(row.map(|(run_id, at_seq, state_json)| Snapshot {
                run_id,
                at_seq: at_seq as Seq,
                state: state_json.0,
            }))
        })
    }

    fn save(&self, snapshot: &Snapshot<S>) -> Result<(), KernelError> {
        self.ensure_schema()?;

        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let run_id = snapshot.run_id.clone();
        let at_seq = snapshot.at_seq as i64;
        let state = snapshot.state.clone();

        rt.block_on(async move {
            let sql = format!(
                "INSERT INTO \"{}\".kernel_snapshots (run_id, at_seq, state_json)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (run_id, at_seq)
                 DO UPDATE SET state_json = EXCLUDED.state_json, created_at = NOW()",
                schema
            );
            sqlx::query(&sql)
                .bind(run_id)
                .bind(at_seq)
                .bind(sqlx::types::Json(state))
                .execute(&pool)
                .await
                .map_err(|e| map_snapshot_err("save snapshot", e))?;
            Ok(())
        })
    }
}

#[cfg(all(test, feature = "kernel-postgres"))]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{PostgresEventStore, PostgresSnapshotStore};
    use crate::kernel::{Event, EventStore, Snapshot, SnapshotStore};

    fn test_schema() -> String {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        format!("oris_kernel_test_{}", ts)
    }

    fn test_db_url() -> Option<String> {
        std::env::var("ORIS_TEST_POSTGRES_URL").ok()
    }

    #[tokio::test]
    async fn postgres_store_construction_is_compile_safe() {
        let _events = PostgresEventStore::new("postgres://localhost/oris").with_schema("oris");
        let _snaps: PostgresSnapshotStore<serde_json::Value> =
            PostgresSnapshotStore::new("postgres://localhost/oris").with_schema("oris");
    }

    #[tokio::test]
    async fn postgres_event_store_roundtrip_when_env_is_set() {
        let Some(db_url) = test_db_url() else {
            return;
        };
        let store = PostgresEventStore::new(db_url).with_schema(test_schema());
        let run_id = "run-events-roundtrip".to_string();

        let initial_head = store.head(&run_id).unwrap();
        assert_eq!(initial_head, 0);

        let seq = store
            .append(
                &run_id,
                &[Event::StateUpdated {
                    step_id: Some("n1".to_string()),
                    payload: serde_json::json!({"v": 1}),
                }],
            )
            .unwrap();
        assert_eq!(seq, 1);

        let seq2 = store
            .append(
                &run_id,
                &[
                    Event::Resumed {
                        value: serde_json::json!(true),
                    },
                    Event::Completed,
                ],
            )
            .unwrap();
        assert_eq!(seq2, 3);

        let scanned = store.scan(&run_id, 2).unwrap();
        assert_eq!(scanned.len(), 2);
        assert_eq!(store.head(&run_id).unwrap(), 3);
    }

    #[tokio::test]
    async fn postgres_snapshot_store_roundtrip_when_env_is_set() {
        let Some(db_url) = test_db_url() else {
            return;
        };
        let store: PostgresSnapshotStore<serde_json::Value> =
            PostgresSnapshotStore::new(db_url).with_schema(test_schema());

        let run_id = "run-snapshot-roundtrip".to_string();
        store
            .save(&Snapshot {
                run_id: run_id.clone(),
                at_seq: 7,
                state: serde_json::json!({"k": "v"}),
            })
            .unwrap();

        let latest = store.load_latest(&run_id).unwrap().unwrap();
        assert_eq!(latest.at_seq, 7);
        assert_eq!(latest.state["k"], "v");
    }
}
