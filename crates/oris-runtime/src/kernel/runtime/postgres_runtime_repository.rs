//! Postgres-backed runtime repository for scheduler/lease contracts.
//!
//! This module is feature-gated behind `kernel-postgres`.

#![cfg(feature = "kernel-postgres")]

use std::sync::{Arc, OnceLock};

use chrono::{DateTime, TimeZone, Utc};
use sqlx::{postgres::PgPoolOptions, PgPool, Row};

use crate::kernel::event::KernelError;
use crate::kernel::identity::{RunId, Seq};

use super::models::{AttemptDispatchRecord, AttemptExecutionStatus, LeaseRecord};
use super::repository::RuntimeRepository;

fn is_valid_schema_ident(schema: &str) -> bool {
    !schema.is_empty()
        && schema
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn new_db_runtime() -> Result<Arc<tokio::runtime::Runtime>, String> {
    static DB_RT: OnceLock<Result<Arc<tokio::runtime::Runtime>, String>> = OnceLock::new();
    DB_RT
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(1)
                .thread_name("oris-runtime-pg")
                .build()
                .map(Arc::new)
                .map_err(|e| e.to_string())
        })
        .clone()
}

fn map_driver_err(prefix: &str, e: impl std::fmt::Display) -> KernelError {
    KernelError::Driver(format!("{prefix}: {e}"))
}

fn is_unique_violation(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Database(db_err) => db_err.code().as_deref() == Some("23505"),
        _ => false,
    }
}

fn dt_to_ms(dt: DateTime<Utc>) -> i64 {
    dt.timestamp_millis()
}

fn ms_to_dt(ms: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(ms)
        .single()
        .unwrap_or_else(Utc::now)
}

fn parse_attempt_status(value: &str) -> AttemptExecutionStatus {
    match value {
        "leased" => AttemptExecutionStatus::Leased,
        "running" => AttemptExecutionStatus::Running,
        "retry_backoff" => AttemptExecutionStatus::RetryBackoff,
        "completed" => AttemptExecutionStatus::Completed,
        "failed" => AttemptExecutionStatus::Failed,
        "cancelled" => AttemptExecutionStatus::Cancelled,
        _ => AttemptExecutionStatus::Queued,
    }
}

#[derive(Clone)]
pub struct PostgresRuntimeRepository {
    pool: Option<PgPool>,
    schema: String,
    init_error: Option<String>,
    db_runtime: Option<Arc<tokio::runtime::Runtime>>,
    schema_ready: OnceLock<Result<(), String>>,
}

impl PostgresRuntimeRepository {
    pub fn new(database_url: impl Into<String>) -> Self {
        let database_url = database_url.into();
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_lazy(&database_url)
            .ok();
        let init_error = if pool.is_some() {
            None
        } else {
            Some("failed to initialize lazy postgres runtime pool".to_string())
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
            return Err(map_driver_err("postgres init error", err));
        }
        self.db_runtime
            .as_deref()
            .ok_or_else(|| map_driver_err("runtime not available", "no db runtime"))
    }

    fn pool(&self) -> Result<&PgPool, KernelError> {
        self.pool
            .as_ref()
            .ok_or_else(|| map_driver_err("pool not available", "no postgres pool"))
    }

