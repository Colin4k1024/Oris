//! Postgres-backed runtime repository for scheduler/lease contracts.
//!
//! This module is feature-gated behind `kernel-postgres`.

use std::sync::{Arc, OnceLock};

use chrono::{DateTime, TimeZone, Utc};
use sqlx::{postgres::PgPoolOptions, PgPool, Row};

use oris_kernel::event::KernelError;
use oris_kernel::identity::{RunId, Seq};

use super::models::{
    AttemptDispatchRecord, AttemptExecutionStatus, BountyRecord, BountyStatus, DisputeRecord,
    DisputeStatus, LeaseRecord, OrganismRecord, RecipeRecord, SessionMessageRecord,
    SessionRecord, SwarmTaskRecord, WorkerRecord,
};
use super::repository::RuntimeRepository;

const POSTGRES_RUNTIME_SCHEMA_VERSION: i64 = 4;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostgresBountyRow {
    pub bounty_id: String,
    pub title: String,
    pub description: Option<String>,
    pub reward: i64,
    pub status: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub accepted_by: Option<String>,
    pub accepted_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostgresSwarmTaskRow {
    pub parent_task_id: String,
    pub decomposition_json: String,
    pub proposer_id: String,
    pub proposer_reward_pct: i32,
    pub solver_reward_pct: i32,
    pub aggregator_reward_pct: i32,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostgresWorkerRegistryRow {
    pub worker_id: String,
    pub domains_json: String,
    pub max_load: i32,
    pub metadata_json: Option<String>,
    pub registered_at: DateTime<Utc>,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostgresRecipeRow {
    pub recipe_id: String,
    pub name: String,
    pub description: Option<String>,
    pub gene_sequence_json: String,
    pub author_id: String,
    pub forked_from: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub is_public: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostgresOrganismRow {
    pub organism_id: String,
    pub recipe_id: String,
    pub status: String,
    pub current_step: i32,
    pub total_steps: i32,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostgresDisputeRow {
    pub dispute_id: String,
    pub bounty_id: String,
    pub opened_by: String,
    pub status: String,
    pub resolution: Option<String>,
    pub resolved_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub evidence_json: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostgresA2aSessionRow {
    pub session_id: String,
    pub sender_id: String,
    pub protocol: String,
    pub protocol_version: String,
    pub enabled_capabilities_json: String,
    pub actor_type: Option<String>,
    pub actor_id: Option<String>,
    pub actor_role: Option<String>,
    pub negotiated_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
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

                // Migration v3: EvoMap Bounty, Swarm, Worker registry
                if current_version < 3 {
                    let sql_bounties = format!(
                        "CREATE TABLE IF NOT EXISTS \"{}\".runtime_bounties (
                            bounty_id TEXT PRIMARY KEY,
                            title TEXT NOT NULL,
                            description TEXT,
                            reward BIGINT NOT NULL,
                            status TEXT NOT NULL DEFAULT 'open',
                            created_by TEXT NOT NULL,
                            created_at_ms BIGINT NOT NULL,
                            closed_at_ms BIGINT NULL,
                            accepted_by TEXT NULL,
                            accepted_at_ms BIGINT NULL
                        )",
                        schema
                    );
                    let sql_swarm = format!(
                        "CREATE TABLE IF NOT EXISTS \"{}\".runtime_swarm_tasks (
                            parent_task_id TEXT PRIMARY KEY,
                            decomposition_json TEXT NOT NULL,
                            proposer_id TEXT NOT NULL,
                            proposer_reward_pct INTEGER NOT NULL DEFAULT 5,
                            solver_reward_pct INTEGER NOT NULL DEFAULT 85,
                            aggregator_reward_pct INTEGER NOT NULL DEFAULT 10,
                            status TEXT NOT NULL DEFAULT 'pending',
                            created_at_ms BIGINT NOT NULL,
                            completed_at_ms BIGINT NULL
                        )",
                        schema
                    );
                    let sql_workers = format!(
                        "CREATE TABLE IF NOT EXISTS \"{}\".runtime_workers_registry (
                            worker_id TEXT PRIMARY KEY,
                            domains TEXT NOT NULL,
                            max_load INTEGER NOT NULL DEFAULT 1,
                            metadata_json TEXT,
                            registered_at_ms BIGINT NOT NULL,
                            last_heartbeat_ms BIGINT NULL,
                            status TEXT NOT NULL DEFAULT 'active'
                        )",
                        schema
                    );

                    sqlx::query(&sql_bounties)
                        .execute(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                    sqlx::query(&sql_swarm)
                        .execute(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                    sqlx::query(&sql_workers)
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
                        .bind(3_i32)
                        .bind("runtime_bounties_swarm_worker")
                        .bind(now)
                        .execute(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                }

                // Migration v4: EvoMap Recipe, Organism, Session, Dispute
                if current_version < 4 {
                    let sql_recipes = format!(
                        "CREATE TABLE IF NOT EXISTS \"{}\".runtime_recipes (
                            recipe_id TEXT PRIMARY KEY,
                            name TEXT NOT NULL,
                            description TEXT,
                            gene_sequence_json TEXT NOT NULL,
                            author_id TEXT NOT NULL,
                            forked_from TEXT NULL,
                            created_at_ms BIGINT NOT NULL,
                            updated_at_ms BIGINT NOT NULL,
                            is_public INTEGER NOT NULL DEFAULT 0
                        )",
                        schema
                    );
                    let sql_organisms = format!(
                        "CREATE TABLE IF NOT EXISTS \"{}\".runtime_organisms (
                            organism_id TEXT PRIMARY KEY,
                            recipe_id TEXT NOT NULL,
                            status TEXT NOT NULL DEFAULT 'pending',
                            current_step INTEGER NOT NULL DEFAULT 0,
                            total_steps INTEGER NOT NULL,
                            created_at_ms BIGINT NOT NULL,
                            completed_at_ms BIGINT NULL
                        )",
                        schema
                    );
                    let sql_sessions = format!(
                        "CREATE TABLE IF NOT EXISTS \"{}\".runtime_collab_sessions (
                            session_id TEXT PRIMARY KEY,
                            session_type TEXT NOT NULL,
                            creator_id TEXT NOT NULL,
                            status TEXT NOT NULL DEFAULT 'active',
                            created_at_ms BIGINT NOT NULL,
                            ended_at_ms BIGINT NULL
                        )",
                        schema
                    );
                    let sql_messages = format!(
                        "CREATE TABLE IF NOT EXISTS \"{}\".runtime_collab_messages (
                            message_id TEXT PRIMARY KEY,
                            session_id TEXT NOT NULL,
                            sender_id TEXT NOT NULL,
                            content TEXT NOT NULL,
                            message_type TEXT NOT NULL DEFAULT 'message',
                            sent_at_ms BIGINT NOT NULL
                        )",
                        schema
                    );
                    let sql_disputes = format!(
                        "CREATE TABLE IF NOT EXISTS \"{}\".runtime_disputes (
                            dispute_id TEXT PRIMARY KEY,
                            bounty_id TEXT NOT NULL,
                            opened_by TEXT NOT NULL,
                            status TEXT NOT NULL DEFAULT 'open',
                            evidence_json TEXT,
                            resolution TEXT NULL,
                            resolved_by TEXT NULL,
                            resolved_at_ms BIGINT NULL,
                            created_at_ms BIGINT NOT NULL
                        )",
                        schema
                    );

                    sqlx::query(&sql_recipes)
                        .execute(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                    sqlx::query(&sql_organisms)
                        .execute(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                    sqlx::query(&sql_sessions)
                        .execute(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                    sqlx::query(&sql_messages)
                        .execute(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                    sqlx::query(&sql_disputes)
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
                        .bind(4_i32)
                        .bind("runtime_recipes_organisms_sessions_disputes")
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

    pub fn create_bounty(
        &self,
        bounty_id: &str,
        title: &str,
        description: Option<&str>,
        reward: i64,
        created_by: &str,
        created_at: DateTime<Utc>,
    ) -> Result<PostgresBountyRow, KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let bounty_id = bounty_id.to_string();
        let title = title.to_string();
        let description = description.map(|v| v.to_string());
        let created_by = created_by.to_string();
        let created_at_ms = dt_to_ms(created_at);
        let bounty_id_for_insert = bounty_id.clone();
        rt.block_on(async move {
            let sql = format!(
                "INSERT INTO \"{}\".runtime_bounties
                 (bounty_id, title, description, reward, status, created_by, created_at_ms, closed_at_ms, accepted_by, accepted_at_ms)
                 VALUES ($1, $2, $3, $4, 'open', $5, $6, NULL, NULL, NULL)",
                schema
            );
            sqlx::query(&sql)
                .bind(&bounty_id_for_insert)
                .bind(&title)
                .bind(description.as_deref())
                .bind(reward)
                .bind(&created_by)
                .bind(created_at_ms)
                .execute(&pool)
                .await
                .map_err(|e| map_driver_err("create bounty", e))?;
            Ok(())
        })?;
        self.get_bounty(&bounty_id)?
            .ok_or_else(|| map_driver_err("create bounty", "missing row after insert"))
    }

    pub fn get_bounty(&self, bounty_id: &str) -> Result<Option<PostgresBountyRow>, KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let bounty_id = bounty_id.to_string();
        rt.block_on(async move {
            let sql = format!(
                "SELECT bounty_id, title, description, reward, status, created_by, created_at_ms, closed_at_ms, accepted_by, accepted_at_ms
                 FROM \"{}\".runtime_bounties
                 WHERE bounty_id = $1",
                schema
            );
            let row = sqlx::query(&sql)
                .bind(&bounty_id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| map_driver_err("get bounty", e))?;
            Ok(row.map(|r| PostgresBountyRow {
                bounty_id: r.get::<String, _>(0),
                title: r.get::<String, _>(1),
                description: r.get::<Option<String>, _>(2),
                reward: r.get::<i64, _>(3),
                status: r.get::<String, _>(4),
                created_by: r.get::<String, _>(5),
                created_at: ms_to_dt(r.get::<i64, _>(6)),
                closed_at: r.get::<Option<i64>, _>(7).map(ms_to_dt),
                accepted_by: r.get::<Option<String>, _>(8),
                accepted_at: r.get::<Option<i64>, _>(9).map(ms_to_dt),
            }))
        })
    }

    pub fn accept_bounty(
        &self,
        bounty_id: &str,
        accepted_by: &str,
        accepted_at: DateTime<Utc>,
    ) -> Result<bool, KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let bounty_id = bounty_id.to_string();
        let accepted_by = accepted_by.to_string();
        let accepted_at_ms = dt_to_ms(accepted_at);
        rt.block_on(async move {
            let sql = format!(
                "UPDATE \"{}\".runtime_bounties
                 SET status = 'accepted',
                     accepted_by = $2,
                     accepted_at_ms = $3
                 WHERE bounty_id = $1
                   AND status = 'open'",
                schema
            );
            let affected = sqlx::query(&sql)
                .bind(&bounty_id)
                .bind(&accepted_by)
                .bind(accepted_at_ms)
                .execute(&pool)
                .await
                .map_err(|e| map_driver_err("accept bounty", e))?
                .rows_affected();
            Ok(affected > 0)
        })
    }

    pub fn close_bounty(
        &self,
        bounty_id: &str,
        closed_at: DateTime<Utc>,
    ) -> Result<bool, KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let bounty_id = bounty_id.to_string();
        let closed_at_ms = dt_to_ms(closed_at);
        rt.block_on(async move {
            let sql = format!(
                "UPDATE \"{}\".runtime_bounties
                 SET status = 'closed',
                     closed_at_ms = $2
                 WHERE bounty_id = $1
                   AND status = 'accepted'",
                schema
            );
            let affected = sqlx::query(&sql)
                .bind(&bounty_id)
                .bind(closed_at_ms)
                .execute(&pool)
                .await
                .map_err(|e| map_driver_err("close bounty", e))?
                .rows_affected();
            Ok(affected > 0)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_swarm_task(
        &self,
        parent_task_id: &str,
        decomposition_json: &str,
        proposer_id: &str,
        proposer_reward_pct: i32,
        solver_reward_pct: i32,
        aggregator_reward_pct: i32,
        status: &str,
        created_at: DateTime<Utc>,
    ) -> Result<PostgresSwarmTaskRow, KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let parent_task_id = parent_task_id.to_string();
        let decomposition_json = decomposition_json.to_string();
        let proposer_id = proposer_id.to_string();
        let status = status.to_string();
        let created_at_ms = dt_to_ms(created_at);
        let parent_task_id_for_insert = parent_task_id.clone();
        rt.block_on(async move {
            let sql = format!(
                "INSERT INTO \"{}\".runtime_swarm_tasks
                 (parent_task_id, decomposition_json, proposer_id, proposer_reward_pct, solver_reward_pct, aggregator_reward_pct, status, created_at_ms, completed_at_ms)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NULL)",
                schema
            );
            sqlx::query(&sql)
                .bind(&parent_task_id_for_insert)
                .bind(&decomposition_json)
                .bind(&proposer_id)
                .bind(proposer_reward_pct)
                .bind(solver_reward_pct)
                .bind(aggregator_reward_pct)
                .bind(&status)
                .bind(created_at_ms)
                .execute(&pool)
                .await
                .map_err(|e| map_driver_err("create swarm task", e))?;
            Ok(())
        })?;
        self.get_swarm_task(&parent_task_id)?
            .ok_or_else(|| map_driver_err("create swarm task", "missing row after insert"))
    }

    pub fn get_swarm_task(
        &self,
        parent_task_id: &str,
    ) -> Result<Option<PostgresSwarmTaskRow>, KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let parent_task_id = parent_task_id.to_string();
        rt.block_on(async move {
            let sql = format!(
                "SELECT parent_task_id, decomposition_json, proposer_id, proposer_reward_pct, solver_reward_pct, aggregator_reward_pct, status, created_at_ms, completed_at_ms
                 FROM \"{}\".runtime_swarm_tasks
                 WHERE parent_task_id = $1",
                schema
            );
            let row = sqlx::query(&sql)
                .bind(&parent_task_id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| map_driver_err("get swarm task", e))?;
            Ok(row.map(|r| PostgresSwarmTaskRow {
                parent_task_id: r.get::<String, _>(0),
                decomposition_json: r.get::<String, _>(1),
                proposer_id: r.get::<String, _>(2),
                proposer_reward_pct: r.get::<i32, _>(3),
                solver_reward_pct: r.get::<i32, _>(4),
                aggregator_reward_pct: r.get::<i32, _>(5),
                status: r.get::<String, _>(6),
                created_at: ms_to_dt(r.get::<i64, _>(7)),
                completed_at: r.get::<Option<i64>, _>(8).map(ms_to_dt),
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

    // ========== Worker Registration Methods ==========

    pub fn upsert_worker_registration(
        &self,
        worker_id: &str,
        domains_json: &str,
        max_load: i32,
        metadata_json: Option<&str>,
        status: &str,
        now: DateTime<Utc>,
    ) -> Result<PostgresWorkerRegistryRow, KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let worker_id = worker_id.to_string();
        let domains_json = domains_json.to_string();
        let metadata_json = metadata_json.map(String::from);
        let status = status.to_string();
        let now_ms = dt_to_ms(now);
        rt.block_on(async move {
            let sql = format!(
                "INSERT INTO \"{}\".runtime_workers_registry
                 (worker_id, domains_json, max_load, metadata_json, registered_at_ms, last_heartbeat_ms, status)
                 VALUES ($1, $2, $3, $4, $5, $5, $6)
                 ON CONFLICT(worker_id)
                 DO UPDATE SET
                   domains_json = excluded.domains_json,
                   max_load = excluded.max_load,
                   metadata_json = excluded.metadata_json,
                   last_heartbeat_ms = excluded.last_heartbeat_ms,
                   status = excluded.status",
                schema
            );
            sqlx::query(&sql)
                .bind(&worker_id)
                .bind(&domains_json)
                .bind(max_load)
                .bind(&metadata_json)
                .bind(now_ms)
                .bind(&status)
                .execute(&pool)
                .await
                .map_err(|e| map_driver_err("upsert worker registration", e))?;

            // Fetch the row
            let sql = format!(
                "SELECT worker_id, domains_json, max_load, metadata_json, registered_at_ms, last_heartbeat_ms, status
                 FROM \"{}\".runtime_workers_registry
                 WHERE worker_id = $1",
                schema
            );
            let row = sqlx::query(&sql)
                .bind(&worker_id)
                .fetch_one(&pool)
                .await
                .map_err(|e| map_driver_err("get worker registration", e))?;
            Ok(PostgresWorkerRegistryRow {
                worker_id: row.get(0),
                domains_json: row.get(1),
                max_load: row.get(2),
                metadata_json: row.get(3),
                registered_at: ms_to_dt(row.get::<i64, _>(4)),
                last_heartbeat_at: row.get::<Option<i64>, _>(5).map(|v| ms_to_dt(v)),
                status: row.get(6),
            })
        })
    }

    pub fn get_worker_registration(
        &self,
        worker_id: &str,
    ) -> Result<Option<PostgresWorkerRegistryRow>, KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let worker_id = worker_id.to_string();
        rt.block_on(async move {
            let sql = format!(
                "SELECT worker_id, domains_json, max_load, metadata_json, registered_at_ms, last_heartbeat_ms, status
                 FROM \"{}\".runtime_workers_registry
                 WHERE worker_id = $1",
                schema
            );
            let row = sqlx::query(&sql)
                .bind(&worker_id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| map_driver_err("get worker registration", e))?;
            Ok(row.map(|r| PostgresWorkerRegistryRow {
                worker_id: r.get(0),
                domains_json: r.get(1),
                max_load: r.get(2),
                metadata_json: r.get(3),
                registered_at: ms_to_dt(r.get(4)),
                last_heartbeat_at: r.get::<Option<i64>, _>(5).map(ms_to_dt),
                status: r.get(6),
            }))
        })
    }

    pub fn count_active_claims_for_worker(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
    ) -> Result<u64, KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let worker_id = worker_id.to_string();
        let now_ms = dt_to_ms(now);
        rt.block_on(async move {
            let sql = format!(
                "SELECT COUNT(*) FROM \"{}\".runtime_a2a_compat_tasks
                 WHERE claimed_by_sender_id = $1
                   AND lease_expires_at_ms IS NOT NULL
                   AND lease_expires_at_ms > $2",
                schema
            );
            let count: i64 = sqlx::query(&sql)
                .bind(&worker_id)
                .bind(now_ms)
                .fetch_one(&pool)
                .await
                .map_err(|e| map_driver_err("count active claims for worker", e))?
                .get(0);
            Ok(count.max(0) as u64)
        })
    }

    // ========== Dispute Methods ==========

    pub fn create_dispute(
        &self,
        dispute_id: &str,
        bounty_id: &str,
        opened_by: &str,
        description: &str,
        created_at: DateTime<Utc>,
    ) -> Result<PostgresDisputeRow, KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let dispute_id = dispute_id.to_string();
        let bounty_id = bounty_id.to_string();
        let opened_by = opened_by.to_string();
        let description = description.to_string();
        let created_at_ms = dt_to_ms(created_at);
        rt.block_on(async move {
            let sql = format!(
                "INSERT INTO \"{}\".runtime_disputes
                 (dispute_id, bounty_id, opened_by, status, resolution, resolved_by, created_at_ms, resolved_at_ms, evidence_json)
                 VALUES ($1, $2, $3, 'open', NULL, NULL, $4, NULL, NULL)",
                schema
            );
            sqlx::query(&sql)
                .bind(&dispute_id)
                .bind(&bounty_id)
                .bind(&opened_by)
                .bind(created_at_ms)
                .execute(&pool)
                .await
                .map_err(|e| map_driver_err("create dispute", e))?;
            Ok(PostgresDisputeRow {
                dispute_id,
                bounty_id,
                opened_by,
                status: "open".to_string(),
                resolution: None,
                resolved_by: None,
                created_at,
                resolved_at: None,
                evidence_json: None,
            })
        })
    }

    pub fn get_dispute(&self, dispute_id: &str) -> Result<Option<PostgresDisputeRow>, KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let dispute_id = dispute_id.to_string();
        rt.block_on(async move {
            let sql = format!(
                "SELECT dispute_id, bounty_id, opened_by, status, resolution, resolved_by, created_at_ms, resolved_at_ms, evidence_json
                 FROM \"{}\".runtime_disputes
                 WHERE dispute_id = $1",
                schema
            );
            let row = sqlx::query(&sql)
                .bind(&dispute_id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| map_driver_err("get dispute", e))?;
            Ok(row.map(|r| PostgresDisputeRow {
                dispute_id: r.get(0),
                bounty_id: r.get(1),
                opened_by: r.get(2),
                status: r.get(3),
                resolution: r.get(4),
                resolved_by: r.get(5),
                created_at: ms_to_dt(r.get(6)),
                resolved_at: r.get::<Option<i64>, _>(7).map(ms_to_dt),
                evidence_json: r.get(8),
            }))
        })
    }

    pub fn append_dispute_evidence(
        &self,
        dispute_id: &str,
        evidence_json: &str,
    ) -> Result<bool, KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let dispute_id = dispute_id.to_string();
        let evidence_json = evidence_json.to_string();
        rt.block_on(async move {
            let sql = format!(
                "UPDATE \"{}\".runtime_disputes
                 SET evidence_json = COALESCE(evidence_json || ', ', '') || $2
                 WHERE dispute_id = $1",
                schema
            );
            let affected = sqlx::query(&sql)
                .bind(&dispute_id)
                .bind(&evidence_json)
                .execute(&pool)
                .await
                .map_err(|e| map_driver_err("append dispute evidence", e))?
                .rows_affected();
            Ok(affected > 0)
        })
    }

    pub fn resolve_dispute(
        &self,
        dispute_id: &str,
        resolution: &str,
        resolved_by: &str,
        resolved_at: DateTime<Utc>,
    ) -> Result<bool, KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let dispute_id = dispute_id.to_string();
        let resolution = resolution.to_string();
        let resolved_by = resolved_by.to_string();
        let resolved_at_ms = dt_to_ms(resolved_at);
        rt.block_on(async move {
            let sql = format!(
                "UPDATE \"{}\".runtime_disputes
                 SET status = 'resolved', resolution = $2, resolved_by = $3, resolved_at_ms = $4
                 WHERE dispute_id = $1 AND status = 'open'",
                schema
            );
            let affected = sqlx::query(&sql)
                .bind(&dispute_id)
                .bind(&resolution)
                .bind(&resolved_by)
                .bind(resolved_at_ms)
                .execute(&pool)
                .await
                .map_err(|e| map_driver_err("resolve dispute", e))?
                .rows_affected();
            Ok(affected > 0)
        })
    }

    pub fn settle_bounty_via_dispute(
        &self,
        bounty_id: &str,
        settlement_status: &str,
        closed_at: DateTime<Utc>,
    ) -> Result<bool, KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let bounty_id = bounty_id.to_string();
        let settlement_status = settlement_status.to_string();
        let closed_at_ms = dt_to_ms(closed_at);
        rt.block_on(async move {
            let sql = format!(
                "UPDATE \"{}\".runtime_bounties
                 SET status = $2, closed_at_ms = $3
                 WHERE bounty_id = $1
                   AND status IN ('open', 'accepted')",
                schema
            );
            let affected = sqlx::query(&sql)
                .bind(&bounty_id)
                .bind(&settlement_status)
                .bind(closed_at_ms)
                .execute(&pool)
                .await
                .map_err(|e| map_driver_err("settle bounty via dispute", e))?
                .rows_affected();
            Ok(affected > 0)
        })
    }

    // ========== Recipe Methods ==========

    pub fn create_recipe(&self, recipe: &PostgresRecipeRow) -> Result<(), KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        rt.block_on(async move {
            let sql = format!(
                "INSERT INTO \"{}\".runtime_recipes
                 (recipe_id, name, description, gene_sequence_json, author_id, forked_from, created_at_ms, updated_at_ms, is_public)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
                schema
            );
            sqlx::query(&sql)
                .bind(&recipe.recipe_id)
                .bind(&recipe.name)
                .bind(&recipe.description)
                .bind(&recipe.gene_sequence_json)
                .bind(&recipe.author_id)
                .bind(&recipe.forked_from)
                .bind(dt_to_ms(recipe.created_at))
                .bind(dt_to_ms(recipe.updated_at))
                .bind(recipe.is_public as i32)
                .execute(&pool)
                .await
                .map_err(|e| map_driver_err("create recipe", e))?;
            Ok(())
        })
    }

    pub fn get_recipe(&self, recipe_id: &str) -> Result<Option<PostgresRecipeRow>, KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let recipe_id = recipe_id.to_string();
        rt.block_on(async move {
            let sql = format!(
                "SELECT recipe_id, name, description, gene_sequence_json, author_id, forked_from, created_at_ms, updated_at_ms, is_public
                 FROM \"{}\".runtime_recipes
                 WHERE recipe_id = $1",
                schema
            );
            let row = sqlx::query(&sql)
                .bind(&recipe_id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| map_driver_err("get recipe", e))?;
            Ok(row.map(|r| PostgresRecipeRow {
                recipe_id: r.get(0),
                name: r.get(1),
                description: r.get(2),
                gene_sequence_json: r.get(3),
                author_id: r.get(4),
                forked_from: r.get(5),
                created_at: ms_to_dt(r.get(6)),
                updated_at: ms_to_dt(r.get(7)),
                is_public: r.get::<i32, _>(8) != 0,
            }))
        })
    }

    // ========== Organism Methods ==========

    pub fn create_organism(&self, organism: &PostgresOrganismRow) -> Result<(), KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        rt.block_on(async move {
            let sql = format!(
                "INSERT INTO \"{}\".runtime_organisms
                 (organism_id, recipe_id, status, current_step, total_steps, created_at_ms, completed_at_ms)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                schema
            );
            sqlx::query(&sql)
                .bind(&organism.organism_id)
                .bind(&organism.recipe_id)
                .bind(&organism.status)
                .bind(organism.current_step)
                .bind(organism.total_steps)
                .bind(dt_to_ms(organism.created_at))
                .bind(organism.completed_at.map(dt_to_ms))
                .execute(&pool)
                .await
                .map_err(|e| map_driver_err("create organism", e))?;
            Ok(())
        })
    }

    pub fn get_organism(
        &self,
        organism_id: &str,
    ) -> Result<Option<PostgresOrganismRow>, KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let organism_id = organism_id.to_string();
        rt.block_on(async move {
            let sql = format!(
                "SELECT organism_id, recipe_id, status, current_step, total_steps, created_at_ms, completed_at_ms
                 FROM \"{}\".runtime_organisms
                 WHERE organism_id = $1",
                schema
            );
            let row = sqlx::query(&sql)
                .bind(&organism_id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| map_driver_err("get organism", e))?;
            Ok(row.map(|r| PostgresOrganismRow {
                organism_id: r.get(0),
                recipe_id: r.get(1),
                status: r.get(2),
                current_step: r.get(3),
                total_steps: r.get(4),
                created_at: ms_to_dt(r.get(5)),
                completed_at: r.get::<Option<i64>, _>(6).map(ms_to_dt),
            }))
        })
    }

    pub fn update_organism_status(
        &self,
        organism_id: &str,
        status: &str,
        current_step: i32,
    ) -> Result<bool, KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let organism_id = organism_id.to_string();
        let status = status.to_string();
        let completed_at_ms: Option<i64> = if status == "completed" {
            Some(dt_to_ms(Utc::now()))
        } else {
            None
        };
        rt.block_on(async move {
            let sql = format!(
                "UPDATE \"{}\".runtime_organisms
                 SET status = $2, current_step = $3, completed_at_ms = $4
                 WHERE organism_id = $1",
                schema
            );
            let affected = sqlx::query(&sql)
                .bind(&organism_id)
                .bind(&status)
                .bind(current_step)
                .bind(completed_at_ms)
                .execute(&pool)
                .await
                .map_err(|e| map_driver_err("update organism status", e))?
                .rows_affected();
            Ok(affected > 0)
        })
    }

    // ========== Session Methods ==========

    pub fn upsert_a2a_session(&self, session: &PostgresA2aSessionRow) -> Result<(), KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        rt.block_on(async move {
            let sql = format!(
                "INSERT INTO \"{}\".runtime_a2a_sessions
                 (session_id, sender_id, protocol, protocol_version, enabled_capabilities_json, actor_type, actor_id, actor_role, negotiated_at, expires_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                 ON CONFLICT(session_id)
                 DO UPDATE SET
                   protocol = excluded.protocol,
                   protocol_version = excluded.protocol_version,
                   enabled_capabilities_json = excluded.enabled_capabilities_json,
                   actor_type = excluded.actor_type,
                   actor_id = excluded.actor_id,
                   actor_role = excluded.actor_role,
                   expires_at = excluded.expires_at,
                   updated_at = excluded.updated_at",
                schema
            );
            sqlx::query(&sql)
                .bind(&session.session_id)
                .bind(&session.sender_id)
                .bind(&session.protocol)
                .bind(&session.protocol_version)
                .bind(&session.enabled_capabilities_json)
                .bind(&session.actor_type)
                .bind(&session.actor_id)
                .bind(&session.actor_role)
                .bind(dt_to_ms(session.negotiated_at))
                .bind(session.expires_at.map(dt_to_ms))
                .bind(dt_to_ms(session.updated_at))
                .execute(&pool)
                .await
                .map_err(|e| map_driver_err("upsert a2a session", e))?;
            Ok(())
        })
    }

    pub fn get_active_a2a_session(
        &self,
        sender_id: &str,
    ) -> Result<Option<PostgresA2aSessionRow>, KernelError> {
        self.ensure_schema()?;
        let pool = self.pool()?.clone();
        let rt = self.runtime()?;
        let schema = self.schema.clone();
        let sender_id = sender_id.to_string();
        let now_ms = dt_to_ms(Utc::now());
        rt.block_on(async move {
            let sql = format!(
                "SELECT session_id, sender_id, protocol, protocol_version, enabled_capabilities_json, actor_type, actor_id, actor_role, negotiated_at, expires_at, updated_at
                 FROM \"{}\".runtime_a2a_sessions
                 WHERE sender_id = $1 AND (expires_at IS NULL OR expires_at > $2)",
                schema
            );
            let row = sqlx::query(&sql)
                .bind(&sender_id)
                .bind(now_ms)
                .fetch_optional(&pool)
                .await
                .map_err(|e| map_driver_err("get active a2a session", e))?;
            Ok(row.map(|r| PostgresA2aSessionRow {
                session_id: r.get(0),
                sender_id: r.get(1),
                protocol: r.get(2),
                protocol_version: r.get(3),
                enabled_capabilities_json: r.get(4),
                actor_type: r.get(5),
                actor_id: r.get(6),
                actor_role: r.get(7),
                negotiated_at: ms_to_dt(r.get(8)),
                expires_at: r.get::<Option<i64>, _>(9).map(ms_to_dt),
                updated_at: ms_to_dt(r.get(10)),
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

    // ============== Bounty Methods ==============

    fn upsert_bounty(&self, _bounty: &BountyRecord) -> Result<(), KernelError> {
        Ok(()) // TODO: implement
    }

    fn get_bounty(&self, _bounty_id: &str) -> Result<Option<BountyRecord>, KernelError> {
        Ok(None) // TODO: implement
    }

    fn list_bounties(&self, _status: Option<&str>, _limit: usize) -> Result<Vec<BountyRecord>, KernelError> {
        Ok(vec![]) // TODO: implement
    }

    fn accept_bounty(&self, _bounty_id: &str, _accepted_by: &str) -> Result<(), KernelError> {
        Ok(()) // TODO: implement
    }

    fn close_bounty(&self, _bounty_id: &str) -> Result<(), KernelError> {
        Ok(()) // TODO: implement
    }

    // ============== Swarm Methods ==============

    fn upsert_swarm_decomposition(&self, _task: &SwarmTaskRecord) -> Result<(), KernelError> {
        Ok(()) // TODO: implement
    }

    fn get_swarm_decomposition(&self, _parent_task_id: &str) -> Result<Option<SwarmTaskRecord>, KernelError> {
        Ok(None) // TODO: implement
    }

    // ============== Worker Methods ==============

    fn register_worker(&self, _worker: &WorkerRecord) -> Result<(), KernelError> {
        Ok(()) // TODO: implement
    }

    fn get_worker(&self, _worker_id: &str) -> Result<Option<WorkerRecord>, KernelError> {
        Ok(None) // TODO: implement
    }

    fn list_workers(&self, _domain: Option<&str>, _status: Option<&str>, _limit: usize) -> Result<Vec<WorkerRecord>, KernelError> {
        Ok(vec![]) // TODO: implement
    }

    fn heartbeat_worker(&self, _worker_id: &str, _heartbeat_at_ms: i64) -> Result<(), KernelError> {
        Ok(()) // TODO: implement
    }

    // ============== Recipe Methods ==============

    fn create_recipe(&self, _recipe: &RecipeRecord) -> Result<(), KernelError> {
        Ok(()) // TODO: implement
    }

    fn get_recipe(&self, _recipe_id: &str) -> Result<Option<RecipeRecord>, KernelError> {
        Ok(None) // TODO: implement
    }

    fn fork_recipe(&self, _original_id: &str, _new_id: &str, _new_author: &str) -> Result<Option<RecipeRecord>, KernelError> {
        Ok(None) // TODO: implement
    }

    fn list_recipes(&self, _author_id: Option<&str>, _limit: usize) -> Result<Vec<RecipeRecord>, KernelError> {
        Ok(vec![]) // TODO: implement
    }

    // ============== Organism Methods ==============

    fn express_organism(&self, _organism: &OrganismRecord) -> Result<(), KernelError> {
        Ok(()) // TODO: implement
    }

    fn get_organism(&self, _organism_id: &str) -> Result<Option<OrganismRecord>, KernelError> {
        Ok(None) // TODO: implement
    }

    fn update_organism(&self, _organism_id: &str, _current_step: i32, _status: &str) -> Result<(), KernelError> {
        Ok(()) // TODO: implement
    }

    // ============== Session Methods ==============

    fn create_session(&self, _session: &SessionRecord) -> Result<(), KernelError> {
        Ok(()) // TODO: implement
    }

    fn get_session(&self, _session_id: &str) -> Result<Option<SessionRecord>, KernelError> {
        Ok(None) // TODO: implement
    }

    fn add_session_message(&self, _message: &SessionMessageRecord) -> Result<(), KernelError> {
        Ok(()) // TODO: implement
    }

    fn get_session_history(&self, _session_id: &str, _limit: usize) -> Result<Vec<SessionMessageRecord>, KernelError> {
        Ok(vec![]) // TODO: implement
    }

    // ============== Dispute Methods ==============

    fn open_dispute(&self, _dispute: &DisputeRecord) -> Result<(), KernelError> {
        Ok(()) // TODO: implement
    }

    fn get_dispute(&self, _dispute_id: &str) -> Result<Option<DisputeRecord>, KernelError> {
        Ok(None) // TODO: implement
    }

    fn get_disputes_for_bounty(&self, _bounty_id: &str) -> Result<Vec<DisputeRecord>, KernelError> {
        Ok(vec![]) // TODO: implement
    }

    fn resolve_dispute(&self, _dispute_id: &str, _resolution: &str, _resolved_by: &str) -> Result<(), KernelError> {
        Ok(()) // TODO: implement
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
    use crate::{RuntimeRepository, SchedulerDecision, SkeletonScheduler, SqliteRuntimeRepository};

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

    #[test]
    fn postgres_bounty_lifecycle_roundtrip_when_env_is_set() {
        let Some(db_url) = test_db_url() else {
            return;
        };
        let repo = PostgresRuntimeRepository::new(db_url).with_schema(test_schema());
        let now = Utc::now();
        let created = repo
            .create_bounty(
                "pg-bounty-1",
                "Implement feature X",
                Some("details"),
                200,
                "alice",
                now,
            )
            .expect("create bounty");
        assert_eq!(created.status, "open");

        let accepted = repo
            .accept_bounty("pg-bounty-1", "worker-1", now + Duration::seconds(1))
            .expect("accept bounty");
        assert!(accepted);

        let closed = repo
            .close_bounty("pg-bounty-1", now + Duration::seconds(2))
            .expect("close bounty");
        assert!(closed);

        let final_row = repo
            .get_bounty("pg-bounty-1")
            .expect("get bounty")
            .expect("bounty exists");
        assert_eq!(final_row.status, "closed");
        assert_eq!(final_row.accepted_by.as_deref(), Some("worker-1"));
        assert!(final_row.closed_at.is_some());
    }

    #[test]
    fn postgres_bounty_invalid_transitions_when_env_is_set() {
        let Some(db_url) = test_db_url() else {
            return;
        };
        let repo = PostgresRuntimeRepository::new(db_url).with_schema(test_schema());
        let now = Utc::now();
        repo.create_bounty(
            "pg-bounty-2",
            "Implement feature Y",
            None,
            100,
            "alice",
            now,
        )
        .expect("create bounty");

        let close_open = repo
            .close_bounty("pg-bounty-2", now + Duration::seconds(1))
            .expect("close open bounty");
        assert!(!close_open);

        let accept_once = repo
            .accept_bounty("pg-bounty-2", "worker-2", now + Duration::seconds(2))
            .expect("accept open bounty");
        assert!(accept_once);

        let accept_again = repo
            .accept_bounty("pg-bounty-2", "worker-3", now + Duration::seconds(3))
            .expect("accept accepted bounty");
        assert!(!accept_again);
    }

    #[test]
    fn postgres_swarm_task_roundtrip_when_env_is_set() {
        let Some(db_url) = test_db_url() else {
            return;
        };
        let repo = PostgresRuntimeRepository::new(db_url).with_schema(test_schema());
        let now = Utc::now();
        let decomposition_json =
            r#"{"child_tasks":[{"task_id":"c1","description":"d1","role":"solver"}]}"#;

        let created = repo
            .create_swarm_task(
                "pg-parent-1",
                decomposition_json,
                "alice",
                5,
                85,
                10,
                "pending",
                now,
            )
            .expect("create swarm task");
        assert_eq!(created.parent_task_id, "pg-parent-1");
        assert_eq!(created.proposer_reward_pct, 5);
        assert_eq!(created.solver_reward_pct, 85);
        assert_eq!(created.aggregator_reward_pct, 10);

        let fetched = repo
            .get_swarm_task("pg-parent-1")
            .expect("get swarm task")
            .expect("swarm task exists");
        assert_eq!(fetched.decomposition_json, decomposition_json);
        assert_eq!(fetched.proposer_id, "alice");
        assert_eq!(fetched.status, "pending");
    }
}
