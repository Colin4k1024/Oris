//! Postgres-backed runtime repository for scheduler/lease contracts.
//!
//! This module is feature-gated behind `kernel-postgres`.

use std::sync::{Arc, OnceLock};

use chrono::{DateTime, TimeZone, Utc};
use sqlx::{postgres::PgPoolOptions, PgPool, Row};

use crate::kernel::event::KernelError;
use crate::kernel::identity::{RunId, Seq};

use super::models::{AttemptDispatchRecord, AttemptExecutionStatus, LeaseRecord};
use super::repository::RuntimeRepository;

const POSTGRES_RUNTIME_SCHEMA_VERSION: i64 = 2;

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
        let db_runtime = new_db_runtime().ok();
        let pool = db_runtime.as_ref().and_then(|rt| {
            let _guard = rt.enter();
            PgPoolOptions::new()
                .max_connections(5)
                .connect_lazy(&database_url)
                .ok()
        });
        let init_error = if pool.is_some() {
            None
        } else if db_runtime.is_none() {
            Some("failed to initialize postgres runtime".to_string())
        } else {
            Some("failed to initialize lazy postgres runtime pool".to_string())
        };

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
            let sql_migration_table = format!(
                "CREATE TABLE IF NOT EXISTS \"{}\".runtime_schema_migrations (
                    version INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    applied_at_ms BIGINT NOT NULL
                )",
                schema
            );
            let sql_current_version = format!(
                "SELECT COALESCE(MAX(version), 0)::BIGINT FROM \"{}\".runtime_schema_migrations",
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
                sqlx::query(&sql_schema)
                    .execute(&pool)
                    .await
                    .map_err(|e| e.to_string())?;
                sqlx::query(&sql_migration_table)
                    .execute(&pool)
                    .await
                    .map_err(|e| e.to_string())?;

                let mut current_version: i64 = sqlx::query_scalar(&sql_current_version)
                    .fetch_one(&pool)
                    .await
                    .map_err(|e| e.to_string())?;
                if current_version > POSTGRES_RUNTIME_SCHEMA_VERSION {
                    return Err(format!(
                        "postgres runtime schema version {} is newer than supported {}",
                        current_version, POSTGRES_RUNTIME_SCHEMA_VERSION
                    ));
                }

                if current_version < 1 {
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
                    sqlx::query(&sql_attempts)
                        .execute(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                    sqlx::query(&sql_leases)
                        .execute(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                    let now = dt_to_ms(Utc::now());
                    let sql_record = format!(
                        "INSERT INTO \"{}\".runtime_schema_migrations(version, name, applied_at_ms)
                         VALUES ($1, $2, $3)
                         ON CONFLICT(version) DO NOTHING",
                        schema
                    );
                    sqlx::query(&sql_record)
                        .bind(1_i32)
                        .bind("baseline_runtime_tables")
                        .bind(now)
                        .execute(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                    current_version = 1;
                }

                if current_version < 2 {
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
                    sqlx::query(&sql_attempt_idx)
                        .execute(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                    sqlx::query(&sql_lease_idx)
                        .execute(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                    let now = dt_to_ms(Utc::now());
                    let sql_record = format!(
                        "INSERT INTO \"{}\".runtime_schema_migrations(version, name, applied_at_ms)
                         VALUES ($1, $2, $3)
                         ON CONFLICT(version) DO NOTHING",
                        schema
                    );
                    sqlx::query(&sql_record)
                        .bind(2_i32)
                        .bind("runtime_indexes")
                        .bind(now)
                        .execute(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                }

                Ok(())
            })
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

    pub fn get_lease_by_id(&self, lease_id: &str) -> Result<Option<LeaseRecord>, KernelError> {
        self.ensure_schema()?;

        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let lease_id = lease_id.to_string();
        rt.block_on(async move {
            let sql = format!(
                "SELECT lease_id, attempt_id, worker_id, lease_expires_at_ms, heartbeat_at_ms, version
                 FROM \"{}\".runtime_leases
                 WHERE lease_id = $1",
                schema
            );
            let row = sqlx::query(&sql)
                .bind(&lease_id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| map_driver_err("get lease by id", e))?;
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

    pub fn heartbeat_lease_with_version(
        &self,
        lease_id: &str,
        worker_id: &str,
        expected_version: u64,
        heartbeat_at: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<(), KernelError> {
        self.ensure_schema()?;

        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let lease_id = lease_id.to_string();
        let worker_id = worker_id.to_string();
        let expected_version = expected_version as i64;
        let heartbeat_at_ms = dt_to_ms(heartbeat_at);
        let lease_expires_at_ms = dt_to_ms(lease_expires_at);
        rt.block_on(async move {
            let sql = format!(
                "UPDATE \"{}\".runtime_leases
                 SET heartbeat_at_ms = $4, lease_expires_at_ms = $5, version = version + 1
                 WHERE lease_id = $1 AND worker_id = $2 AND version = $3",
                schema
            );
            let updated = sqlx::query(&sql)
                .bind(&lease_id)
                .bind(&worker_id)
                .bind(expected_version)
                .bind(heartbeat_at_ms)
                .bind(lease_expires_at_ms)
                .execute(&pool)
                .await
                .map_err(|e| map_driver_err("heartbeat lease with version", e))?
                .rows_affected();
            if updated == 0 {
                return Err(KernelError::Driver(format!(
                    "lease heartbeat version conflict for lease: {}",
                    lease_id
                )));
            }
            Ok(())
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

            // Serialize lease ownership change for one attempt to avoid split-brain races.
            sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
                .bind(&attempt_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| map_driver_err("advisory lock attempt", e))?;

            let attempt_status_sql = format!(
                "SELECT status FROM \"{}\".runtime_attempts WHERE attempt_id = $1 FOR UPDATE",
                schema
            );
            let attempt_status: Option<String> = sqlx::query_scalar(&attempt_status_sql)
                .bind(&attempt_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| map_driver_err("read attempt status", e))?;
            let Some(status) = attempt_status else {
                return Err(KernelError::Driver(format!(
                    "attempt is not dispatchable for lease: {}",
                    attempt_id
                )));
            };
            if status != "queued" && status != "retry_backoff" {
                return Err(KernelError::Driver(format!(
                    "attempt is not dispatchable for lease: {}",
                    attempt_id
                )));
            }

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
                 WHERE attempt_id = $1",
                schema
            );
            sqlx::query(&update_sql)
                .bind(&attempt_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| map_driver_err("mark leased status", e))?;

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

    fn expire_leases_and_requeue(&self, stale_before: DateTime<Utc>) -> Result<u64, KernelError> {
        self.ensure_schema()?;

        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let stale_before_ms = dt_to_ms(stale_before);

        rt.block_on(async move {
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| map_driver_err("begin expire/requeue tx", e))?;

            // Delete first and use RETURNING as the authoritative expired-attempt set.
            let delete_sql = format!(
                "DELETE FROM \"{}\".runtime_leases
                 WHERE lease_expires_at_ms < $1
                 RETURNING attempt_id",
                schema
            );
            let deleted_rows = sqlx::query(&delete_sql)
                .bind(stale_before_ms)
                .fetch_all(&mut *tx)
                .await
                .map_err(|e| map_driver_err("delete expired leases", e))?;
            let attempt_ids: Vec<String> = deleted_rows.into_iter().map(|r| r.get(0)).collect();

            for attempt_id in &attempt_ids {
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
    use std::sync::Arc;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{Duration, Utc};
    use sqlx::postgres::PgPoolOptions;

    use super::{PostgresRuntimeRepository, POSTGRES_RUNTIME_SCHEMA_VERSION};
    use crate::kernel::runtime::{
        RuntimeRepository, SchedulerDecision, SkeletonScheduler, SqliteRuntimeRepository,
    };

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

    fn pg_query_i64(db_url: &str, query: String) -> i64 {
        let rt = tokio::runtime::Runtime::new().expect("create tokio runtime");
        rt.block_on(async move {
            let pool = PgPoolOptions::new()
                .max_connections(1)
                .connect(db_url)
                .await
                .expect("connect postgres");
            sqlx::query_scalar::<_, i64>(&query)
                .fetch_one(&pool)
                .await
                .expect("query postgres i64")
        })
    }

    fn pg_execute_batch(db_url: &str, statements: Vec<String>) {
        let rt = tokio::runtime::Runtime::new().expect("create tokio runtime");
        rt.block_on(async move {
            let pool = PgPoolOptions::new()
                .max_connections(1)
                .connect(db_url)
                .await
                .expect("connect postgres");
            for sql in statements {
                sqlx::query(&sql)
                    .execute(&pool)
                    .await
                    .expect("execute postgres statement");
            }
        });
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

    #[test]
    fn postgres_schema_migration_clean_init_reaches_latest_when_env_is_set() {
        let Some(db_url) = test_db_url() else {
            return;
        };
        let schema = test_schema();
        let repo = PostgresRuntimeRepository::new(db_url.clone()).with_schema(schema.clone());
        repo.enqueue_attempt("migration-clean-attempt", "migration-clean-run")
            .expect("enqueue attempt for schema init");

        let version = pg_query_i64(
            &db_url,
            format!(
                "SELECT COALESCE(MAX(version), 0)::BIGINT FROM \"{}\".runtime_schema_migrations",
                schema
            ),
        );
        assert_eq!(version, POSTGRES_RUNTIME_SCHEMA_VERSION);
    }

    #[test]
    fn postgres_schema_migration_incremental_upgrade_from_v1_when_env_is_set() {
        let Some(db_url) = test_db_url() else {
            return;
        };
        let schema = test_schema();

        pg_execute_batch(
            &db_url,
            vec![
                format!("CREATE SCHEMA IF NOT EXISTS \"{}\"", schema),
                format!(
                    "CREATE TABLE IF NOT EXISTS \"{}\".runtime_schema_migrations (
                        version INTEGER PRIMARY KEY,
                        name TEXT NOT NULL,
                        applied_at_ms BIGINT NOT NULL
                    )",
                    schema
                ),
                format!(
                    "CREATE TABLE IF NOT EXISTS \"{}\".runtime_attempts (
                        attempt_id TEXT PRIMARY KEY,
                        run_id TEXT NOT NULL,
                        attempt_no INTEGER NOT NULL,
                        status TEXT NOT NULL,
                        retry_at_ms BIGINT NULL
                    )",
                    schema
                ),
                format!(
                    "CREATE TABLE IF NOT EXISTS \"{}\".runtime_leases (
                        lease_id TEXT PRIMARY KEY,
                        attempt_id TEXT NOT NULL UNIQUE,
                        worker_id TEXT NOT NULL,
                        lease_expires_at_ms BIGINT NOT NULL,
                        heartbeat_at_ms BIGINT NOT NULL,
                        version BIGINT NOT NULL
                    )",
                    schema
                ),
                format!(
                    "INSERT INTO \"{}\".runtime_schema_migrations(version, name, applied_at_ms)
                     VALUES (1, 'baseline_runtime_tables', 1)
                     ON CONFLICT(version) DO NOTHING",
                    schema
                ),
            ],
        );

        let repo = PostgresRuntimeRepository::new(db_url.clone()).with_schema(schema.clone());
        repo.enqueue_attempt("migration-upgrade-attempt", "migration-upgrade-run")
            .expect("enqueue attempt for upgrade");

        let version = pg_query_i64(
            &db_url,
            format!(
                "SELECT COALESCE(MAX(version), 0)::BIGINT FROM \"{}\".runtime_schema_migrations",
                schema
            ),
        );
        assert_eq!(version, POSTGRES_RUNTIME_SCHEMA_VERSION);

        let attempts_idx_exists = pg_query_i64(
            &db_url,
            format!(
                "SELECT COUNT(*) FROM pg_indexes
                 WHERE schemaname = '{}'
                   AND tablename = 'runtime_attempts'
                   AND indexname = 'idx_runtime_attempts_status_retry'",
                schema
            ),
        );
        let leases_idx_exists = pg_query_i64(
            &db_url,
            format!(
                "SELECT COUNT(*) FROM pg_indexes
                 WHERE schemaname = '{}'
                   AND tablename = 'runtime_leases'
                   AND indexname = 'idx_runtime_leases_expiry'",
                schema
            ),
        );
        assert_eq!(attempts_idx_exists, 1);
        assert_eq!(leases_idx_exists, 1);
    }

    #[test]
    fn postgres_concurrent_upsert_lease_has_single_winner_when_env_is_set() {
        let Some(db_url) = test_db_url() else {
            return;
        };
        let repo = Arc::new(PostgresRuntimeRepository::new(db_url).with_schema(test_schema()));
        let run_id = "run-pg-concurrent";
        let attempt_id = "attempt-pg-concurrent";
        repo.enqueue_attempt(attempt_id, run_id)
            .expect("enqueue postgres attempt");

        let mut handles = Vec::new();
        for idx in 0..8 {
            let repo = repo.clone();
            let attempt_id = attempt_id.to_string();
            handles.push(thread::spawn(move || {
                repo.upsert_lease(
                    &attempt_id,
                    &format!("worker-{}", idx),
                    Utc::now() + Duration::seconds(30),
                )
            }));
        }

        let mut winners = Vec::new();
        let mut failures = 0;
        for handle in handles {
            match handle.join().expect("join worker thread") {
                Ok(lease) => winners.push(lease),
                Err(_) => failures += 1,
            }
        }
        assert_eq!(winners.len(), 1, "exactly one lease acquisition should win");
        assert_eq!(failures, 7, "all other concurrent acquisitions should fail");

        let active = repo
            .get_lease_for_attempt(attempt_id)
            .expect("get lease for attempt")
            .expect("active lease exists");
        assert_eq!(active.worker_id, winners[0].worker_id);
        assert_eq!(active.attempt_id, attempt_id);
    }

    #[test]
    fn postgres_heartbeat_with_version_enforces_ownership_when_env_is_set() {
        let Some(db_url) = test_db_url() else {
            return;
        };
        let repo = PostgresRuntimeRepository::new(db_url).with_schema(test_schema());
        let run_id = "run-pg-owner";
        let attempt_id = "attempt-pg-owner";
        let now = Utc::now();

        repo.enqueue_attempt(attempt_id, run_id)
            .expect("enqueue postgres attempt");
        let lease = repo
            .upsert_lease(attempt_id, "owner-worker", now + Duration::seconds(20))
            .expect("upsert lease");

        let wrong_owner = repo.heartbeat_lease_with_version(
            &lease.lease_id,
            "other-worker",
            lease.version,
            now + Duration::seconds(1),
            now + Duration::seconds(25),
        );
        assert!(
            wrong_owner.is_err(),
            "wrong worker must not heartbeat another lease"
        );

        let wrong_version = repo.heartbeat_lease_with_version(
            &lease.lease_id,
            "owner-worker",
            lease.version + 1,
            now + Duration::seconds(1),
            now + Duration::seconds(25),
        );
        assert!(
            wrong_version.is_err(),
            "stale/invalid version must be rejected"
        );

        repo.heartbeat_lease_with_version(
            &lease.lease_id,
            "owner-worker",
            lease.version,
            now + Duration::seconds(1),
            now + Duration::seconds(25),
        )
        .expect("owner heartbeat with matching version");

        let stale_after_update = repo.heartbeat_lease_with_version(
            &lease.lease_id,
            "owner-worker",
            lease.version,
            now + Duration::seconds(2),
            now + Duration::seconds(30),
        );
        assert!(
            stale_after_update.is_err(),
            "old version must not be reusable after version increments"
        );

        let latest = repo
            .get_lease_by_id(&lease.lease_id)
            .expect("get lease by id")
            .expect("lease exists");
        assert_eq!(latest.worker_id, "owner-worker");
        assert_eq!(latest.version, lease.version + 1);
    }

    #[test]
    fn postgres_scheduler_concurrent_dispatch_has_single_winner_when_env_is_set() {
        let Some(db_url) = test_db_url() else {
            return;
        };
        let repo = PostgresRuntimeRepository::new(db_url).with_schema(test_schema());
        repo.enqueue_attempt("attempt-pg-scheduler", "run-pg-scheduler")
            .expect("enqueue postgres attempt");

        let scheduler_a = SkeletonScheduler::new(repo.clone());
        let scheduler_b = SkeletonScheduler::new(repo.clone());

        let handle_a = thread::spawn(move || scheduler_a.dispatch_one("worker-a"));
        let handle_b = thread::spawn(move || scheduler_b.dispatch_one("worker-b"));

        let decision_a = handle_a
            .join()
            .expect("join scheduler a")
            .expect("decision a");
        let decision_b = handle_b
            .join()
            .expect("join scheduler b")
            .expect("decision b");

        let decisions = [decision_a, decision_b];
        let dispatched = decisions
            .iter()
            .filter(|d| matches!(d, SchedulerDecision::Dispatched { .. }))
            .count();
        let noops = decisions
            .iter()
            .filter(|d| matches!(d, SchedulerDecision::Noop))
            .count();

        assert_eq!(dispatched, 1, "only one scheduler dispatch should succeed");
        assert_eq!(noops, 1, "one scheduler should observe conflict and noop");
    }
}