    fn ensure_schema(&self) -> Result<(), KernelError> {
        if !is_valid_schema_ident(&self.schema) {
            return Err(map_driver_err("invalid schema", &self.schema));
        }

        let result = self.schema_ready.get_or_init(|| {
            let schema = self.schema.clone();
            let sql_schema = format!("CREATE SCHEMA IF NOT EXISTS \"{}\"", schema);
            let sql_attempts = format!(
                "CREATE TABLE IF NOT EXISTS \"{}\".runtime_attempts (
                    attempt_id TEXT PRIMARY KEY,
                    run_id TEXT NOT NULL,
                    attempt_no INTEGER NOT NULL,
                    status TEXT NOT NULL,
                    retry_at_ms BIGINT NULL
                )",
                schema
            );
            let sql_leases = format!(
                "CREATE TABLE IF NOT EXISTS \"{}\".runtime_leases (
                    lease_id TEXT PRIMARY KEY,
                    attempt_id TEXT NOT NULL UNIQUE,
                    worker_id TEXT NOT NULL,
                    lease_expires_at_ms BIGINT NOT NULL,
                    heartbeat_at_ms BIGINT NOT NULL,
                    version BIGINT NOT NULL
                )",
                schema
            );
            let sql_attempt_idx = format!(
                "CREATE INDEX IF NOT EXISTS idx_runtime_attempts_status_retry
                 ON \"{}\".runtime_attempts(status, retry_at_ms)",
                schema
            );
            let sql_lease_idx = format!(
                "CREATE INDEX IF NOT EXISTS idx_runtime_leases_expiry
                 ON \"{}\".runtime_leases(lease_expires_at_ms)",
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
                sqlx::query(&sql_attempts).execute(&pool).await?;
                sqlx::query(&sql_leases).execute(&pool).await?;
                sqlx::query(&sql_attempt_idx).execute(&pool).await?;
                sqlx::query(&sql_lease_idx).execute(&pool).await?;
                Ok::<(), sqlx::Error>(())
            })
            .map_err(|e| e.to_string())
        });

        result
            .clone()
            .map_err(|e| map_driver_err("schema bootstrap", e))
    }

    pub fn enqueue_attempt(&self, attempt_id: &str, run_id: &str) -> Result<(), KernelError> {
        self.ensure_schema()?;

        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let attempt_id = attempt_id.to_string();
        let run_id = run_id.to_string();
        rt.block_on(async move {
            let sql = format!(
                "INSERT INTO \"{}\".runtime_attempts (attempt_id, run_id, attempt_no, status, retry_at_ms)
                 VALUES ($1, $2, 1, 'queued', NULL)
                 ON CONFLICT(attempt_id) DO NOTHING",
                schema
            );
            sqlx::query(&sql)
                .bind(&attempt_id)
                .bind(&run_id)
                .execute(&pool)
                .await
                .map_err(|e| map_driver_err("enqueue attempt", e))?;
            Ok(())
        })
    }

    pub fn get_lease_for_attempt(
        &self,
        attempt_id: &str,
    ) -> Result<Option<LeaseRecord>, KernelError> {
        self.ensure_schema()?;

        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let attempt_id = attempt_id.to_string();
        rt.block_on(async move {
            let sql = format!(
                "SELECT lease_id, attempt_id, worker_id, lease_expires_at_ms, heartbeat_at_ms, version
                 FROM \"{}\".runtime_leases
                 WHERE attempt_id = $1",
                schema
            );
            let row = sqlx::query(&sql)
                .bind(&attempt_id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| map_driver_err("get lease by attempt", e))?;

            Ok(row.map(|row| LeaseRecord {
                lease_id: row.get::<String, _>(0),
                attempt_id: row.get::<String, _>(1),
                worker_id: row.get::<String, _>(2),
                lease_expires_at: ms_to_dt(row.get::<i64, _>(3)),
                heartbeat_at: ms_to_dt(row.get::<i64, _>(4)),
                version: row.get::<i64, _>(5) as u64,
            }))
        })
    }
}

impl RuntimeRepository for PostgresRuntimeRepository {
    fn list_dispatchable_attempts(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<AttemptDispatchRecord>, KernelError> {
        self.ensure_schema()?;

        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let now_ms = dt_to_ms(now);
        rt.block_on(async move {
            let sql = format!(
                "SELECT a.attempt_id, a.run_id, a.attempt_no, a.status, a.retry_at_ms
                 FROM \"{}\".runtime_attempts a
                 LEFT JOIN \"{}\".runtime_leases l
                   ON l.attempt_id = a.attempt_id
                  AND l.lease_expires_at_ms >= $1
                 WHERE l.attempt_id IS NULL
                   AND (
                     a.status = 'queued'
                     OR (a.status = 'retry_backoff' AND (a.retry_at_ms IS NULL OR a.retry_at_ms <= $1))
                   )
                 ORDER BY a.attempt_no ASC, a.attempt_id ASC
                 LIMIT $2",
                schema, schema
            );

            let rows = sqlx::query(&sql)
                .bind(now_ms)
                .bind(limit as i64)
                .fetch_all(&pool)
                .await
                .map_err(|e| map_driver_err("list dispatchable attempts", e))?;

            Ok(rows
                .into_iter()
                .map(|row| {
                    let retry_at_ms: Option<i64> = row.get(4);
                    AttemptDispatchRecord {
                        attempt_id: row.get(0),
                        run_id: row.get(1),
                        attempt_no: row.get::<i32, _>(2) as u32,
                        status: parse_attempt_status(row.get::<String, _>(3).as_str()),
                        retry_at: retry_at_ms.map(ms_to_dt),
                    }
                })
                .collect())
        })
    }

    fn upsert_lease(
        &self,
        attempt_id: &str,
        worker_id: &str,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<LeaseRecord, KernelError> {
        self.ensure_schema()?;

        let now = Utc::now();
        let now_ms = dt_to_ms(now);
        let lease_expires_at_ms = dt_to_ms(lease_expires_at);
        let lease_id = format!(
            "lease-{}-{}",
            attempt_id,
            now.timestamp_nanos_opt().unwrap_or(0)
        );
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let attempt_id = attempt_id.to_string();
        let worker_id = worker_id.to_string();
        let lease_id_out = lease_id.clone();

        rt.block_on(async move {
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| map_driver_err("begin upsert lease tx", e))?;

            let delete_sql = format!(
                "DELETE FROM \"{}\".runtime_leases
                 WHERE attempt_id = $1 AND lease_expires_at_ms < $2",
                schema
            );
            sqlx::query(&delete_sql)
                .bind(&attempt_id)
                .bind(now_ms)
                .execute(&mut *tx)
                .await
                .map_err(|e| map_driver_err("cleanup expired lease", e))?;

            let insert_sql = format!(
                "INSERT INTO \"{}\".runtime_leases
                 (lease_id, attempt_id, worker_id, lease_expires_at_ms, heartbeat_at_ms, version)
                 VALUES ($1, $2, $3, $4, $5, 1)",
                schema
            );
            match sqlx::query(&insert_sql)
                .bind(&lease_id)
                .bind(&attempt_id)
                .bind(&worker_id)
                .bind(lease_expires_at_ms)
                .bind(now_ms)
                .execute(&mut *tx)
                .await
            {
                Ok(_) => {}
                Err(e) if is_unique_violation(&e) => {
                    return Err(KernelError::Driver(format!(
                        "active lease already exists for attempt: {}",
                        attempt_id
                    )));
                }
                Err(e) => return Err(map_driver_err("insert lease", e)),
            }

            let update_sql = format!(
                "UPDATE \"{}\".runtime_attempts
                 SET status = 'leased'
                 WHERE attempt_id = $1 AND status IN ('queued', 'retry_backoff')",
                schema
            );
            let updated = sqlx::query(&update_sql)
                .bind(&attempt_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| map_driver_err("mark leased status", e))?
                .rows_affected();
            if updated == 0 {
                return Err(KernelError::Driver(format!(
                    "attempt is not dispatchable for lease: {}",
                    attempt_id
                )));
            }

            let version_sql = format!(
                "SELECT version FROM \"{}\".runtime_leases WHERE attempt_id = $1",
                schema
            );
            let version: i64 = sqlx::query_scalar(&version_sql)
                .bind(&attempt_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| map_driver_err("read lease version", e))?;

            tx.commit()
                .await
                .map_err(|e| map_driver_err("commit upsert lease tx", e))?;
            Ok(LeaseRecord {
                lease_id: lease_id_out,
                attempt_id,
                worker_id,
                lease_expires_at,
                heartbeat_at: now,
                version: version as u64,
            })
        })
    }

    fn heartbeat_lease(
        &self,
        lease_id: &str,
        heartbeat_at: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<(), KernelError> {
        self.ensure_schema()?;

        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let lease_id = lease_id.to_string();
        let heartbeat_at_ms = dt_to_ms(heartbeat_at);
        let lease_expires_at_ms = dt_to_ms(lease_expires_at);

        rt.block_on(async move {
            let sql = format!(
                "UPDATE \"{}\".runtime_leases
                 SET heartbeat_at_ms = $2, lease_expires_at_ms = $3, version = version + 1
                 WHERE lease_id = $1",
                schema
            );
            let updated = sqlx::query(&sql)
                .bind(&lease_id)
                .bind(heartbeat_at_ms)
                .bind(lease_expires_at_ms)
                .execute(&pool)
                .await
                .map_err(|e| map_driver_err("heartbeat lease", e))?
                .rows_affected();
            if updated == 0 {
                return Err(KernelError::Driver(format!(
                    "lease not found for heartbeat: {}",
                    lease_id
                )));
            }
            Ok(())
        })
    }

    fn expire_leases_and_requeue(&self, now: DateTime<Utc>) -> Result<u64, KernelError> {
        self.ensure_schema()?;

        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let now_ms = dt_to_ms(now);

        rt.block_on(async move {
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| map_driver_err("begin expire/requeue tx", e))?;

            let select_sql = format!(
                "SELECT attempt_id FROM \"{}\".runtime_leases WHERE lease_expires_at_ms < $1",
                schema
            );
            let rows = sqlx::query(&select_sql)
                .bind(now_ms)
                .fetch_all(&mut *tx)
                .await
                .map_err(|e| map_driver_err("query expired leases", e))?;
            let attempt_ids: Vec<String> = rows.into_iter().map(|r| r.get(0)).collect();

            for attempt_id in &attempt_ids {
                let delete_sql = format!(
                    "DELETE FROM \"{}\".runtime_leases WHERE attempt_id = $1",
                    schema
                );
                sqlx::query(&delete_sql)
                    .bind(attempt_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| map_driver_err("delete expired lease", e))?;

                let requeue_sql = format!(
                    "UPDATE \"{}\".runtime_attempts
                     SET status = 'queued'
                     WHERE attempt_id = $1
                       AND status NOT IN ('completed', 'failed', 'cancelled')",
                    schema
                );
                sqlx::query(&requeue_sql)
                    .bind(attempt_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| map_driver_err("requeue attempt", e))?;
            }

            tx.commit()
                .await
                .map_err(|e| map_driver_err("commit expire/requeue tx", e))?;
            Ok(attempt_ids.len() as u64)
        })
    }

    fn latest_seq_for_run(&self, _run_id: &RunId) -> Result<Seq, KernelError> {
        Ok(0)
    }
}

#[cfg(all(test, feature = "sqlite-persistence"))]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{Duration, Utc};

    use super::PostgresRuntimeRepository;
    use crate::kernel::runtime::{RuntimeRepository, SqliteRuntimeRepository};

    trait ContractHarness: RuntimeRepository {
        fn seed_attempt(&self, attempt_id: &str, run_id: &str);
        fn has_lease(&self, attempt_id: &str) -> bool;
    }

    impl ContractHarness for SqliteRuntimeRepository {
        fn seed_attempt(&self, attempt_id: &str, run_id: &str) {
            self.enqueue_attempt(attempt_id, run_id)
                .expect("enqueue sqlite attempt");
        }

        fn has_lease(&self, attempt_id: &str) -> bool {
            self.get_lease_for_attempt(attempt_id)
                .expect("sqlite get lease")
                .is_some()
        }
    }

    impl ContractHarness for PostgresRuntimeRepository {
        fn seed_attempt(&self, attempt_id: &str, run_id: &str) {
            self.enqueue_attempt(attempt_id, run_id)
                .expect("enqueue postgres attempt");
        }

        fn has_lease(&self, attempt_id: &str) -> bool {
            self.get_lease_for_attempt(attempt_id)
                .expect("postgres get lease")
                .is_some()
        }
    }

    fn assert_dispatch_lease_requeue_contract<R: ContractHarness>(repo: &R, name: &str) {
        let run_id = format!("run-{}", name);
        let attempt_id = format!("attempt-{}", name);
        let now = Utc::now();

        repo.seed_attempt(&attempt_id, &run_id);
        let initial = repo
            .list_dispatchable_attempts(now, 10)
            .expect("list dispatchable initial");
        assert!(initial.iter().any(|r| r.attempt_id == attempt_id));

        let lease = repo
            .upsert_lease(&attempt_id, "worker-a", now + Duration::seconds(1))
            .expect("upsert lease");
        assert!(repo.has_lease(&attempt_id));

        let duplicate = repo.upsert_lease(&attempt_id, "worker-b", now + Duration::seconds(2));
        assert!(duplicate.is_err());

        let hidden = repo
            .list_dispatchable_attempts(now, 10)
            .expect("list dispatchable while leased");
        assert!(!hidden.iter().any(|r| r.attempt_id == attempt_id));

        repo.heartbeat_lease(
            &lease.lease_id,
            now + Duration::milliseconds(500),
            now + Duration::seconds(2),
        )
        .expect("heartbeat lease");

        let expired = repo
            .expire_leases_and_requeue(now + Duration::seconds(10))
            .expect("expire and requeue");
        assert_eq!(expired, 1);

        let available = repo
            .list_dispatchable_attempts(now + Duration::seconds(10), 10)
            .expect("list dispatchable after requeue");
        assert!(available.iter().any(|r| r.attempt_id == attempt_id));

        assert_eq!(repo.latest_seq_for_run(&run_id).expect("latest seq"), 0);
    }

    fn test_db_url() -> Option<String> {
        std::env::var("ORIS_TEST_POSTGRES_URL").ok()
    }

    fn test_schema() -> String {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        format!("oris_runtime_repo_test_{}", ts)
    }

    #[test]
    fn runtime_repository_contract_sqlite() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("sqlite repo");
        assert_dispatch_lease_requeue_contract(&repo, "sqlite");
    }

    #[test]
    fn runtime_repository_contract_postgres_when_env_is_set() {
        let Some(db_url) = test_db_url() else {
            return;
        };
        let repo = PostgresRuntimeRepository::new(db_url).with_schema(test_schema());
        assert_dispatch_lease_requeue_contract(&repo, "postgres");
    }
}
