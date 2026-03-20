//! SQLite-backed runtime repository for Phase 3 worker APIs.

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, TimeZone, Utc};
use rusqlite::{params, Connection, ErrorCode, OptionalExtension};

use oris_kernel::event::KernelError;
use oris_kernel::identity::{RunId, Seq};

use super::models::{
    AttemptDispatchRecord, AttemptExecutionStatus, BountyRecord, BountyStatus, DisputeRecord,
    DisputeStatus, LeaseRecord, OrganismRecord, RecipeRecord, SessionMessageRecord, SessionRecord,
    SwarmTaskRecord, WorkerRecord,
};
use super::repository::RuntimeRepository;

const SQLITE_RUNTIME_SCHEMA_VERSION: i64 = 13;

#[derive(Clone)]
pub struct SqliteRuntimeRepository {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryStrategy {
    Fixed,
    Exponential,
}

impl RetryStrategy {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Exponential => "exponential",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "fixed" => Some(Self::Fixed),
            "exponential" => Some(Self::Exponential),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RetryPolicyConfig {
    pub strategy: RetryStrategy,
    pub backoff_ms: i64,
    pub max_backoff_ms: Option<i64>,
    pub multiplier: Option<f64>,
    pub max_retries: u32,
}

impl RetryPolicyConfig {
    fn next_backoff_ms(&self, current_attempt_no: u32) -> i64 {
        let current_attempt_no = current_attempt_no.max(1);
        let raw_backoff = match self.strategy {
            RetryStrategy::Fixed => self.backoff_ms,
            RetryStrategy::Exponential => {
                let multiplier = self.multiplier.unwrap_or(2.0);
                let exponent = (current_attempt_no - 1) as i32;
                ((self.backoff_ms as f64) * multiplier.powi(exponent)).round() as i64
            }
        };
        if let Some(max_backoff_ms) = self.max_backoff_ms {
            raw_backoff.min(max_backoff_ms)
        } else {
            raw_backoff
        }
    }
}

#[derive(Clone, Debug)]
pub struct AttemptAckOutcome {
    pub status: AttemptExecutionStatus,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub next_attempt_no: u32,
}

#[derive(Clone, Debug)]
pub struct AttemptRetryHistoryRow {
    pub attempt_no: u32,
    pub strategy: String,
    pub backoff_ms: i64,
    pub max_retries: u32,
    pub scheduled_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct AttemptRetryHistorySnapshot {
    pub attempt_id: String,
    pub current_attempt_no: u32,
    pub current_status: AttemptExecutionStatus,
    pub history: Vec<AttemptRetryHistoryRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeoutPolicyConfig {
    pub timeout_ms: i64,
    pub on_timeout_status: AttemptExecutionStatus,
}

#[derive(Clone, Debug)]
pub struct DeadLetterRow {
    pub attempt_id: String,
    pub run_id: String,
    pub attempt_no: u32,
    pub terminal_status: String,
    pub reason: Option<String>,
    pub dead_at: DateTime<Utc>,
    pub replay_status: String,
    pub replay_count: u32,
    pub last_replayed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayEffectClaim {
    Acquired,
    InProgress,
    Completed(String),
}

#[derive(Clone, Debug)]
pub struct ReplayEffectLogRow {
    pub fingerprint: String,
    pub thread_id: String,
    pub replay_target: String,
    pub effect_type: String,
    pub status: String,
    pub execution_count: u32,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug)]
pub struct DispatchableAttemptContext {
    pub attempt_id: String,
    pub tenant_id: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptTraceContextRow {
    pub trace_id: String,
    pub parent_span_id: Option<String>,
    pub span_id: String,
    pub trace_flags: String,
}

impl SqliteRuntimeRepository {
    pub fn new(db_path: &str) -> Result<Self, KernelError> {
        let conn = Connection::open(db_path)
            .map_err(|e| KernelError::Driver(format!("open sqlite runtime repo: {}", e)))?;
        let repo = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        repo.ensure_schema()?;
        Ok(repo)
    }

    fn ensure_schema(&self) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        ensure_sqlite_migration_table(&conn)?;
        let current = sqlite_current_schema_version(&conn)?;
        if current > SQLITE_RUNTIME_SCHEMA_VERSION {
            return Err(KernelError::Driver(format!(
                "sqlite runtime schema version {} is newer than supported {}",
                current, SQLITE_RUNTIME_SCHEMA_VERSION
            )));
        }
        if current < 1 {
            apply_sqlite_runtime_migration_v1(&conn)?;
            record_sqlite_migration(&conn, 1, "baseline_runtime_tables")?;
        }
        if current < 2 {
            apply_sqlite_runtime_migration_v2(&conn)?;
            record_sqlite_migration(&conn, 2, "interrupt_resume_and_api_key_role")?;
        }
        if current < 3 {
            apply_sqlite_runtime_migration_v3(&conn)?;
            record_sqlite_migration(&conn, 3, "attempt_retry_policy_and_history")?;
        }
        if current < 4 {
            apply_sqlite_runtime_migration_v4(&conn)?;
            record_sqlite_migration(&conn, 4, "attempt_execution_timeout_policy")?;
        }
        if current < 5 {
            apply_sqlite_runtime_migration_v5(&conn)?;
            record_sqlite_migration(&conn, 5, "runtime_dead_letter_queue")?;
        }
        if current < 6 {
            apply_sqlite_runtime_migration_v6(&conn)?;
            record_sqlite_migration(&conn, 6, "attempt_priority_dispatch_order")?;
        }
        if current < 7 {
            apply_sqlite_runtime_migration_v7(&conn)?;
            record_sqlite_migration(&conn, 7, "attempt_tenant_rate_limits")?;
        }
        if current < 8 {
            apply_sqlite_runtime_migration_v8(&conn)?;
            record_sqlite_migration(&conn, 8, "attempt_trace_context")?;
        }
        if current < 9 {
            apply_sqlite_runtime_migration_v9(&conn)?;
            record_sqlite_migration(&conn, 9, "replay_effect_guard")?;
        }
        if current < 10 {
            apply_sqlite_runtime_migration_v10(&conn)?;
            record_sqlite_migration(&conn, 10, "runtime_a2a_sessions")?;
        }
        if current < 11 {
            apply_sqlite_runtime_migration_v11(&conn)?;
            record_sqlite_migration(&conn, 11, "runtime_a2a_compat_tasks")?;
        }
        // EvoMap Alignment: Bounty, Swarm, Worker registry
        if current < 12 {
            apply_sqlite_runtime_migration_v12(&conn)?;
            record_sqlite_migration(&conn, 12, "runtime_bounties_swarm_worker")?;
        }
        // EvoMap Alignment: Recipe, Organism, Session, Dispute
        if current < 13 {
            apply_sqlite_runtime_migration_v13(&conn)?;
            record_sqlite_migration(&conn, 13, "runtime_recipes_organisms_sessions_disputes")?;
        }
        Ok(())
    }

    pub fn enqueue_attempt(&self, attempt_id: &str, run_id: &str) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT OR IGNORE INTO runtime_attempts (attempt_id, run_id, attempt_no, status, retry_at_ms)
             VALUES (?1, ?2, 1, 'queued', NULL)",
            params![attempt_id, run_id],
        )
        .map_err(|e| KernelError::Driver(format!("enqueue attempt: {}", e)))?;
        Ok(())
    }

    pub fn set_attempt_timeout_policy(
        &self,
        attempt_id: &str,
        policy: &TimeoutPolicyConfig,
    ) -> Result<(), KernelError> {
        if policy.timeout_ms <= 0 {
            return Err(KernelError::Driver(
                "timeout policy timeout_ms must be > 0".to_string(),
            ));
        }
        if !matches!(
            policy.on_timeout_status,
            AttemptExecutionStatus::Failed | AttemptExecutionStatus::Cancelled
        ) {
            return Err(KernelError::Driver(
                "timeout policy terminal status must be failed or cancelled".to_string(),
            ));
        }
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let updated = conn
            .execute(
                "UPDATE runtime_attempts
                 SET execution_timeout_ms = ?2,
                     timeout_terminal_status = ?3
                 WHERE attempt_id = ?1",
                params![
                    attempt_id,
                    policy.timeout_ms,
                    attempt_status_to_str(&policy.on_timeout_status)
                ],
            )
            .map_err(|e| KernelError::Driver(format!("set attempt timeout policy: {}", e)))?;
        if updated == 0 {
            return Err(KernelError::Driver(format!(
                "attempt not found for timeout policy: {}",
                attempt_id
            )));
        }
        Ok(())
    }

    pub fn set_attempt_priority(&self, attempt_id: &str, priority: i32) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let updated = conn
            .execute(
                "UPDATE runtime_attempts SET priority = ?2 WHERE attempt_id = ?1",
                params![attempt_id, priority],
            )
            .map_err(|e| KernelError::Driver(format!("set attempt priority: {}", e)))?;
        if updated == 0 {
            return Err(KernelError::Driver(format!(
                "attempt not found for priority update: {}",
                attempt_id
            )));
        }
        Ok(())
    }

    pub fn set_attempt_tenant_id(
        &self,
        attempt_id: &str,
        tenant_id: Option<&str>,
    ) -> Result<(), KernelError> {
        let normalized = tenant_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let updated = conn
            .execute(
                "UPDATE runtime_attempts SET tenant_id = ?2 WHERE attempt_id = ?1",
                params![attempt_id, normalized],
            )
            .map_err(|e| KernelError::Driver(format!("set attempt tenant_id: {}", e)))?;
        if updated == 0 {
            return Err(KernelError::Driver(format!(
                "attempt not found for tenant update: {}",
                attempt_id
            )));
        }
        Ok(())
    }

    pub fn get_attempt_status(
        &self,
        attempt_id: &str,
    ) -> Result<Option<(u32, AttemptExecutionStatus)>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.query_row(
            "SELECT attempt_no, status FROM runtime_attempts WHERE attempt_id = ?1",
            params![attempt_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? as u32,
                    parse_attempt_status(&row.get::<_, String>(1)?),
                ))
            },
        )
        .optional()
        .map_err(|e| KernelError::Driver(format!("get attempt status: {}", e)))
    }

    pub fn set_attempt_trace_context(
        &self,
        attempt_id: &str,
        trace_id: &str,
        parent_span_id: Option<&str>,
        span_id: &str,
        trace_flags: &str,
    ) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let updated = conn
            .execute(
                "UPDATE runtime_attempts
                 SET trace_id = ?2,
                     trace_parent_span_id = ?3,
                     trace_span_id = ?4,
                     trace_flags = ?5
                 WHERE attempt_id = ?1",
                params![attempt_id, trace_id, parent_span_id, span_id, trace_flags],
            )
            .map_err(|e| KernelError::Driver(format!("set attempt trace context: {}", e)))?;
        if updated == 0 {
            return Err(KernelError::Driver(format!(
                "attempt not found for trace update: {}",
                attempt_id
            )));
        }
        Ok(())
    }

    pub fn get_attempt_trace_context(
        &self,
        attempt_id: &str,
    ) -> Result<Option<AttemptTraceContextRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.query_row(
            "SELECT trace_id, trace_parent_span_id, trace_span_id, COALESCE(trace_flags, '01')
             FROM runtime_attempts
             WHERE attempt_id = ?1
               AND trace_id IS NOT NULL
               AND trace_span_id IS NOT NULL",
            params![attempt_id],
            |row| {
                Ok(AttemptTraceContextRow {
                    trace_id: row.get(0)?,
                    parent_span_id: row.get(1)?,
                    span_id: row.get(2)?,
                    trace_flags: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(|e| KernelError::Driver(format!("get attempt trace context: {}", e)))
    }

    pub fn latest_attempt_trace_for_run(
        &self,
        run_id: &str,
    ) -> Result<Option<AttemptTraceContextRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.query_row(
            "SELECT trace_id, trace_parent_span_id, trace_span_id, COALESCE(trace_flags, '01')
             FROM runtime_attempts
             WHERE run_id = ?1
               AND trace_id IS NOT NULL
               AND trace_span_id IS NOT NULL
             ORDER BY attempt_no DESC, attempt_id DESC
             LIMIT 1",
            params![run_id],
            |row| {
                Ok(AttemptTraceContextRow {
                    trace_id: row.get(0)?,
                    parent_span_id: row.get(1)?,
                    span_id: row.get(2)?,
                    trace_flags: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(|e| KernelError::Driver(format!("latest attempt trace for run: {}", e)))
    }

    pub fn latest_attempt_id_for_run(&self, run_id: &str) -> Result<Option<String>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.query_row(
            "SELECT attempt_id
             FROM runtime_attempts
             WHERE run_id = ?1
             ORDER BY attempt_no DESC, attempt_id DESC
             LIMIT 1",
            params![run_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| KernelError::Driver(format!("latest attempt id for run: {}", e)))
    }

    pub fn advance_attempt_trace(
        &self,
        attempt_id: &str,
        next_span_id: &str,
    ) -> Result<Option<AttemptTraceContextRow>, KernelError> {
        let Some(current) = self.get_attempt_trace_context(attempt_id)? else {
            return Ok(None);
        };
        self.set_attempt_trace_context(
            attempt_id,
            &current.trace_id,
            Some(&current.span_id),
            next_span_id,
            &current.trace_flags,
        )?;
        Ok(Some(AttemptTraceContextRow {
            trace_id: current.trace_id,
            parent_span_id: Some(current.span_id),
            span_id: next_span_id.to_string(),
            trace_flags: current.trace_flags,
        }))
    }

    pub fn set_attempt_started_at_for_test(
        &self,
        attempt_id: &str,
        started_at: Option<DateTime<Utc>>,
    ) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let updated = conn
            .execute(
                "UPDATE runtime_attempts SET started_at_ms = ?2 WHERE attempt_id = ?1",
                params![attempt_id, started_at.map(dt_to_ms)],
            )
            .map_err(|e| KernelError::Driver(format!("set attempt started_at: {}", e)))?;
        if updated == 0 {
            return Err(KernelError::Driver(format!(
                "attempt not found for started_at update: {}",
                attempt_id
            )));
        }
        Ok(())
    }

    pub fn get_lease_for_attempt(
        &self,
        attempt_id: &str,
    ) -> Result<Option<LeaseRecord>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT lease_id, attempt_id, worker_id, lease_expires_at_ms, heartbeat_at_ms, version
                 FROM runtime_leases WHERE attempt_id = ?1",
            )
            .map_err(|e| KernelError::Driver(format!("prepare get lease by attempt: {}", e)))?;
        let mut rows = stmt
            .query(params![attempt_id])
            .map_err(|e| KernelError::Driver(format!("query get lease by attempt: {}", e)))?;
        if let Some(row) = rows
            .next()
            .map_err(|e| KernelError::Driver(format!("scan get lease by attempt: {}", e)))?
        {
            Ok(Some(LeaseRecord {
                lease_id: row.get(0).map_err(map_rusqlite_err)?,
                attempt_id: row.get(1).map_err(map_rusqlite_err)?,
                worker_id: row.get(2).map_err(map_rusqlite_err)?,
                lease_expires_at: ms_to_dt(row.get::<_, i64>(3).map_err(map_rusqlite_err)?),
                heartbeat_at: ms_to_dt(row.get::<_, i64>(4).map_err(map_rusqlite_err)?),
                version: row.get::<_, i64>(5).map_err(map_rusqlite_err)? as u64,
                terminal_state: None,
                terminal_at: None,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn get_lease_by_id(&self, lease_id: &str) -> Result<Option<LeaseRecord>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT lease_id, attempt_id, worker_id, lease_expires_at_ms, heartbeat_at_ms, version
                 FROM runtime_leases WHERE lease_id = ?1",
            )
            .map_err(|e| KernelError::Driver(format!("prepare get lease by id: {}", e)))?;
        let mut rows = stmt
            .query(params![lease_id])
            .map_err(|e| KernelError::Driver(format!("query get lease by id: {}", e)))?;
        if let Some(row) = rows
            .next()
            .map_err(|e| KernelError::Driver(format!("scan get lease by id: {}", e)))?
        {
            Ok(Some(LeaseRecord {
                lease_id: row.get(0).map_err(map_rusqlite_err)?,
                attempt_id: row.get(1).map_err(map_rusqlite_err)?,
                worker_id: row.get(2).map_err(map_rusqlite_err)?,
                lease_expires_at: ms_to_dt(row.get::<_, i64>(3).map_err(map_rusqlite_err)?),
                heartbeat_at: ms_to_dt(row.get::<_, i64>(4).map_err(map_rusqlite_err)?),
                version: row.get::<_, i64>(5).map_err(map_rusqlite_err)? as u64,
                terminal_state: None,
                terminal_at: None,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn active_leases_for_worker(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
    ) -> Result<usize, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM runtime_leases WHERE worker_id = ?1 AND lease_expires_at_ms >= ?2",
                params![worker_id, dt_to_ms(now)],
                |r| r.get(0),
            )
            .map_err(|e| KernelError::Driver(format!("count active leases: {}", e)))?;
        Ok(count as usize)
    }

    pub fn active_leases_for_tenant(
        &self,
        tenant_id: &str,
        now: DateTime<Utc>,
    ) -> Result<usize, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*)
                 FROM runtime_leases l
                 JOIN runtime_attempts a ON a.attempt_id = l.attempt_id
                 WHERE a.tenant_id = ?1
                   AND l.lease_expires_at_ms >= ?2",
                params![tenant_id, dt_to_ms(now)],
                |r| r.get(0),
            )
            .map_err(|e| KernelError::Driver(format!("count tenant active leases: {}", e)))?;
        Ok(count as usize)
    }

    pub fn queue_depth(&self, now: DateTime<Utc>) -> Result<usize, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*)
                 FROM runtime_attempts a
                 LEFT JOIN runtime_leases l ON l.attempt_id = a.attempt_id AND l.lease_expires_at_ms >= ?1
                 WHERE l.attempt_id IS NULL
                   AND (
                     a.status = 'queued'
                     OR (a.status = 'retry_backoff' AND (a.retry_at_ms IS NULL OR a.retry_at_ms <= ?1))
                   )",
                params![dt_to_ms(now)],
                |r| r.get(0),
            )
            .map_err(|e| KernelError::Driver(format!("queue depth: {}", e)))?;
        Ok(count as usize)
    }

    pub fn a2a_compat_queue_depth(&self) -> Result<usize, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM runtime_a2a_compat_tasks", [], |r| {
                r.get(0)
            })
            .map_err(|e| KernelError::Driver(format!("a2a compat queue depth: {}", e)))?;
        Ok(count as usize)
    }

    pub fn list_dispatchable_attempt_contexts(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<DispatchableAttemptContext>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT a.attempt_id, a.tenant_id, a.started_at_ms
                 FROM runtime_attempts a
                 LEFT JOIN runtime_leases l ON l.attempt_id = a.attempt_id AND l.lease_expires_at_ms >= ?1
                 WHERE l.attempt_id IS NULL
                   AND (
                     a.status = 'queued'
                     OR (a.status = 'retry_backoff' AND (a.retry_at_ms IS NULL OR a.retry_at_ms <= ?1))
                   )
                 ORDER BY a.priority DESC, a.attempt_no ASC, a.attempt_id ASC
                 LIMIT ?2",
            )
            .map_err(|e| KernelError::Driver(format!("prepare dispatchable contexts: {}", e)))?;
        let rows = stmt
            .query_map(params![dt_to_ms(now), limit as i64], |row| {
                let started_at_ms: Option<i64> = row.get(2)?;
                Ok(DispatchableAttemptContext {
                    attempt_id: row.get(0)?,
                    tenant_id: row.get(1)?,
                    started_at: started_at_ms.map(ms_to_dt),
                })
            })
            .map_err(|e| KernelError::Driver(format!("query dispatchable contexts: {}", e)))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(map_rusqlite_err)?);
        }
        Ok(out)
    }

    pub fn heartbeat_lease_with_version(
        &self,
        lease_id: &str,
        worker_id: &str,
        expected_version: u64,
        heartbeat_at: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let updated = conn
            .execute(
                "UPDATE runtime_leases
                 SET heartbeat_at_ms = ?4, lease_expires_at_ms = ?5, version = version + 1
                 WHERE lease_id = ?1 AND worker_id = ?2 AND version = ?3",
                params![
                    lease_id,
                    worker_id,
                    expected_version as i64,
                    dt_to_ms(heartbeat_at),
                    dt_to_ms(lease_expires_at)
                ],
            )
            .map_err(|e| KernelError::Driver(format!("heartbeat lease with version: {}", e)))?;
        if updated == 0 {
            return Err(KernelError::Driver(format!(
                "lease heartbeat version conflict for lease: {}",
                lease_id
            )));
        }
        Ok(())
    }

    pub fn mark_attempt_status(
        &self,
        attempt_id: &str,
        status: AttemptExecutionStatus,
    ) -> Result<(), KernelError> {
        let status_str = attempt_status_to_str(&status);
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "UPDATE runtime_attempts
             SET status = ?2,
                 retry_at_ms = CASE
                   WHEN ?2 IN ('completed', 'failed', 'cancelled') THEN NULL
                   ELSE retry_at_ms
                 END,
                 started_at_ms = CASE
                   WHEN ?2 IN ('completed', 'failed', 'cancelled', 'queued', 'retry_backoff') THEN NULL
                   ELSE started_at_ms
                 END
             WHERE attempt_id = ?1",
            params![attempt_id, status_str],
        )
        .map_err(|e| KernelError::Driver(format!("mark attempt status: {}", e)))?;
        Ok(())
    }

    pub fn ack_attempt(
        &self,
        attempt_id: &str,
        status: AttemptExecutionStatus,
        retry_policy: Option<&RetryPolicyConfig>,
        now: DateTime<Utc>,
    ) -> Result<AttemptAckOutcome, KernelError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| KernelError::Driver(format!("begin ack attempt tx: {}", e)))?;

        let attempt_row = tx
            .query_row(
                "SELECT run_id, attempt_no, status, retry_strategy, retry_backoff_ms, retry_max_backoff_ms, retry_multiplier, retry_max_retries
                 FROM runtime_attempts
                 WHERE attempt_id = ?1",
                params![attempt_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, Option<f64>>(6)?,
                        row.get::<_, Option<i64>>(7)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| KernelError::Driver(format!("read attempt for ack: {}", e)))?;

        let Some((
            run_id,
            current_attempt_no,
            _current_status,
            stored_strategy,
            stored_backoff_ms,
            stored_max_backoff_ms,
            stored_multiplier,
            stored_max_retries,
        )) = attempt_row
        else {
            return Err(KernelError::Driver(format!(
                "attempt not found for ack: {}",
                attempt_id
            )));
        };

        if let Some(policy) = retry_policy {
            tx.execute(
                "UPDATE runtime_attempts
                 SET retry_strategy = ?2,
                     retry_backoff_ms = ?3,
                     retry_max_backoff_ms = ?4,
                     retry_multiplier = ?5,
                     retry_max_retries = ?6
                 WHERE attempt_id = ?1",
                params![
                    attempt_id,
                    policy.strategy.as_str(),
                    policy.backoff_ms,
                    policy.max_backoff_ms,
                    policy.multiplier,
                    policy.max_retries as i64
                ],
            )
            .map_err(|e| KernelError::Driver(format!("persist retry policy: {}", e)))?;
        }

        tx.execute(
            "DELETE FROM runtime_leases WHERE attempt_id = ?1",
            params![attempt_id],
        )
        .map_err(|e| KernelError::Driver(format!("delete attempt lease on ack: {}", e)))?;

        if status == AttemptExecutionStatus::Failed {
            let effective_policy = retry_policy.cloned().or_else(|| {
                parse_retry_policy_record(
                    stored_strategy,
                    stored_backoff_ms,
                    stored_max_backoff_ms,
                    stored_multiplier,
                    stored_max_retries,
                )
            });

            let current_attempt_no = current_attempt_no.max(1) as u32;
            if let Some(policy) = effective_policy {
                if current_attempt_no <= policy.max_retries {
                    let next_attempt_no = current_attempt_no + 1;
                    let backoff_ms = policy.next_backoff_ms(current_attempt_no).max(1);
                    let scheduled_at = now + Duration::milliseconds(backoff_ms);
                    tx.execute(
                        "UPDATE runtime_attempts
                         SET attempt_no = ?2,
                             status = 'retry_backoff',
                             retry_at_ms = ?3,
                             started_at_ms = NULL
                         WHERE attempt_id = ?1",
                        params![attempt_id, next_attempt_no as i64, dt_to_ms(scheduled_at)],
                    )
                    .map_err(|e| KernelError::Driver(format!("schedule retry backoff: {}", e)))?;
                    tx.execute(
                        "INSERT INTO runtime_attempt_retry_history
                         (attempt_id, attempt_no, strategy, backoff_ms, max_retries, scheduled_at_ms)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![
                            attempt_id,
                            next_attempt_no as i64,
                            policy.strategy.as_str(),
                            backoff_ms,
                            policy.max_retries as i64,
                            dt_to_ms(scheduled_at)
                        ],
                    )
                    .map_err(|e| KernelError::Driver(format!("insert retry history: {}", e)))?;
                    tx.commit()
                        .map_err(|e| KernelError::Driver(format!("commit retry ack: {}", e)))?;
                    return Ok(AttemptAckOutcome {
                        status: AttemptExecutionStatus::RetryBackoff,
                        next_retry_at: Some(scheduled_at),
                        next_attempt_no,
                    });
                }
            }
        }

        tx.execute(
            "UPDATE runtime_attempts
             SET status = ?2,
                 retry_at_ms = NULL,
                 started_at_ms = NULL
             WHERE attempt_id = ?1",
            params![attempt_id, attempt_status_to_str(&status)],
        )
        .map_err(|e| KernelError::Driver(format!("mark terminal attempt status: {}", e)))?;
        if status == AttemptExecutionStatus::Failed {
            tx.execute(
                "INSERT INTO runtime_dead_letters
                 (attempt_id, run_id, attempt_no, terminal_status, reason, dead_at_ms, replay_status, replay_count, last_replayed_at_ms)
                 VALUES (?1, ?2, ?3, 'failed', ?4, ?5, 'pending', 0, NULL)
                 ON CONFLICT(attempt_id) DO UPDATE SET
                   run_id = excluded.run_id,
                   attempt_no = excluded.attempt_no,
                   terminal_status = excluded.terminal_status,
                   reason = excluded.reason,
                   dead_at_ms = excluded.dead_at_ms,
                   replay_status = 'pending'",
                params![
                    attempt_id,
                    run_id,
                    current_attempt_no,
                    "terminal_failed",
                    dt_to_ms(now)
                ],
            )
            .map_err(|e| KernelError::Driver(format!("upsert dead letter from ack: {}", e)))?;
        }
        tx.commit()
            .map_err(|e| KernelError::Driver(format!("commit terminal ack: {}", e)))?;
        Ok(AttemptAckOutcome {
            status: status.clone(),
            next_retry_at: None,
            next_attempt_no: current_attempt_no.max(1) as u32,
        })
    }

    pub fn get_attempt_retry_history(
        &self,
        attempt_id: &str,
    ) -> Result<Option<AttemptRetryHistorySnapshot>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let attempt = conn
            .query_row(
                "SELECT attempt_no, status FROM runtime_attempts WHERE attempt_id = ?1",
                params![attempt_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|e| KernelError::Driver(format!("read retry history attempt: {}", e)))?;
        let Some((current_attempt_no, status)) = attempt else {
            return Ok(None);
        };

        let mut stmt = conn
            .prepare(
                "SELECT attempt_no, strategy, backoff_ms, max_retries, scheduled_at_ms
                 FROM runtime_attempt_retry_history
                 WHERE attempt_id = ?1
                 ORDER BY retry_id ASC",
            )
            .map_err(|e| KernelError::Driver(format!("prepare retry history: {}", e)))?;
        let rows = stmt
            .query_map(params![attempt_id], |row| {
                Ok(AttemptRetryHistoryRow {
                    attempt_no: row.get::<_, i64>(0)? as u32,
                    strategy: row.get(1)?,
                    backoff_ms: row.get(2)?,
                    max_retries: row.get::<_, i64>(3)? as u32,
                    scheduled_at: ms_to_dt(row.get::<_, i64>(4)?),
                })
            })
            .map_err(|e| KernelError::Driver(format!("query retry history: {}", e)))?;
        let mut history = Vec::new();
        for row in rows {
            history.push(row.map_err(map_rusqlite_err)?);
        }

        Ok(Some(AttemptRetryHistorySnapshot {
            attempt_id: attempt_id.to_string(),
            current_attempt_no: current_attempt_no.max(1) as u32,
            current_status: parse_attempt_status(&status),
            history,
        }))
    }

    pub fn list_dead_letters(
        &self,
        status_filter: Option<&str>,
        limit: usize,
    ) -> Result<Vec<DeadLetterRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut out = Vec::new();
        if let Some(status) = status_filter {
            let mut stmt = conn
                .prepare(
                    "SELECT attempt_id, run_id, attempt_no, terminal_status, reason, dead_at_ms, replay_status, replay_count, last_replayed_at_ms
                     FROM runtime_dead_letters
                     WHERE replay_status = ?1
                     ORDER BY dead_at_ms DESC
                     LIMIT ?2",
                )
                .map_err(|e| KernelError::Driver(format!("prepare list dead letters: {}", e)))?;
            let rows = stmt
                .query_map(params![status, limit as i64], map_row_to_dead_letter)
                .map_err(|e| KernelError::Driver(format!("query list dead letters: {}", e)))?;
            for row in rows {
                out.push(row.map_err(map_rusqlite_err)?);
            }
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT attempt_id, run_id, attempt_no, terminal_status, reason, dead_at_ms, replay_status, replay_count, last_replayed_at_ms
                     FROM runtime_dead_letters
                     ORDER BY dead_at_ms DESC
                     LIMIT ?1",
                )
                .map_err(|e| KernelError::Driver(format!("prepare list dead letters: {}", e)))?;
            let rows = stmt
                .query_map(params![limit as i64], map_row_to_dead_letter)
                .map_err(|e| KernelError::Driver(format!("query list dead letters: {}", e)))?;
            for row in rows {
                out.push(row.map_err(map_rusqlite_err)?);
            }
        }
        Ok(out)
    }

    pub fn get_dead_letter(&self, attempt_id: &str) -> Result<Option<DeadLetterRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.query_row(
            "SELECT attempt_id, run_id, attempt_no, terminal_status, reason, dead_at_ms, replay_status, replay_count, last_replayed_at_ms
             FROM runtime_dead_letters
             WHERE attempt_id = ?1",
            params![attempt_id],
            map_row_to_dead_letter,
        )
        .optional()
        .map_err(|e| KernelError::Driver(format!("get dead letter: {}", e)))
    }

    pub fn replay_dead_letter(
        &self,
        attempt_id: &str,
        now: DateTime<Utc>,
    ) -> Result<DeadLetterRow, KernelError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| KernelError::Driver(format!("begin replay dead letter tx: {}", e)))?;
        let Some(mut row) = tx
            .query_row(
                "SELECT attempt_id, run_id, attempt_no, terminal_status, reason, dead_at_ms, replay_status, replay_count, last_replayed_at_ms
                 FROM runtime_dead_letters
                 WHERE attempt_id = ?1",
                params![attempt_id],
                map_row_to_dead_letter,
            )
            .optional()
            .map_err(|e| KernelError::Driver(format!("read dead letter for replay: {}", e)))?
        else {
            return Err(KernelError::Driver(format!(
                "dead letter not found for attempt: {}",
                attempt_id
            )));
        };

        if row.replay_status != "pending" {
            return Err(KernelError::Driver(format!(
                "dead letter already replayed for attempt: {}",
                attempt_id
            )));
        }

        tx.execute(
            "DELETE FROM runtime_leases WHERE attempt_id = ?1",
            params![attempt_id],
        )
        .map_err(|e| KernelError::Driver(format!("delete lease before dlq replay: {}", e)))?;
        let updated = tx
            .execute(
                "UPDATE runtime_attempts
                 SET status = 'queued',
                     retry_at_ms = NULL,
                     started_at_ms = NULL
                 WHERE attempt_id = ?1",
                params![attempt_id],
            )
            .map_err(|e| KernelError::Driver(format!("requeue dead letter attempt: {}", e)))?;
        if updated == 0 {
            return Err(KernelError::Driver(format!(
                "attempt not found for dead letter replay: {}",
                attempt_id
            )));
        }
        tx.execute(
            "UPDATE runtime_dead_letters
             SET replay_status = 'replayed',
                 replay_count = replay_count + 1,
                 last_replayed_at_ms = ?2
             WHERE attempt_id = ?1",
            params![attempt_id, dt_to_ms(now)],
        )
        .map_err(|e| KernelError::Driver(format!("mark dead letter replayed: {}", e)))?;
        tx.commit()
            .map_err(|e| KernelError::Driver(format!("commit dead letter replay: {}", e)))?;

        row.replay_status = "replayed".to_string();
        row.replay_count += 1;
        row.last_replayed_at = Some(now);
        Ok(row)
    }

    pub fn claim_replay_effect(
        &self,
        thread_id: &str,
        replay_target: &str,
        fingerprint: &str,
        now: DateTime<Utc>,
    ) -> Result<ReplayEffectClaim, KernelError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| KernelError::Driver(format!("begin replay effect tx: {}", e)))?;

        let existing = tx
            .query_row(
                "SELECT status, response_json
                 FROM runtime_replay_effects
                 WHERE fingerprint = ?1",
                params![fingerprint],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .optional()
            .map_err(|e| KernelError::Driver(format!("read replay effect: {}", e)))?;

        let claim = match existing {
            Some((status, response_json)) if status == "completed" => {
                let stored = response_json.ok_or_else(|| {
                    KernelError::Driver("missing stored replay response".to_string())
                })?;
                ReplayEffectClaim::Completed(stored)
            }
            Some((status, _)) if status == "in_progress" => ReplayEffectClaim::InProgress,
            Some((_status, _)) => ReplayEffectClaim::InProgress,
            None => {
                tx.execute(
                    "INSERT INTO runtime_replay_effects
                     (fingerprint, thread_id, replay_target, effect_type, status, execution_count, created_at_ms, completed_at_ms, response_json)
                     VALUES (?1, ?2, ?3, 'job_replay', 'in_progress', 1, ?4, NULL, NULL)",
                    params![fingerprint, thread_id, replay_target, dt_to_ms(now)],
                )
                .map_err(|e| KernelError::Driver(format!("insert replay effect: {}", e)))?;
                ReplayEffectClaim::Acquired
            }
        };

        tx.commit()
            .map_err(|e| KernelError::Driver(format!("commit replay effect tx: {}", e)))?;
        Ok(claim)
    }

    pub fn complete_replay_effect(
        &self,
        fingerprint: &str,
        response_json: &str,
        now: DateTime<Utc>,
    ) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let updated = conn
            .execute(
                "UPDATE runtime_replay_effects
                 SET status = 'completed',
                     completed_at_ms = ?2,
                     response_json = ?3
                 WHERE fingerprint = ?1
                   AND status = 'in_progress'",
                params![fingerprint, dt_to_ms(now), response_json],
            )
            .map_err(|e| KernelError::Driver(format!("complete replay effect: {}", e)))?;
        if updated == 0 {
            return Err(KernelError::Driver(format!(
                "replay effect not claimable for completion: {}",
                fingerprint
            )));
        }
        Ok(())
    }

    pub fn abandon_replay_effect(&self, fingerprint: &str) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "DELETE FROM runtime_replay_effects
             WHERE fingerprint = ?1
               AND status = 'in_progress'",
            params![fingerprint],
        )
        .map_err(|e| KernelError::Driver(format!("abandon replay effect: {}", e)))?;
        Ok(())
    }

    pub fn list_replay_effects_for_thread(
        &self,
        thread_id: &str,
    ) -> Result<Vec<ReplayEffectLogRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT fingerprint, thread_id, replay_target, effect_type, status, execution_count, created_at_ms, completed_at_ms
                 FROM runtime_replay_effects
                 WHERE thread_id = ?1
                 ORDER BY created_at_ms ASC",
            )
            .map_err(|e| KernelError::Driver(format!("prepare list replay effects: {}", e)))?;
        let rows = stmt
            .query_map(params![thread_id], map_row_to_replay_effect_log)
            .map_err(|e| KernelError::Driver(format!("query replay effects: {}", e)))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| KernelError::Driver(format!("row replay effects: {}", e)))?);
        }
        Ok(out)
    }

    pub fn upsert_job(&self, thread_id: &str, status: &str) -> Result<(), KernelError> {
        let now = dt_to_ms(Utc::now());
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_jobs (thread_id, status, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(thread_id) DO UPDATE SET status = ?2, updated_at_ms = ?3",
            params![thread_id, status, now],
        )
        .map_err(|e| KernelError::Driver(format!("upsert job: {}", e)))?;
        Ok(())
    }

    pub fn list_runs(
        &self,
        limit: usize,
        offset: usize,
        status_filter: Option<&str>,
    ) -> Result<Vec<(String, String, DateTime<Utc>)>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let limit_i = limit as i64;
        let offset_i = offset as i64;
        let mut out = Vec::new();
        if let Some(s) = status_filter {
            let mut stmt = conn
                .prepare(
                    "SELECT thread_id, status, updated_at_ms FROM runtime_jobs WHERE status = ?1 ORDER BY updated_at_ms DESC LIMIT ?2 OFFSET ?3",
                )
                .map_err(|e| KernelError::Driver(format!("prepare list_runs: {}", e)))?;
            let rows = stmt
                .query_map(params![s, limit_i, offset_i], |row| {
                    let ms: i64 = row.get(2)?;
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        ms_to_dt(ms),
                    ))
                })
                .map_err(|e| KernelError::Driver(format!("query list_runs: {}", e)))?;
            for item in rows {
                out.push(item.map_err(map_rusqlite_err)?);
            }
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT thread_id, status, updated_at_ms FROM runtime_jobs ORDER BY updated_at_ms DESC LIMIT ?1 OFFSET ?2",
                )
                .map_err(|e| KernelError::Driver(format!("prepare list_runs: {}", e)))?;
            let rows = stmt
                .query_map(params![limit_i, offset_i], |row| {
                    let ms: i64 = row.get(2)?;
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        ms_to_dt(ms),
                    ))
                })
                .map_err(|e| KernelError::Driver(format!("query list_runs: {}", e)))?;
            for item in rows {
                out.push(item.map_err(map_rusqlite_err)?);
            }
        }
        Ok(out)
    }

    pub fn insert_interrupt(
        &self,
        interrupt_id: &str,
        thread_id: &str,
        run_id: &str,
        attempt_id: &str,
        value_json: &str,
    ) -> Result<(), KernelError> {
        let now = dt_to_ms(Utc::now());
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_interrupts (interrupt_id, thread_id, run_id, attempt_id, value_json, status, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6)",
            params![interrupt_id, thread_id, run_id, attempt_id, value_json, now],
        )
        .map_err(|e| KernelError::Driver(format!("insert interrupt: {}", e)))?;
        Ok(())
    }

    pub fn list_interrupts(
        &self,
        status_filter: Option<&str>,
        run_id_filter: Option<&str>,
        limit: usize,
    ) -> Result<Vec<InterruptRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let limit_i = limit as i64;
        let mut out = Vec::new();
        if let (Some(s), Some(r)) = (status_filter, run_id_filter) {
            let mut stmt = conn
                .prepare(
                    "SELECT interrupt_id, thread_id, run_id, attempt_id, value_json, status, created_at_ms, resume_payload_hash, resume_response_json
                     FROM runtime_interrupts WHERE status = ?1 AND run_id = ?2 ORDER BY created_at_ms DESC LIMIT ?3",
                )
                .map_err(|e| KernelError::Driver(format!("prepare list_interrupts: {}", e)))?;
            let rows = stmt
                .query_map(params![s, r, limit_i], map_row_to_interrupt)
                .map_err(|e| KernelError::Driver(format!("query list_interrupts: {}", e)))?;
            for item in rows {
                out.push(item.map_err(map_rusqlite_err)?);
            }
        } else if let Some(s) = status_filter {
            let mut stmt = conn
                .prepare(
                    "SELECT interrupt_id, thread_id, run_id, attempt_id, value_json, status, created_at_ms, resume_payload_hash, resume_response_json
                     FROM runtime_interrupts WHERE status = ?1 ORDER BY created_at_ms DESC LIMIT ?2",
                )
                .map_err(|e| KernelError::Driver(format!("prepare list_interrupts: {}", e)))?;
            let rows = stmt
                .query_map(params![s, limit_i], map_row_to_interrupt)
                .map_err(|e| KernelError::Driver(format!("query list_interrupts: {}", e)))?;
            for item in rows {
                out.push(item.map_err(map_rusqlite_err)?);
            }
        } else if let Some(r) = run_id_filter {
            let mut stmt = conn
                .prepare(
                    "SELECT interrupt_id, thread_id, run_id, attempt_id, value_json, status, created_at_ms, resume_payload_hash, resume_response_json
                     FROM runtime_interrupts WHERE run_id = ?1 ORDER BY created_at_ms DESC LIMIT ?2",
                )
                .map_err(|e| KernelError::Driver(format!("prepare list_interrupts: {}", e)))?;
            let rows = stmt
                .query_map(params![r, limit_i], map_row_to_interrupt)
                .map_err(|e| KernelError::Driver(format!("query list_interrupts: {}", e)))?;
            for item in rows {
                out.push(item.map_err(map_rusqlite_err)?);
            }
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT interrupt_id, thread_id, run_id, attempt_id, value_json, status, created_at_ms, resume_payload_hash, resume_response_json
                     FROM runtime_interrupts ORDER BY created_at_ms DESC LIMIT ?1",
                )
                .map_err(|e| KernelError::Driver(format!("prepare list_interrupts: {}", e)))?;
            let rows = stmt
                .query_map(params![limit_i], map_row_to_interrupt)
                .map_err(|e| KernelError::Driver(format!("query list_interrupts: {}", e)))?;
            for item in rows {
                out.push(item.map_err(map_rusqlite_err)?);
            }
        }
        Ok(out)
    }

    pub fn get_interrupt(&self, interrupt_id: &str) -> Result<Option<InterruptRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT interrupt_id, thread_id, run_id, attempt_id, value_json, status, created_at_ms, resume_payload_hash, resume_response_json
                 FROM runtime_interrupts WHERE interrupt_id = ?1",
            )
            .map_err(|e| KernelError::Driver(format!("prepare get_interrupt: {}", e)))?;
        let mut rows = stmt
            .query(params![interrupt_id])
            .map_err(|e| KernelError::Driver(format!("query get_interrupt: {}", e)))?;
        if let Some(row) = rows
            .next()
            .map_err(|e| KernelError::Driver(format!("scan get_interrupt: {}", e)))?
        {
            Ok(Some(map_row_to_interrupt(&row).map_err(map_rusqlite_err)?))
        } else {
            Ok(None)
        }
    }

    pub fn update_interrupt_status(
        &self,
        interrupt_id: &str,
        status: &str,
    ) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let updated = conn
            .execute(
                "UPDATE runtime_interrupts SET status = ?2 WHERE interrupt_id = ?1",
                params![interrupt_id, status],
            )
            .map_err(|e| KernelError::Driver(format!("update interrupt status: {}", e)))?;
        if updated == 0 {
            return Err(KernelError::Driver(format!(
                "interrupt not found: {}",
                interrupt_id
            )));
        }
        Ok(())
    }

    pub fn persist_interrupt_resume_result(
        &self,
        interrupt_id: &str,
        resume_payload_hash: &str,
        resume_response_json: &str,
    ) -> Result<(), KernelError> {
        let now = dt_to_ms(Utc::now());
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| KernelError::Driver(format!("begin resume result tx: {}", e)))?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT resume_payload_hash FROM runtime_interrupts WHERE interrupt_id = ?1",
                params![interrupt_id],
                |r| r.get(0),
            )
            .map_err(map_rusqlite_err)?;
        if let Some(hash) = existing {
            if hash != resume_payload_hash {
                return Err(KernelError::Driver(format!(
                    "interrupt {} already resumed with a different payload",
                    interrupt_id
                )));
            }
        }
        let updated = tx
            .execute(
                "UPDATE runtime_interrupts
                 SET status = 'resumed',
                     resume_payload_hash = COALESCE(resume_payload_hash, ?2),
                     resume_response_json = COALESCE(resume_response_json, ?3),
                     resumed_at_ms = COALESCE(resumed_at_ms, ?4)
                 WHERE interrupt_id = ?1",
                params![interrupt_id, resume_payload_hash, resume_response_json, now],
            )
            .map_err(|e| KernelError::Driver(format!("persist interrupt resume result: {}", e)))?;
        if updated == 0 {
            return Err(KernelError::Driver(format!(
                "interrupt not found: {}",
                interrupt_id
            )));
        }
        tx.commit()
            .map_err(|e| KernelError::Driver(format!("commit resume result tx: {}", e)))?;
        Ok(())
    }

    pub fn record_step_report(
        &self,
        worker_id: &str,
        attempt_id: &str,
        action_id: &str,
        status: &str,
        dedupe_token: &str,
    ) -> Result<StepReportWriteResult, KernelError> {
        let now = dt_to_ms(Utc::now());
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let insert = conn.execute(
            "INSERT INTO runtime_step_reports
             (worker_id, attempt_id, action_id, status, dedupe_token, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![worker_id, attempt_id, action_id, status, dedupe_token, now],
        );
        match insert {
            Ok(_) => Ok(StepReportWriteResult::Inserted),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == ErrorCode::ConstraintViolation =>
            {
                let existing = conn
                    .query_row(
                        "SELECT action_id, status FROM runtime_step_reports
                         WHERE attempt_id = ?1 AND dedupe_token = ?2",
                        params![attempt_id, dedupe_token],
                        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                    )
                    .map_err(map_rusqlite_err)?;
                if existing.0 == action_id && existing.1 == status {
                    Ok(StepReportWriteResult::Duplicate)
                } else {
                    Err(KernelError::Driver(format!(
                        "dedupe_token '{}' already used with different payload for attempt '{}'",
                        dedupe_token, attempt_id
                    )))
                }
            }
            Err(e) => Err(KernelError::Driver(format!("record step report: {}", e))),
        }
    }

    pub fn upsert_api_key_record(
        &self,
        key_id: &str,
        secret_hash: &str,
        active: bool,
        role: &str,
    ) -> Result<(), KernelError> {
        let now = dt_to_ms(Utc::now());
        let status = if active { "active" } else { "disabled" };
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_api_keys (key_id, secret_hash, role, status, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(key_id)
             DO UPDATE SET secret_hash = excluded.secret_hash, role = excluded.role, status = excluded.status, updated_at_ms = excluded.updated_at_ms",
            params![key_id, secret_hash, role, status, now],
        )
        .map_err(|e| KernelError::Driver(format!("upsert api key: {}", e)))?;
        Ok(())
    }

    pub fn get_api_key_record(&self, key_id: &str) -> Result<Option<ApiKeyRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT key_id, secret_hash, role, status, created_at_ms, updated_at_ms
                 FROM runtime_api_keys WHERE key_id = ?1",
            )
            .map_err(|e| KernelError::Driver(format!("prepare get_api_key_record: {}", e)))?;
        let mut rows = stmt
            .query(params![key_id])
            .map_err(|e| KernelError::Driver(format!("query get_api_key_record: {}", e)))?;
        if let Some(row) = rows
            .next()
            .map_err(|e| KernelError::Driver(format!("scan get_api_key_record: {}", e)))?
        {
            let status: String = row.get(3).map_err(map_rusqlite_err)?;
            Ok(Some(ApiKeyRow {
                key_id: row.get(0).map_err(map_rusqlite_err)?,
                secret_hash: row.get(1).map_err(map_rusqlite_err)?,
                role: row.get(2).map_err(map_rusqlite_err)?,
                active: status == "active",
                created_at: ms_to_dt(row.get::<_, i64>(4).map_err(map_rusqlite_err)?),
                updated_at: ms_to_dt(row.get::<_, i64>(5).map_err(map_rusqlite_err)?),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn set_api_key_status(&self, key_id: &str, active: bool) -> Result<(), KernelError> {
        let now = dt_to_ms(Utc::now());
        let status = if active { "active" } else { "disabled" };
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let updated = conn
            .execute(
                "UPDATE runtime_api_keys SET status = ?2, updated_at_ms = ?3 WHERE key_id = ?1",
                params![key_id, status, now],
            )
            .map_err(|e| KernelError::Driver(format!("set api key status: {}", e)))?;
        if updated == 0 {
            return Err(KernelError::Driver(format!(
                "api key not found: {}",
                key_id
            )));
        }
        Ok(())
    }

    pub fn has_any_api_keys(&self) -> Result<bool, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM runtime_api_keys", [], |r| r.get(0))
            .map_err(|e| KernelError::Driver(format!("count api keys: {}", e)))?;
        Ok(count > 0)
    }

    pub fn upsert_a2a_session(&self, session: &A2aSessionRow) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_a2a_sessions
             (sender_id, protocol, protocol_version, enabled_capabilities_json, actor_type, actor_id, actor_role, negotiated_at_ms, expires_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(sender_id)
             DO UPDATE SET
               protocol = excluded.protocol,
               protocol_version = excluded.protocol_version,
               enabled_capabilities_json = excluded.enabled_capabilities_json,
               actor_type = excluded.actor_type,
               actor_id = excluded.actor_id,
               actor_role = excluded.actor_role,
               negotiated_at_ms = excluded.negotiated_at_ms,
               expires_at_ms = excluded.expires_at_ms,
               updated_at_ms = excluded.updated_at_ms",
            params![
                &session.sender_id,
                &session.protocol,
                &session.protocol_version,
                &session.enabled_capabilities_json,
                session.actor_type.as_deref(),
                session.actor_id.as_deref(),
                session.actor_role.as_deref(),
                dt_to_ms(session.negotiated_at),
                dt_to_ms(session.expires_at),
                dt_to_ms(session.updated_at),
            ],
        )
        .map_err(|e| KernelError::Driver(format!("upsert a2a session: {}", e)))?;
        Ok(())
    }

    pub fn get_active_a2a_session(
        &self,
        sender_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<A2aSessionRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT sender_id, protocol, protocol_version, enabled_capabilities_json,
                        actor_type, actor_id, actor_role, negotiated_at_ms, expires_at_ms, updated_at_ms
                 FROM runtime_a2a_sessions
                 WHERE sender_id = ?1
                   AND expires_at_ms > ?2",
            )
            .map_err(|e| KernelError::Driver(format!("prepare get active a2a session: {}", e)))?;
        let mut rows = stmt
            .query(params![sender_id, dt_to_ms(now)])
            .map_err(|e| KernelError::Driver(format!("query get active a2a session: {}", e)))?;
        if let Some(row) = rows
            .next()
            .map_err(|e| KernelError::Driver(format!("scan get active a2a session: {}", e)))?
        {
            Ok(Some(A2aSessionRow {
                sender_id: row.get(0).map_err(map_rusqlite_err)?,
                protocol: row.get(1).map_err(map_rusqlite_err)?,
                protocol_version: row.get(2).map_err(map_rusqlite_err)?,
                enabled_capabilities_json: row.get(3).map_err(map_rusqlite_err)?,
                actor_type: row.get(4).map_err(map_rusqlite_err)?,
                actor_id: row.get(5).map_err(map_rusqlite_err)?,
                actor_role: row.get(6).map_err(map_rusqlite_err)?,
                negotiated_at: ms_to_dt(row.get::<_, i64>(7).map_err(map_rusqlite_err)?),
                expires_at: ms_to_dt(row.get::<_, i64>(8).map_err(map_rusqlite_err)?),
                updated_at: ms_to_dt(row.get::<_, i64>(9).map_err(map_rusqlite_err)?),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn purge_expired_a2a_sessions(&self, now: DateTime<Utc>) -> Result<u64, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let deleted = conn
            .execute(
                "DELETE FROM runtime_a2a_sessions WHERE expires_at_ms <= ?1",
                params![dt_to_ms(now)],
            )
            .map_err(|e| KernelError::Driver(format!("purge expired a2a sessions: {}", e)))?;
        Ok(deleted as u64)
    }

    pub fn create_bounty(
        &self,
        bounty_id: &str,
        title: &str,
        description: Option<&str>,
        reward: i64,
        created_by: &str,
        created_at: DateTime<Utc>,
    ) -> Result<BountyRow, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_bounties
             (bounty_id, title, description, reward, status, created_by, created_at_ms, closed_at_ms, accepted_by, accepted_at_ms)
             VALUES (?1, ?2, ?3, ?4, 'open', ?5, ?6, NULL, NULL, NULL)",
            params![
                bounty_id,
                title,
                description,
                reward,
                created_by,
                dt_to_ms(created_at)
            ],
        )
        .map_err(|e| KernelError::Driver(format!("create bounty: {}", e)))?;
        drop(conn);
        self.get_bounty(bounty_id)?
            .ok_or_else(|| KernelError::Driver("created bounty missing after insert".to_string()))
    }

    pub fn get_bounty(&self, bounty_id: &str) -> Result<Option<BountyRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.query_row(
            "SELECT bounty_id, title, description, reward, status, created_by, created_at_ms, closed_at_ms, accepted_by, accepted_at_ms
             FROM runtime_bounties
             WHERE bounty_id = ?1",
            params![bounty_id],
            map_row_to_bounty,
        )
        .optional()
        .map_err(|e| KernelError::Driver(format!("get bounty: {}", e)))
    }

    pub fn accept_bounty(
        &self,
        bounty_id: &str,
        accepted_by: &str,
        accepted_at: DateTime<Utc>,
    ) -> Result<bool, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let updated = conn
            .execute(
                "UPDATE runtime_bounties
                 SET status = 'accepted',
                     accepted_by = ?2,
                     accepted_at_ms = ?3
                 WHERE bounty_id = ?1
                   AND status = 'open'",
                params![bounty_id, accepted_by, dt_to_ms(accepted_at)],
            )
            .map_err(|e| KernelError::Driver(format!("accept bounty: {}", e)))?;
        Ok(updated > 0)
    }

    pub fn close_bounty(
        &self,
        bounty_id: &str,
        closed_at: DateTime<Utc>,
    ) -> Result<bool, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let updated = conn
            .execute(
                "UPDATE runtime_bounties
                 SET status = 'closed',
                     closed_at_ms = ?2
                 WHERE bounty_id = ?1
                   AND status = 'accepted'",
                params![bounty_id, dt_to_ms(closed_at)],
            )
            .map_err(|e| KernelError::Driver(format!("close bounty: {}", e)))?;
        Ok(updated > 0)
    }

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
    ) -> Result<SwarmTaskRow, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_swarm_tasks
             (parent_task_id, decomposition_json, proposer_id, proposer_reward_pct, solver_reward_pct, aggregator_reward_pct, status, created_at_ms, completed_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL)",
            params![
                parent_task_id,
                decomposition_json,
                proposer_id,
                proposer_reward_pct,
                solver_reward_pct,
                aggregator_reward_pct,
                status,
                dt_to_ms(created_at)
            ],
        )
        .map_err(|e| KernelError::Driver(format!("create swarm task: {}", e)))?;
        drop(conn);
        self.get_swarm_task(parent_task_id)?.ok_or_else(|| {
            KernelError::Driver("created swarm task missing after insert".to_string())
        })
    }

    pub fn get_swarm_task(
        &self,
        parent_task_id: &str,
    ) -> Result<Option<SwarmTaskRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.query_row(
            "SELECT parent_task_id, decomposition_json, proposer_id, proposer_reward_pct, solver_reward_pct, aggregator_reward_pct, status, created_at_ms, completed_at_ms
             FROM runtime_swarm_tasks
             WHERE parent_task_id = ?1",
            params![parent_task_id],
            map_row_to_swarm_task,
        )
        .optional()
        .map_err(|e| KernelError::Driver(format!("get swarm task: {}", e)))
    }

    pub fn upsert_worker_registration(
        &self,
        worker_id: &str,
        domains_json: &str,
        max_load: i32,
        metadata_json: Option<&str>,
        status: &str,
        now: DateTime<Utc>,
    ) -> Result<WorkerRegistryRow, KernelError> {
        let now_ms = dt_to_ms(now);
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_workers_registry
             (worker_id, domains, max_load, metadata_json, registered_at_ms, last_heartbeat_ms, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)
             ON CONFLICT(worker_id)
             DO UPDATE SET
               domains = excluded.domains,
               max_load = excluded.max_load,
               metadata_json = excluded.metadata_json,
               last_heartbeat_ms = excluded.last_heartbeat_ms,
               status = excluded.status",
            params![worker_id, domains_json, max_load, metadata_json, now_ms, status],
        )
        .map_err(|e| KernelError::Driver(format!("upsert worker registration: {}", e)))?;
        drop(conn);
        self.get_worker_registration(worker_id)?
            .ok_or_else(|| KernelError::Driver("worker row missing after upsert".to_string()))
    }

    pub fn get_worker_registration(
        &self,
        worker_id: &str,
    ) -> Result<Option<WorkerRegistryRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.query_row(
            "SELECT worker_id, domains, max_load, metadata_json, registered_at_ms, last_heartbeat_ms, status
             FROM runtime_workers_registry
             WHERE worker_id = ?1",
            params![worker_id],
            map_row_to_worker_registry,
        )
        .optional()
        .map_err(|e| KernelError::Driver(format!("get worker registration: {}", e)))
    }

    pub fn count_active_claims_for_worker(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
    ) -> Result<u64, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let count = conn
            .query_row(
                "SELECT COUNT(*)
                 FROM runtime_a2a_compat_tasks
                 WHERE claimed_by_sender_id = ?1
                   AND lease_expires_at_ms IS NOT NULL
                   AND lease_expires_at_ms > ?2",
                params![worker_id, dt_to_ms(now)],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| KernelError::Driver(format!("count active claims for worker: {}", e)))?;
        Ok(count.max(0) as u64)
    }

    pub fn create_dispute(
        &self,
        dispute_id: &str,
        bounty_id: &str,
        opened_by: &str,
        description: &str,
        created_at: DateTime<Utc>,
    ) -> Result<DisputeRow, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let opening_evidence = serde_json::json!([{
            "kind": "opening_description",
            "submitted_by": opened_by,
            "content": description,
            "submitted_at_ms": dt_to_ms(created_at)
        }])
        .to_string();
        conn.execute(
            "INSERT INTO runtime_disputes
             (dispute_id, bounty_id, opened_by, status, evidence_json, resolution, resolved_by, created_at_ms, resolved_at_ms)
             VALUES (?1, ?2, ?3, 'open', ?4, NULL, NULL, ?5, NULL)",
            params![
                dispute_id,
                bounty_id,
                opened_by,
                opening_evidence,
                dt_to_ms(created_at)
            ],
        )
        .map_err(|e| KernelError::Driver(format!("create dispute: {}", e)))?;
        drop(conn);
        self.get_dispute(dispute_id)?
            .ok_or_else(|| KernelError::Driver("created dispute missing after insert".to_string()))
    }

    pub fn get_dispute(&self, dispute_id: &str) -> Result<Option<DisputeRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.query_row(
            "SELECT dispute_id, bounty_id, opened_by, status, resolution, resolved_by, created_at_ms, resolved_at_ms, evidence_json
             FROM runtime_disputes
             WHERE dispute_id = ?1",
            params![dispute_id],
            map_row_to_dispute,
        )
        .optional()
        .map_err(|e| KernelError::Driver(format!("get dispute: {}", e)))
    }

    pub fn append_dispute_evidence(
        &self,
        dispute_id: &str,
        submitted_by: &str,
        evidence_json: &str,
    ) -> Result<bool, KernelError> {
        let payload = serde_json::json!({
            "submitted_by": submitted_by,
            "evidence": evidence_json,
            "submitted_at_ms": Utc::now().timestamp_millis()
        })
        .to_string();
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let updated = conn
            .execute(
                "UPDATE runtime_disputes
                 SET evidence_json = CASE
                     WHEN evidence_json IS NULL OR evidence_json = ''
                       THEN json_array(?2)
                     ELSE json_insert(COALESCE(evidence_json, '[]'), '$[#]', ?2)
                 END
                 WHERE dispute_id = ?1
                   AND status = 'open'",
                params![dispute_id, payload],
            )
            .map_err(|e| KernelError::Driver(format!("append dispute evidence: {}", e)))?;
        Ok(updated > 0)
    }

    pub fn resolve_dispute(
        &self,
        dispute_id: &str,
        resolved_by: &str,
        resolution: &str,
        resolved_at: DateTime<Utc>,
    ) -> Result<Option<DisputeRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let updated = conn
            .execute(
                "UPDATE runtime_disputes
                 SET status = 'resolved',
                     resolution = ?2,
                     resolved_by = ?3,
                     resolved_at_ms = ?4
                 WHERE dispute_id = ?1
                   AND status = 'open'",
                params![dispute_id, resolution, resolved_by, dt_to_ms(resolved_at)],
            )
            .map_err(|e| KernelError::Driver(format!("resolve dispute: {}", e)))?;
        if updated == 0 {
            return Ok(None);
        }
        drop(conn);
        self.get_dispute(dispute_id)
    }

    pub fn settle_bounty_via_dispute(
        &self,
        bounty_id: &str,
        settlement_status: &str,
        closed_at: DateTime<Utc>,
    ) -> Result<bool, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let updated = conn
            .execute(
                "UPDATE runtime_bounties
                 SET status = ?2,
                     closed_at_ms = ?3
                 WHERE bounty_id = ?1
                   AND status IN ('open', 'accepted')",
                params![bounty_id, settlement_status, dt_to_ms(closed_at)],
            )
            .map_err(|e| KernelError::Driver(format!("settle bounty via dispute: {}", e)))?;
        Ok(updated > 0)
    }

    // ========== Recipe Methods ==========

    pub fn create_recipe(&self, recipe: &RecipeRow) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_recipes (
                recipe_id, name, description, gene_sequence_json, author_id,
                forked_from, created_at_ms, updated_at_ms, is_public
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                recipe.recipe_id,
                recipe.name,
                recipe.description,
                recipe.gene_sequence_json,
                recipe.author_id,
                recipe.forked_from,
                dt_to_ms(recipe.created_at),
                dt_to_ms(recipe.updated_at),
                recipe.is_public as i32,
            ],
        )
        .map_err(|e| KernelError::Driver(format!("create recipe: {}", e)))?;
        Ok(())
    }

    pub fn get_recipe(&self, recipe_id: &str) -> Result<Option<RecipeRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT recipe_id, name, description, gene_sequence_json, author_id,
                        forked_from, created_at_ms, updated_at_ms, is_public
                 FROM runtime_recipes WHERE recipe_id = ?1",
            )
            .map_err(|e| KernelError::Driver(format!("prepare get recipe: {}", e)))?;
        let result = stmt
            .query_row(params![recipe_id], |row| {
                Ok(RecipeRow {
                    recipe_id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    gene_sequence_json: row.get(3)?,
                    author_id: row.get(4)?,
                    forked_from: row.get(5)?,
                    created_at: ms_to_dt(row.get::<_, i64>(6)?),
                    updated_at: ms_to_dt(row.get::<_, i64>(7)?),
                    is_public: row.get::<_, i32>(8)? != 0,
                })
            })
            .optional()
            .map_err(|e| KernelError::Driver(format!("get recipe: {}", e)))?;
        Ok(result)
    }

    pub fn list_recipes_by_author(&self, author_id: &str) -> Result<Vec<RecipeRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT recipe_id, name, description, gene_sequence_json, author_id,
                        forked_from, created_at_ms, updated_at_ms, is_public
                 FROM runtime_recipes WHERE author_id = ?1 ORDER BY created_at_ms DESC",
            )
            .map_err(|e| KernelError::Driver(format!("prepare list recipes: {}", e)))?;
        let rows = stmt
            .query_map(params![author_id], |row| {
                Ok(RecipeRow {
                    recipe_id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    gene_sequence_json: row.get(3)?,
                    author_id: row.get(4)?,
                    forked_from: row.get(5)?,
                    created_at: ms_to_dt(row.get::<_, i64>(6)?),
                    updated_at: ms_to_dt(row.get::<_, i64>(7)?),
                    is_public: row.get::<_, i32>(8)? != 0,
                })
            })
            .map_err(|e| KernelError::Driver(format!("list recipes: {}", e)))?;
        let mut recipes = Vec::new();
        for row in rows {
            recipes.push(row.map_err(|e| KernelError::Driver(format!("iterate recipes: {}", e)))?);
        }
        Ok(recipes)
    }

    // ========== Organism Methods ==========

    pub fn create_organism(&self, organism: &OrganismRow) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_organisms (
                organism_id, recipe_id, status, current_step, total_steps,
                created_at_ms, completed_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                organism.organism_id,
                organism.recipe_id,
                organism.status,
                organism.current_step,
                organism.total_steps,
                dt_to_ms(organism.created_at),
                organism.completed_at.map(dt_to_ms),
            ],
        )
        .map_err(|e| KernelError::Driver(format!("create organism: {}", e)))?;
        Ok(())
    }

    pub fn get_organism(&self, organism_id: &str) -> Result<Option<OrganismRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT organism_id, recipe_id, status, current_step, total_steps,
                        created_at_ms, completed_at_ms
                 FROM runtime_organisms WHERE organism_id = ?1",
            )
            .map_err(|e| KernelError::Driver(format!("prepare get organism: {}", e)))?;
        let result = stmt
            .query_row(params![organism_id], |row| {
                Ok(OrganismRow {
                    organism_id: row.get(0)?,
                    recipe_id: row.get(1)?,
                    status: row.get(2)?,
                    current_step: row.get(3)?,
                    total_steps: row.get(4)?,
                    created_at: ms_to_dt(row.get::<_, i64>(5)?),
                    completed_at: row.get::<_, Option<i64>>(6)?.map(ms_to_dt),
                })
            })
            .optional()
            .map_err(|e| KernelError::Driver(format!("get organism: {}", e)))?;
        Ok(result)
    }

    pub fn update_organism_status(
        &self,
        organism_id: &str,
        status: &str,
        current_step: i32,
    ) -> Result<bool, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let completed_at_ms: Option<i64> = if status == "completed" {
            Some(dt_to_ms(Utc::now()))
        } else {
            None
        };
        let updated = conn
            .execute(
                "UPDATE runtime_organisms
                 SET status = ?2, current_step = ?3, completed_at_ms = ?4
                 WHERE organism_id = ?1",
                params![organism_id, status, current_step, completed_at_ms],
            )
            .map_err(|e| KernelError::Driver(format!("update organism status: {}", e)))?;
        Ok(updated > 0)
    }

    pub fn upsert_a2a_compat_task(&self, task: &A2aCompatTaskRow) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_a2a_compat_tasks
             (session_id, sender_id, protocol_version, task_id, task_summary, dispatch_id, claimed_by_sender_id, lease_expires_at_ms, enqueued_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(session_id)
             DO UPDATE SET
               sender_id = excluded.sender_id,
               protocol_version = excluded.protocol_version,
               task_id = excluded.task_id,
               task_summary = excluded.task_summary,
               dispatch_id = excluded.dispatch_id,
               updated_at_ms = excluded.updated_at_ms",
            params![
                &task.session_id,
                &task.sender_id,
                &task.protocol_version,
                &task.task_id,
                &task.task_summary,
                &task.dispatch_id,
                task.claimed_by_sender_id.as_deref(),
                task.lease_expires_at.map(dt_to_ms),
                dt_to_ms(task.enqueued_at),
                dt_to_ms(task.updated_at),
            ],
        )
        .map_err(|e| KernelError::Driver(format!("upsert a2a compat task: {}", e)))?;
        Ok(())
    }

    pub fn list_a2a_compat_tasks(
        &self,
        sender_id: &str,
        protocol_version: &str,
    ) -> Result<Vec<A2aCompatTaskRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT session_id, sender_id, protocol_version, task_id, task_summary, dispatch_id,
                        claimed_by_sender_id, lease_expires_at_ms, enqueued_at_ms, updated_at_ms
                 FROM runtime_a2a_compat_tasks
                 WHERE sender_id = ?1
                   AND protocol_version = ?2
                 ORDER BY enqueued_at_ms ASC, session_id ASC",
            )
            .map_err(|e| KernelError::Driver(format!("prepare list a2a compat tasks: {}", e)))?;
        let rows = stmt
            .query_map(
                params![sender_id, protocol_version],
                map_row_to_a2a_compat_task,
            )
            .map_err(|e| KernelError::Driver(format!("query list a2a compat tasks: {}", e)))?;
        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(
                row.map_err(|e| KernelError::Driver(format!("scan list a2a compat tasks: {}", e)))?,
            );
        }
        Ok(tasks)
    }

    pub fn claim_a2a_compat_task(
        &self,
        sender_id: &str,
        protocol_version: &str,
        now: DateTime<Utc>,
        lease_duration_ms: u64,
        requested_task_id: Option<&str>,
    ) -> Result<A2aCompatClaimOutcome, KernelError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| KernelError::Driver(format!("begin claim a2a compat task tx: {}", e)))?;
        let now_ms = dt_to_ms(now);
        let candidate = tx
            .query_row(
                "SELECT session_id, sender_id, protocol_version, task_id, task_summary, dispatch_id,
                        claimed_by_sender_id, lease_expires_at_ms, enqueued_at_ms, updated_at_ms
                 FROM runtime_a2a_compat_tasks
                 WHERE sender_id = ?1
                   AND protocol_version = ?2
                   AND (?4 IS NULL OR task_id = ?4)
                   AND (
                     claimed_by_sender_id IS NULL
                     OR lease_expires_at_ms IS NULL
                     OR lease_expires_at_ms <= ?3
                   )
                 ORDER BY enqueued_at_ms ASC, session_id ASC
                 LIMIT 1",
                params![sender_id, protocol_version, now_ms, requested_task_id],
                map_row_to_a2a_compat_task,
            )
            .optional()
            .map_err(|e| KernelError::Driver(format!("query claim a2a compat task: {}", e)))?;

        if let Some(mut task) = candidate {
            let reclaimed_expired_lease = task.claimed_by_sender_id.is_some()
                && task
                    .lease_expires_at
                    .map(|expires_at| expires_at <= now)
                    .unwrap_or(true);
            let lease_ms_i64 = i64::try_from(lease_duration_ms).unwrap_or(i64::MAX / 4);
            let lease_expires_at_ms = now_ms.saturating_add(lease_ms_i64.max(1));
            let updated = tx
                .execute(
                    "UPDATE runtime_a2a_compat_tasks
                     SET claimed_by_sender_id = ?2,
                         lease_expires_at_ms = ?3,
                         updated_at_ms = ?4
                     WHERE session_id = ?1
                       AND sender_id = ?5
                       AND protocol_version = ?6
                       AND (?7 IS NULL OR task_id = ?7)
                       AND (
                         claimed_by_sender_id IS NULL
                         OR lease_expires_at_ms IS NULL
                         OR lease_expires_at_ms <= ?4
                       )",
                    params![
                        &task.session_id,
                        sender_id,
                        lease_expires_at_ms,
                        now_ms,
                        sender_id,
                        protocol_version,
                        requested_task_id
                    ],
                )
                .map_err(|e| KernelError::Driver(format!("update claim a2a compat task: {}", e)))?;
            if updated > 0 {
                tx.commit().map_err(|e| {
                    KernelError::Driver(format!("commit claim a2a compat task: {}", e))
                })?;
                task.claimed_by_sender_id = Some(sender_id.to_string());
                task.lease_expires_at = Some(ms_to_dt(lease_expires_at_ms));
                task.updated_at = now;
                return Ok(A2aCompatClaimOutcome {
                    task: Some(task),
                    retry_after_ms: None,
                    reclaimed_expired_lease,
                });
            }
        }

        let retry_after_ms = tx
            .query_row(
                "SELECT MIN(lease_expires_at_ms - ?3)
                 FROM runtime_a2a_compat_tasks
                 WHERE sender_id = ?1
                   AND protocol_version = ?2
                   AND (?4 IS NULL OR task_id = ?4)
                   AND claimed_by_sender_id IS NOT NULL
                   AND lease_expires_at_ms > ?3",
                params![sender_id, protocol_version, now_ms, requested_task_id],
                |row| row.get::<_, Option<i64>>(0),
            )
            .map_err(|e| KernelError::Driver(format!("query claim retry_after a2a compat: {}", e)))?
            .and_then(|ms| if ms > 0 { u64::try_from(ms).ok() } else { None });

        tx.commit().map_err(|e| {
            KernelError::Driver(format!("commit claim miss a2a compat task: {}", e))
        })?;
        Ok(A2aCompatClaimOutcome {
            task: None,
            retry_after_ms,
            reclaimed_expired_lease: false,
        })
    }

    pub fn get_a2a_compat_task(
        &self,
        session_id: &str,
    ) -> Result<Option<A2aCompatTaskRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.query_row(
            "SELECT session_id, sender_id, protocol_version, task_id, task_summary, dispatch_id,
                    claimed_by_sender_id, lease_expires_at_ms, enqueued_at_ms, updated_at_ms
             FROM runtime_a2a_compat_tasks
             WHERE session_id = ?1",
            params![session_id],
            map_row_to_a2a_compat_task,
        )
        .optional()
        .map_err(|e| KernelError::Driver(format!("get a2a compat task: {}", e)))
    }

    pub fn touch_a2a_compat_task_lease(
        &self,
        session_id: &str,
        sender_id: &str,
        now: DateTime<Utc>,
        lease_duration_ms: u64,
    ) -> Result<bool, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let now_ms = dt_to_ms(now);
        let lease_ms_i64 = i64::try_from(lease_duration_ms).unwrap_or(i64::MAX / 4);
        let lease_expires_at_ms = now_ms.saturating_add(lease_ms_i64.max(1));
        let updated = conn
            .execute(
                "UPDATE runtime_a2a_compat_tasks
                 SET claimed_by_sender_id = ?2,
                     lease_expires_at_ms = ?3,
                     updated_at_ms = ?4
                 WHERE session_id = ?1
                   AND claimed_by_sender_id = ?2
                   AND lease_expires_at_ms IS NOT NULL
                   AND lease_expires_at_ms > ?4",
                params![session_id, sender_id, lease_expires_at_ms, now_ms],
            )
            .map_err(|e| KernelError::Driver(format!("touch a2a compat task lease: {}", e)))?;
        Ok(updated > 0)
    }

    pub fn remove_a2a_compat_task(&self, session_id: &str) -> Result<u64, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let removed = conn
            .execute(
                "DELETE FROM runtime_a2a_compat_tasks WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(|e| KernelError::Driver(format!("remove a2a compat task: {}", e)))?;
        Ok(removed as u64)
    }

    pub fn append_audit_log(&self, entry: &AuditLogEntry) -> Result<(), KernelError> {
        let now = dt_to_ms(Utc::now());
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_audit_logs
             (actor_type, actor_id, actor_role, action, resource_type, resource_id, result, request_id, details_json, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                entry.actor_type,
                entry.actor_id,
                entry.actor_role,
                entry.action,
                entry.resource_type,
                entry.resource_id,
                entry.result,
                entry.request_id,
                entry.details_json,
                now
            ],
        )
        .map_err(|e| KernelError::Driver(format!("append audit log: {}", e)))?;
        Ok(())
    }

    pub fn list_audit_logs(&self, limit: usize) -> Result<Vec<AuditLogRow>, KernelError> {
        self.list_audit_logs_filtered(None, None, None, None, limit)
    }

    pub fn list_audit_logs_filtered(
        &self,
        request_id: Option<&str>,
        action: Option<&str>,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        limit: usize,
    ) -> Result<Vec<AuditLogRow>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT audit_id, actor_type, actor_id, actor_role, action, resource_type, resource_id, result, request_id, details_json, created_at_ms
                 FROM runtime_audit_logs
                 WHERE (?1 IS NULL OR request_id = ?1)
                   AND (?2 IS NULL OR action = ?2)
                   AND (?3 IS NULL OR created_at_ms >= ?3)
                   AND (?4 IS NULL OR created_at_ms <= ?4)
                 ORDER BY audit_id DESC
                 LIMIT ?5",
            )
            .map_err(|e| KernelError::Driver(format!("prepare list_audit_logs: {}", e)))?;
        let rows = stmt
            .query_map(
                params![request_id, action, from_ms, to_ms, limit as i64],
                |row| {
                    Ok(AuditLogRow {
                        audit_id: row.get(0)?,
                        actor_type: row.get(1)?,
                        actor_id: row.get(2)?,
                        actor_role: row.get(3)?,
                        action: row.get(4)?,
                        resource_type: row.get(5)?,
                        resource_id: row.get(6)?,
                        result: row.get(7)?,
                        request_id: row.get(8)?,
                        details_json: row.get(9)?,
                        created_at: ms_to_dt(row.get::<_, i64>(10)?),
                    })
                },
            )
            .map_err(|e| KernelError::Driver(format!("query list_audit_logs: {}", e)))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(map_rusqlite_err)?);
        }
        Ok(out)
    }
}

fn map_row_to_interrupt(row: &rusqlite::Row) -> rusqlite::Result<InterruptRow> {
    Ok(InterruptRow {
        interrupt_id: row.get(0)?,
        thread_id: row.get(1)?,
        run_id: row.get(2)?,
        attempt_id: row.get(3)?,
        value_json: row.get(4)?,
        status: row.get(5)?,
        created_at: ms_to_dt(row.get::<_, i64>(6)?),
        resume_payload_hash: row.get(7)?,
        resume_response_json: row.get(8)?,
    })
}

fn map_row_to_dead_letter(row: &rusqlite::Row) -> rusqlite::Result<DeadLetterRow> {
    Ok(DeadLetterRow {
        attempt_id: row.get(0)?,
        run_id: row.get(1)?,
        attempt_no: row.get::<_, i64>(2)? as u32,
        terminal_status: row.get(3)?,
        reason: row.get(4)?,
        dead_at: ms_to_dt(row.get::<_, i64>(5)?),
        replay_status: row.get(6)?,
        replay_count: row.get::<_, i64>(7)? as u32,
        last_replayed_at: row.get::<_, Option<i64>>(8)?.map(ms_to_dt),
    })
}

fn map_row_to_replay_effect_log(row: &rusqlite::Row) -> rusqlite::Result<ReplayEffectLogRow> {
    Ok(ReplayEffectLogRow {
        fingerprint: row.get(0)?,
        thread_id: row.get(1)?,
        replay_target: row.get(2)?,
        effect_type: row.get(3)?,
        status: row.get(4)?,
        execution_count: row.get::<_, i64>(5)? as u32,
        created_at: ms_to_dt(row.get::<_, i64>(6)?),
        completed_at: row.get::<_, Option<i64>>(7)?.map(ms_to_dt),
    })
}

fn map_row_to_a2a_compat_task(row: &rusqlite::Row) -> rusqlite::Result<A2aCompatTaskRow> {
    Ok(A2aCompatTaskRow {
        session_id: row.get(0)?,
        sender_id: row.get(1)?,
        protocol_version: row.get(2)?,
        task_id: row.get(3)?,
        task_summary: row.get(4)?,
        dispatch_id: row.get(5)?,
        claimed_by_sender_id: row.get(6)?,
        lease_expires_at: row.get::<_, Option<i64>>(7)?.map(ms_to_dt),
        enqueued_at: ms_to_dt(row.get::<_, i64>(8)?),
        updated_at: ms_to_dt(row.get::<_, i64>(9)?),
    })
}

fn map_row_to_bounty(row: &rusqlite::Row) -> rusqlite::Result<BountyRow> {
    Ok(BountyRow {
        bounty_id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        reward: row.get(3)?,
        status: row.get(4)?,
        created_by: row.get(5)?,
        created_at: ms_to_dt(row.get::<_, i64>(6)?),
        closed_at: row.get::<_, Option<i64>>(7)?.map(ms_to_dt),
        accepted_by: row.get(8)?,
        accepted_at: row.get::<_, Option<i64>>(9)?.map(ms_to_dt),
    })
}

fn map_row_to_swarm_task(row: &rusqlite::Row) -> rusqlite::Result<SwarmTaskRow> {
    Ok(SwarmTaskRow {
        parent_task_id: row.get(0)?,
        decomposition_json: row.get(1)?,
        proposer_id: row.get(2)?,
        proposer_reward_pct: row.get(3)?,
        solver_reward_pct: row.get(4)?,
        aggregator_reward_pct: row.get(5)?,
        status: row.get(6)?,
        created_at: ms_to_dt(row.get::<_, i64>(7)?),
        completed_at: row.get::<_, Option<i64>>(8)?.map(ms_to_dt),
    })
}

fn map_row_to_worker_registry(row: &rusqlite::Row) -> rusqlite::Result<WorkerRegistryRow> {
    Ok(WorkerRegistryRow {
        worker_id: row.get(0)?,
        domains_json: row.get(1)?,
        max_load: row.get(2)?,
        metadata_json: row.get(3)?,
        registered_at: ms_to_dt(row.get::<_, i64>(4)?),
        last_heartbeat_at: row.get::<_, Option<i64>>(5)?.map(ms_to_dt),
        status: row.get(6)?,
    })
}

fn map_row_to_dispute(row: &rusqlite::Row) -> rusqlite::Result<DisputeRow> {
    Ok(DisputeRow {
        dispute_id: row.get(0)?,
        bounty_id: row.get(1)?,
        opened_by: row.get(2)?,
        status: row.get(3)?,
        resolution: row.get(4)?,
        resolved_by: row.get(5)?,
        created_at: ms_to_dt(row.get::<_, i64>(6)?),
        resolved_at: row.get::<_, Option<i64>>(7)?.map(ms_to_dt),
        evidence_json: row.get(8)?,
    })
}

#[derive(Clone, Debug)]
pub struct InterruptRow {
    pub interrupt_id: String,
    pub thread_id: String,
    pub run_id: String,
    pub attempt_id: String,
    pub value_json: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub resume_payload_hash: Option<String>,
    pub resume_response_json: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ApiKeyRow {
    pub key_id: String,
    pub secret_hash: String,
    pub role: String,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct A2aSessionRow {
    pub sender_id: String,
    pub protocol: String,
    pub protocol_version: String,
    pub enabled_capabilities_json: String,
    pub actor_type: Option<String>,
    pub actor_id: Option<String>,
    pub actor_role: Option<String>,
    pub negotiated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BountyRow {
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
pub struct SwarmTaskRow {
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
pub struct WorkerRegistryRow {
    pub worker_id: String,
    pub domains_json: String,
    pub max_load: i32,
    pub metadata_json: Option<String>,
    pub registered_at: DateTime<Utc>,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisputeRow {
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
pub struct RecipeRow {
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
pub struct OrganismRow {
    pub organism_id: String,
    pub recipe_id: String,
    pub status: String,
    pub current_step: i32,
    pub total_steps: i32,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct A2aCompatTaskRow {
    pub session_id: String,
    pub sender_id: String,
    pub protocol_version: String,
    pub task_id: String,
    pub task_summary: String,
    pub dispatch_id: String,
    pub claimed_by_sender_id: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub enqueued_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct A2aCompatClaimOutcome {
    pub task: Option<A2aCompatTaskRow>,
    pub retry_after_ms: Option<u64>,
    pub reclaimed_expired_lease: bool,
}

#[derive(Clone, Debug)]
pub struct AuditLogEntry {
    pub actor_type: String,
    pub actor_id: Option<String>,
    pub actor_role: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub result: String,
    pub request_id: String,
    pub details_json: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AuditLogRow {
    pub audit_id: i64,
    pub actor_type: String,
    pub actor_id: Option<String>,
    pub actor_role: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub result: String,
    pub request_id: String,
    pub details_json: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepReportWriteResult {
    Inserted,
    Duplicate,
}

impl RuntimeRepository for SqliteRuntimeRepository {
    fn list_dispatchable_attempts(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<AttemptDispatchRecord>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT a.attempt_id, a.run_id, a.attempt_no, a.status, a.retry_at_ms
                 FROM runtime_attempts a
                 LEFT JOIN runtime_leases l ON l.attempt_id = a.attempt_id AND l.lease_expires_at_ms >= ?1
                 WHERE l.attempt_id IS NULL
                   AND (
                     a.status = 'queued'
                     OR (a.status = 'retry_backoff' AND (a.retry_at_ms IS NULL OR a.retry_at_ms <= ?1))
                   )
                 ORDER BY a.priority DESC, a.attempt_no ASC, a.attempt_id ASC
                 LIMIT ?2",
            )
            .map_err(|e| KernelError::Driver(format!("prepare list dispatchable attempts: {}", e)))?;
        let rows = stmt
            .query_map(params![dt_to_ms(now), limit as i64], |row| {
                let retry_at_ms: Option<i64> = row.get(4)?;
                Ok(AttemptDispatchRecord {
                    attempt_id: row.get(0)?,
                    run_id: row.get(1)?,
                    attempt_no: row.get::<_, i64>(2)? as u32,
                    status: parse_attempt_status(&row.get::<_, String>(3)?),
                    retry_at: retry_at_ms.map(ms_to_dt),
                })
            })
            .map_err(|e| KernelError::Driver(format!("query dispatchable attempts: {}", e)))?;
        let mut out = Vec::new();
        for item in rows {
            out.push(item.map_err(map_rusqlite_err)?);
        }
        Ok(out)
    }

    fn upsert_lease(
        &self,
        attempt_id: &str,
        worker_id: &str,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<LeaseRecord, KernelError> {
        let now = Utc::now();
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| KernelError::Driver(format!("begin upsert lease tx: {}", e)))?;
        let lease_id = format!("lease-{}", uuid::Uuid::new_v4());
        tx.execute(
            "DELETE FROM runtime_leases WHERE attempt_id = ?1 AND lease_expires_at_ms < ?2",
            params![attempt_id, dt_to_ms(now)],
        )
        .map_err(|e| KernelError::Driver(format!("cleanup expired lease: {}", e)))?;
        match tx.execute(
            "INSERT INTO runtime_leases
             (lease_id, attempt_id, worker_id, lease_expires_at_ms, heartbeat_at_ms, version)
             VALUES (?1, ?2, ?3, ?4, ?5, 1)",
            params![
                lease_id,
                attempt_id,
                worker_id,
                dt_to_ms(lease_expires_at),
                dt_to_ms(now)
            ],
        ) {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == ErrorCode::ConstraintViolation =>
            {
                return Err(KernelError::Driver(format!(
                    "active lease already exists for attempt: {}",
                    attempt_id
                )));
            }
            Err(e) => return Err(KernelError::Driver(format!("insert lease: {}", e))),
        };
        let updated_attempt = tx
            .execute(
                "UPDATE runtime_attempts
                 SET status = 'leased',
                     started_at_ms = COALESCE(started_at_ms, ?2)
                 WHERE attempt_id = ?1 AND status IN ('queued', 'retry_backoff')",
                params![attempt_id, dt_to_ms(now)],
            )
            .map_err(|e| KernelError::Driver(format!("mark leased status: {}", e)))?;
        if updated_attempt == 0 {
            return Err(KernelError::Driver(format!(
                "attempt is not dispatchable for lease: {}",
                attempt_id
            )));
        }
        let version: i64 = tx
            .query_row(
                "SELECT version FROM runtime_leases WHERE attempt_id = ?1",
                params![attempt_id],
                |r| r.get(0),
            )
            .map_err(|e| KernelError::Driver(format!("read lease version: {}", e)))?;
        tx.commit()
            .map_err(|e| KernelError::Driver(format!("commit upsert lease tx: {}", e)))?;
        Ok(LeaseRecord {
            lease_id,
            attempt_id: attempt_id.to_string(),
            worker_id: worker_id.to_string(),
            lease_expires_at,
            heartbeat_at: now,
            version: version as u64,
            terminal_state: None,
            terminal_at: None,
        })
    }

    fn heartbeat_lease(
        &self,
        lease_id: &str,
        heartbeat_at: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let updated = conn
            .execute(
                "UPDATE runtime_leases SET heartbeat_at_ms = ?2, lease_expires_at_ms = ?3, version = version + 1 WHERE lease_id = ?1",
                params![lease_id, dt_to_ms(heartbeat_at), dt_to_ms(lease_expires_at)],
            )
            .map_err(|e| KernelError::Driver(format!("heartbeat lease: {}", e)))?;
        if updated == 0 {
            return Err(KernelError::Driver(format!(
                "lease not found for heartbeat: {}",
                lease_id
            )));
        }
        Ok(())
    }

    fn expire_leases_and_requeue(&self, stale_before: DateTime<Utc>) -> Result<u64, KernelError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| KernelError::Driver(format!("begin expire/requeue tx: {}", e)))?;
        let mut stmt = tx
            .prepare(
                "SELECT attempt_id
                 FROM runtime_leases
                 WHERE lease_expires_at_ms < ?1",
            )
            .map_err(|e| KernelError::Driver(format!("prepare expired lease query: {}", e)))?;
        let rows = stmt
            .query_map(params![dt_to_ms(stale_before)], |r| r.get::<_, String>(0))
            .map_err(|e| KernelError::Driver(format!("query expired leases: {}", e)))?;
        let mut expired_attempts = Vec::new();
        for row in rows {
            expired_attempts.push(row.map_err(map_rusqlite_err)?);
        }
        drop(stmt);
        for attempt_id in &expired_attempts {
            tx.execute(
                "DELETE FROM runtime_leases WHERE attempt_id = ?1",
                params![attempt_id],
            )
            .map_err(|e| KernelError::Driver(format!("delete expired lease: {}", e)))?;
            tx.execute(
                "UPDATE runtime_attempts
                 SET status = 'queued'
                 WHERE attempt_id = ?1
                   AND status NOT IN ('completed', 'failed', 'cancelled')",
                params![attempt_id],
            )
            .map_err(|e| KernelError::Driver(format!("requeue attempt: {}", e)))?;
        }
        tx.commit()
            .map_err(|e| KernelError::Driver(format!("commit expire/requeue tx: {}", e)))?;
        Ok(expired_attempts.len() as u64)
    }

    fn transition_timed_out_attempts(&self, now: DateTime<Utc>) -> Result<u64, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT attempt_id, run_id, attempt_no, timeout_terminal_status
                 FROM runtime_attempts
                 WHERE started_at_ms IS NOT NULL
                   AND execution_timeout_ms IS NOT NULL
                   AND timeout_terminal_status IS NOT NULL
                   AND status IN ('leased', 'running')
                   AND (started_at_ms + execution_timeout_ms) <= ?1",
            )
            .map_err(|e| KernelError::Driver(format!("prepare timed-out attempts query: {}", e)))?;
        let rows = stmt
            .query_map(params![dt_to_ms(now)], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| KernelError::Driver(format!("query timed-out attempts: {}", e)))?;
        let mut timed_out = Vec::new();
        for row in rows {
            timed_out.push(row.map_err(map_rusqlite_err)?);
        }
        for (attempt_id, run_id, attempt_no, terminal_status) in &timed_out {
            conn.execute(
                "DELETE FROM runtime_leases WHERE attempt_id = ?1",
                params![attempt_id],
            )
            .map_err(|e| KernelError::Driver(format!("delete timed-out lease: {}", e)))?;
            conn.execute(
                "UPDATE runtime_attempts
                 SET status = ?2,
                     retry_at_ms = NULL,
                     started_at_ms = NULL
                 WHERE attempt_id = ?1",
                params![attempt_id, terminal_status],
            )
            .map_err(|e| KernelError::Driver(format!("mark timed-out attempt status: {}", e)))?;
            if terminal_status == "failed" {
                conn.execute(
                    "INSERT INTO runtime_dead_letters
                     (attempt_id, run_id, attempt_no, terminal_status, reason, dead_at_ms, replay_status, replay_count, last_replayed_at_ms)
                     VALUES (?1, ?2, ?3, 'failed', ?4, ?5, 'pending', 0, NULL)
                     ON CONFLICT(attempt_id) DO UPDATE SET
                       run_id = excluded.run_id,
                       attempt_no = excluded.attempt_no,
                       terminal_status = excluded.terminal_status,
                       reason = excluded.reason,
                       dead_at_ms = excluded.dead_at_ms,
                       replay_status = 'pending'",
                    params![attempt_id, run_id, attempt_no, "execution_timeout", dt_to_ms(now)],
                )
                .map_err(|e| KernelError::Driver(format!("upsert dead letter from timeout: {}", e)))?;
            }
        }
        Ok(timed_out.len() as u64)
    }

    fn latest_seq_for_run(&self, _run_id: &RunId) -> Result<Seq, KernelError> {
        Ok(0)
    }

    // ============== Bounty Methods ==============

    fn upsert_bounty(&self, bounty: &BountyRecord) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_bounties (bounty_id, title, description, reward, status, created_by, created_at_ms, closed_at_ms, accepted_by, accepted_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(bounty_id) DO UPDATE SET
               title = excluded.title,
               description = excluded.description,
               reward = excluded.reward,
               status = excluded.status,
               closed_at_ms = excluded.closed_at_ms,
               accepted_by = excluded.accepted_by,
               accepted_at_ms = excluded.accepted_at_ms",
            params![
                bounty.bounty_id,
                bounty.title,
                bounty.description,
                bounty.reward,
                bounty.status.as_str(),
                bounty.created_by,
                bounty.created_at_ms,
                bounty.closed_at_ms,
                bounty.accepted_by,
                bounty.accepted_at_ms,
            ],
        )
        .map_err(|e| KernelError::Driver(format!("upsert bounty: {}", e)))?;
        Ok(())
    }

    fn get_bounty(&self, bounty_id: &str) -> Result<Option<BountyRecord>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let result = conn.query_row(
            "SELECT bounty_id, title, description, reward, status, created_by, created_at_ms, closed_at_ms, accepted_by, accepted_at_ms
             FROM runtime_bounties WHERE bounty_id = ?1",
            params![bounty_id],
            |r| {
                Ok(BountyRecord {
                    bounty_id: r.get(0)?,
                    title: r.get(1)?,
                    description: r.get(2)?,
                    reward: r.get(3)?,
                    status: BountyStatus::from_str(&r.get::<_, String>(4)?),
                    created_by: r.get(5)?,
                    created_at_ms: r.get(6)?,
                    closed_at_ms: r.get(7)?,
                    accepted_by: r.get(8)?,
                    accepted_at_ms: r.get(9)?,
                })
            },
        );
        match result {
            Ok(bounty) => Ok(Some(bounty)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(KernelError::Driver(format!("get bounty: {}", e))),
        }
    }

    fn list_bounties(
        &self,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Vec<BountyRecord>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;

        let bounties: Vec<BountyRecord> = match status {
            Some(s) => {
                let mut stmt = conn.prepare(
                    "SELECT bounty_id, title, description, reward, status, created_by, created_at_ms, closed_at_ms, accepted_by, accepted_at_ms FROM runtime_bounties WHERE status = ?1 ORDER BY created_at_ms DESC LIMIT ?2"
                ).map_err(|e| KernelError::Driver(format!("prepare list bounties: {}", e)))?;
                let x = stmt
                    .query_map(params![s, limit as i64], |r| {
                        Ok(BountyRecord {
                            bounty_id: r.get(0)?,
                            title: r.get(1)?,
                            description: r.get(2)?,
                            reward: r.get(3)?,
                            status: BountyStatus::from_str(&r.get::<_, String>(4)?),
                            created_by: r.get(5)?,
                            created_at_ms: r.get(6)?,
                            closed_at_ms: r.get(7)?,
                            accepted_by: r.get(8)?,
                            accepted_at_ms: r.get(9)?,
                        })
                    })
                    .map_err(|e| KernelError::Driver(format!("query bounties: {}", e)))?
                    .filter_map(|r| r.ok())
                    .collect();
                x
            }
            None => {
                let mut stmt = conn.prepare(
                    "SELECT bounty_id, title, description, reward, status, created_by, created_at_ms, closed_at_ms, accepted_by, accepted_at_ms FROM runtime_bounties ORDER BY created_at_ms DESC LIMIT ?1"
                ).map_err(|e| KernelError::Driver(format!("prepare list bounties: {}", e)))?;
                let x = stmt
                    .query_map(params![limit as i64], |r| {
                        Ok(BountyRecord {
                            bounty_id: r.get(0)?,
                            title: r.get(1)?,
                            description: r.get(2)?,
                            reward: r.get(3)?,
                            status: BountyStatus::from_str(&r.get::<_, String>(4)?),
                            created_by: r.get(5)?,
                            created_at_ms: r.get(6)?,
                            closed_at_ms: r.get(7)?,
                            accepted_by: r.get(8)?,
                            accepted_at_ms: r.get(9)?,
                        })
                    })
                    .map_err(|e| KernelError::Driver(format!("query bounties: {}", e)))?
                    .filter_map(|r| r.ok())
                    .collect();
                x
            }
        };
        Ok(bounties)
    }

    fn accept_bounty(&self, bounty_id: &str, accepted_by: &str) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let now = Utc::now().timestamp_millis();
        let updated = conn
            .execute(
                "UPDATE runtime_bounties SET status = 'accepted', accepted_by = ?2, accepted_at_ms = ?3 WHERE bounty_id = ?1 AND status = 'open'",
                params![bounty_id, accepted_by, now],
            )
            .map_err(|e| KernelError::Driver(format!("accept bounty: {}", e)))?;
        if updated == 0 {
            return Err(KernelError::Driver(format!(
                "bounty not found or not in open status: {}",
                bounty_id
            )));
        }
        Ok(())
    }

    fn close_bounty(&self, bounty_id: &str) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let now = Utc::now().timestamp_millis();
        let updated = conn
            .execute(
                "UPDATE runtime_bounties SET status = 'closed', closed_at_ms = ?2 WHERE bounty_id = ?1 AND status IN ('open', 'accepted')",
                params![bounty_id, now],
            )
            .map_err(|e| KernelError::Driver(format!("close bounty: {}", e)))?;
        if updated == 0 {
            return Err(KernelError::Driver(format!(
                "bounty not found or already closed: {}",
                bounty_id
            )));
        }
        Ok(())
    }

    // ============== Swarm Methods ==============

    fn upsert_swarm_decomposition(&self, task: &SwarmTaskRecord) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_swarm_tasks (parent_task_id, decomposition_json, proposer_id, proposer_reward_pct, solver_reward_pct, aggregator_reward_pct, status, created_at_ms, completed_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(parent_task_id) DO UPDATE SET
               decomposition_json = excluded.decomposition_json,
               status = excluded.status,
               completed_at_ms = excluded.completed_at_ms",
            params![
                task.parent_task_id,
                task.decomposition_json,
                task.proposer_id,
                task.proposer_reward_pct,
                task.solver_reward_pct,
                task.aggregator_reward_pct,
                task.status,
                task.created_at_ms,
                task.completed_at_ms,
            ],
        )
        .map_err(|e| KernelError::Driver(format!("upsert swarm: {}", e)))?;
        Ok(())
    }

    fn get_swarm_decomposition(
        &self,
        parent_task_id: &str,
    ) -> Result<Option<SwarmTaskRecord>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let result = conn.query_row(
            "SELECT parent_task_id, decomposition_json, proposer_id, proposer_reward_pct, solver_reward_pct, aggregator_reward_pct, status, created_at_ms, completed_at_ms
             FROM runtime_swarm_tasks WHERE parent_task_id = ?1",
            params![parent_task_id],
            |r| {
                Ok(SwarmTaskRecord {
                    parent_task_id: r.get(0)?,
                    decomposition_json: r.get(1)?,
                    proposer_id: r.get(2)?,
                    proposer_reward_pct: r.get(3)?,
                    solver_reward_pct: r.get(4)?,
                    aggregator_reward_pct: r.get(5)?,
                    status: r.get(6)?,
                    created_at_ms: r.get(7)?,
                    completed_at_ms: r.get(8)?,
                })
            },
        );
        match result {
            Ok(task) => Ok(Some(task)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(KernelError::Driver(format!("get swarm: {}", e))),
        }
    }

    // ============== Worker Methods ==============

    fn register_worker(&self, worker: &WorkerRecord) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_workers_registry (worker_id, domains, max_load, metadata_json, registered_at_ms, last_heartbeat_ms, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(worker_id) DO UPDATE SET
               domains = excluded.domains,
               max_load = excluded.max_load,
               metadata_json = excluded.metadata_json,
               last_heartbeat_ms = excluded.last_heartbeat_ms,
               status = excluded.status",
            params![
                worker.worker_id,
                worker.domains,
                worker.max_load,
                worker.metadata_json,
                worker.registered_at_ms,
                worker.last_heartbeat_ms,
                worker.status,
            ],
        )
        .map_err(|e| KernelError::Driver(format!("register worker: {}", e)))?;
        Ok(())
    }

    fn get_worker(&self, worker_id: &str) -> Result<Option<WorkerRecord>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let result = conn.query_row(
            "SELECT worker_id, domains, max_load, metadata_json, registered_at_ms, last_heartbeat_ms, status
             FROM runtime_workers_registry WHERE worker_id = ?1",
            params![worker_id],
            |r| {
                Ok(WorkerRecord {
                    worker_id: r.get(0)?,
                    domains: r.get(1)?,
                    max_load: r.get(2)?,
                    metadata_json: r.get(3)?,
                    registered_at_ms: r.get(4)?,
                    last_heartbeat_ms: r.get(5)?,
                    status: r.get(6)?,
                })
            },
        );
        match result {
            Ok(worker) => Ok(Some(worker)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(KernelError::Driver(format!("get worker: {}", e))),
        }
    }

    fn list_workers(
        &self,
        domain: Option<&str>,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Vec<WorkerRecord>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let (sql, params): (String, Vec<Box<dyn rusqlite::ToSql>>) = match (&domain, &status) {
            (Some(d), Some(s)) => (
                "SELECT worker_id, domains, max_load, metadata_json, registered_at_ms, last_heartbeat_ms, status
                 FROM runtime_workers_registry WHERE domains LIKE ?1 AND status = ?2 ORDER BY registered_at_ms DESC LIMIT ?3".to_string(),
                vec![Box::new(format!("%{}%", d)), Box::new(s.to_string()), Box::new(limit as i64)]
            ),
            (Some(d), None) => (
                "SELECT worker_id, domains, max_load, metadata_json, registered_at_ms, last_heartbeat_ms, status
                 FROM runtime_workers_registry WHERE domains LIKE ?1 ORDER BY registered_at_ms DESC LIMIT ?2".to_string(),
                vec![Box::new(format!("%{}%", d)), Box::new(limit as i64)]
            ),
            (None, Some(s)) => (
                "SELECT worker_id, domains, max_load, metadata_json, registered_at_ms, last_heartbeat_ms, status
                 FROM runtime_workers_registry WHERE status = ?1 ORDER BY registered_at_ms DESC LIMIT ?2".to_string(),
                vec![Box::new(s.to_string()), Box::new(limit as i64)]
            ),
            (None, None) => (
                "SELECT worker_id, domains, max_load, metadata_json, registered_at_ms, last_heartbeat_ms, status
                 FROM runtime_workers_registry ORDER BY registered_at_ms DESC LIMIT ?1".to_string(),
                vec![Box::new(limit as i64)]
            ),
        };
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| KernelError::Driver(format!("prepare list workers: {}", e)))?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let workers = stmt
            .query_map(params_refs.as_slice(), |r| {
                Ok(WorkerRecord {
                    worker_id: r.get(0)?,
                    domains: r.get(1)?,
                    max_load: r.get(2)?,
                    metadata_json: r.get(3)?,
                    registered_at_ms: r.get(4)?,
                    last_heartbeat_ms: r.get(5)?,
                    status: r.get(6)?,
                })
            })
            .map_err(|e| KernelError::Driver(format!("query workers: {}", e)))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(workers)
    }

    fn heartbeat_worker(&self, worker_id: &str, heartbeat_at_ms: i64) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let updated = conn
            .execute(
                "UPDATE runtime_workers_registry SET last_heartbeat_ms = ?2, status = 'active' WHERE worker_id = ?1",
                params![worker_id, heartbeat_at_ms],
            )
            .map_err(|e| KernelError::Driver(format!("heartbeat worker: {}", e)))?;
        if updated == 0 {
            return Err(KernelError::Driver(format!(
                "worker not found: {}",
                worker_id
            )));
        }
        Ok(())
    }

    // ============== Recipe Methods ==============

    fn create_recipe(&self, recipe: &RecipeRecord) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_recipes (recipe_id, name, description, gene_sequence_json, author_id, forked_from, created_at_ms, updated_at_ms, is_public)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                recipe.recipe_id,
                recipe.name,
                recipe.description,
                recipe.gene_sequence_json,
                recipe.author_id,
                recipe.forked_from,
                recipe.created_at_ms,
                recipe.updated_at_ms,
                recipe.is_public as i32,
            ],
        )
        .map_err(|e| KernelError::Driver(format!("create recipe: {}", e)))?;
        Ok(())
    }

    fn get_recipe(&self, recipe_id: &str) -> Result<Option<RecipeRecord>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let result = conn.query_row(
            "SELECT recipe_id, name, description, gene_sequence_json, author_id, forked_from, created_at_ms, updated_at_ms, is_public
             FROM runtime_recipes WHERE recipe_id = ?1",
            params![recipe_id],
            |r| {
                Ok(RecipeRecord {
                    recipe_id: r.get(0)?,
                    name: r.get(1)?,
                    description: r.get(2)?,
                    gene_sequence_json: r.get(3)?,
                    author_id: r.get(4)?,
                    forked_from: r.get(5)?,
                    created_at_ms: r.get(6)?,
                    updated_at_ms: r.get(7)?,
                    is_public: r.get::<_, i32>(8)? != 0,
                })
            },
        );
        match result {
            Ok(recipe) => Ok(Some(recipe)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(KernelError::Driver(format!("get recipe: {}", e))),
        }
    }

    fn fork_recipe(
        &self,
        original_id: &str,
        new_id: &str,
        new_author: &str,
    ) -> Result<Option<RecipeRecord>, KernelError> {
        let original = self.get_recipe(original_id)?;
        if let Some(orig) = original {
            let conn = self.conn.lock().map_err(|_| {
                KernelError::Driver("sqlite runtime repo lock poisoned".to_string())
            })?;
            let now = Utc::now().timestamp_millis();
            conn.execute(
                "INSERT INTO runtime_recipes (recipe_id, name, description, gene_sequence_json, author_id, forked_from, created_at_ms, updated_at_ms, is_public)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    new_id,
                    format!("Fork of {}", orig.name),
                    orig.description,
                    orig.gene_sequence_json,
                    new_author,
                    Some(original_id),
                    now,
                    now,
                    orig.is_public as i32,
                ],
            )
            .map_err(|e| KernelError::Driver(format!("fork recipe: {}", e)))?;
            Ok(Some(RecipeRecord {
                recipe_id: new_id.to_string(),
                name: format!("Fork of {}", orig.name),
                description: orig.description,
                gene_sequence_json: orig.gene_sequence_json,
                author_id: new_author.to_string(),
                forked_from: Some(original_id.to_string()),
                created_at_ms: now,
                updated_at_ms: now,
                is_public: orig.is_public,
            }))
        } else {
            Ok(None)
        }
    }

    fn list_recipes(
        &self,
        author_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<RecipeRecord>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;

        let rows: Vec<RecipeRecord> = match author_id {
            Some(aid) => {
                let mut stmt = conn.prepare(
                    "SELECT recipe_id, name, description, gene_sequence_json, author_id, forked_from, created_at_ms, updated_at_ms, is_public FROM runtime_recipes WHERE author_id = ?1 ORDER BY created_at_ms DESC LIMIT ?2"
                ).map_err(|e| KernelError::Driver(format!("prepare list recipes: {}", e)))?;
                let result: Vec<RecipeRecord> = stmt
                    .query_map(params![aid, limit as i64], |r| {
                        Ok(RecipeRecord {
                            recipe_id: r.get(0)?,
                            name: r.get(1)?,
                            description: r.get(2)?,
                            gene_sequence_json: r.get(3)?,
                            author_id: r.get(4)?,
                            forked_from: r.get(5)?,
                            created_at_ms: r.get(6)?,
                            updated_at_ms: r.get(7)?,
                            is_public: r.get::<_, i32>(8)? != 0,
                        })
                    })
                    .map_err(|e| KernelError::Driver(format!("query recipes: {}", e)))?
                    .filter_map(|r| r.ok())
                    .collect();
                result
            }
            None => {
                let mut stmt = conn.prepare(
                    "SELECT recipe_id, name, description, gene_sequence_json, author_id, forked_from, created_at_ms, updated_at_ms, is_public FROM runtime_recipes ORDER BY created_at_ms DESC LIMIT ?1"
                ).map_err(|e| KernelError::Driver(format!("prepare list recipes: {}", e)))?;
                let result: Vec<RecipeRecord> = stmt
                    .query_map(params![limit as i64], |r| {
                        Ok(RecipeRecord {
                            recipe_id: r.get(0)?,
                            name: r.get(1)?,
                            description: r.get(2)?,
                            gene_sequence_json: r.get(3)?,
                            author_id: r.get(4)?,
                            forked_from: r.get(5)?,
                            created_at_ms: r.get(6)?,
                            updated_at_ms: r.get(7)?,
                            is_public: r.get::<_, i32>(8)? != 0,
                        })
                    })
                    .map_err(|e| KernelError::Driver(format!("query recipes: {}", e)))?
                    .filter_map(|r| r.ok())
                    .collect();
                result
            }
        };
        Ok(rows)
    }

    // ============== Organism Methods ==============

    fn express_organism(&self, organism: &OrganismRecord) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_organisms (organism_id, recipe_id, status, current_step, total_steps, created_at_ms, completed_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                organism.organism_id,
                organism.recipe_id,
                organism.status,
                organism.current_step,
                organism.total_steps,
                organism.created_at_ms,
                organism.completed_at_ms,
            ],
        )
        .map_err(|e| KernelError::Driver(format!("express organism: {}", e)))?;
        Ok(())
    }

    fn get_organism(&self, organism_id: &str) -> Result<Option<OrganismRecord>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let result = conn.query_row(
            "SELECT organism_id, recipe_id, status, current_step, total_steps, created_at_ms, completed_at_ms
             FROM runtime_organisms WHERE organism_id = ?1",
            params![organism_id],
            |r| {
                Ok(OrganismRecord {
                    organism_id: r.get(0)?,
                    recipe_id: r.get(1)?,
                    status: r.get(2)?,
                    current_step: r.get(3)?,
                    total_steps: r.get(4)?,
                    created_at_ms: r.get(5)?,
                    completed_at_ms: r.get(6)?,
                })
            },
        );
        match result {
            Ok(organism) => Ok(Some(organism)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(KernelError::Driver(format!("get organism: {}", e))),
        }
    }

    fn update_organism(
        &self,
        organism_id: &str,
        current_step: i32,
        status: &str,
    ) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let now = Utc::now().timestamp_millis();
        let completed_at_ms: Option<i64> = if status == "completed" {
            Some(now)
        } else {
            None
        };
        let updated = conn
            .execute(
                "UPDATE runtime_organisms SET current_step = ?2, status = ?3, completed_at_ms = ?4 WHERE organism_id = ?1",
                params![organism_id, current_step, status, completed_at_ms],
            )
            .map_err(|e| KernelError::Driver(format!("update organism: {}", e)))?;
        if updated == 0 {
            return Err(KernelError::Driver(format!(
                "organism not found: {}",
                organism_id
            )));
        }
        Ok(())
    }

    // ============== Session Methods ==============

    fn create_session(&self, session: &SessionRecord) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_collab_sessions (session_id, session_type, creator_id, status, created_at_ms, ended_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                session.session_id,
                session.session_type,
                session.creator_id,
                session.status,
                session.created_at_ms,
                session.ended_at_ms,
            ],
        )
        .map_err(|e| KernelError::Driver(format!("create session: {}", e)))?;
        Ok(())
    }

    fn get_session(&self, session_id: &str) -> Result<Option<SessionRecord>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let result = conn.query_row(
            "SELECT session_id, session_type, creator_id, status, created_at_ms, ended_at_ms
             FROM runtime_collab_sessions WHERE session_id = ?1",
            params![session_id],
            |r| {
                Ok(SessionRecord {
                    session_id: r.get(0)?,
                    session_type: r.get(1)?,
                    creator_id: r.get(2)?,
                    status: r.get(3)?,
                    created_at_ms: r.get(4)?,
                    ended_at_ms: r.get(5)?,
                })
            },
        );
        match result {
            Ok(session) => Ok(Some(session)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(KernelError::Driver(format!("get session: {}", e))),
        }
    }

    fn add_session_message(&self, message: &SessionMessageRecord) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_collab_messages (message_id, session_id, sender_id, content, message_type, sent_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                message.message_id,
                message.session_id,
                message.sender_id,
                message.content,
                message.message_type,
                message.sent_at_ms,
            ],
        )
        .map_err(|e| KernelError::Driver(format!("add session message: {}", e)))?;
        Ok(())
    }

    fn get_session_history(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<SessionMessageRecord>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT message_id, session_id, sender_id, content, message_type, sent_at_ms
             FROM runtime_collab_messages WHERE session_id = ?1 ORDER BY sent_at_ms DESC LIMIT ?2",
            )
            .map_err(|e| KernelError::Driver(format!("prepare session history: {}", e)))?;
        let messages = stmt
            .query_map(params![session_id, limit as i64], |r| {
                Ok(SessionMessageRecord {
                    message_id: r.get(0)?,
                    session_id: r.get(1)?,
                    sender_id: r.get(2)?,
                    content: r.get(3)?,
                    message_type: r.get(4)?,
                    sent_at_ms: r.get(5)?,
                })
            })
            .map_err(|e| KernelError::Driver(format!("query session history: {}", e)))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(messages)
    }

    // ============== Dispute Methods ==============

    fn open_dispute(&self, dispute: &DisputeRecord) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO runtime_disputes (dispute_id, bounty_id, opened_by, status, evidence_json, resolution, resolved_by, resolved_at_ms, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                dispute.dispute_id,
                dispute.bounty_id,
                dispute.opened_by,
                dispute.status.as_str(),
                dispute.evidence_json,
                dispute.resolution,
                dispute.resolved_by,
                dispute.resolved_at_ms,
                dispute.created_at_ms,
            ],
        )
        .map_err(|e| KernelError::Driver(format!("open dispute: {}", e)))?;
        Ok(())
    }

    fn get_dispute(&self, dispute_id: &str) -> Result<Option<DisputeRecord>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let result = conn.query_row(
            "SELECT dispute_id, bounty_id, opened_by, status, evidence_json, resolution, resolved_by, resolved_at_ms, created_at_ms
             FROM runtime_disputes WHERE dispute_id = ?1",
            params![dispute_id],
            |r| {
                Ok(DisputeRecord {
                    dispute_id: r.get(0)?,
                    bounty_id: r.get(1)?,
                    opened_by: r.get(2)?,
                    status: DisputeStatus::from_str(&r.get::<_, String>(3)?),
                    evidence_json: r.get(4)?,
                    resolution: r.get(5)?,
                    resolved_by: r.get(6)?,
                    resolved_at_ms: r.get(7)?,
                    created_at_ms: r.get(8)?,
                })
            },
        );
        match result {
            Ok(dispute) => Ok(Some(dispute)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(KernelError::Driver(format!("get dispute: {}", e))),
        }
    }

    fn get_disputes_for_bounty(&self, bounty_id: &str) -> Result<Vec<DisputeRecord>, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT dispute_id, bounty_id, opened_by, status, evidence_json, resolution, resolved_by, resolved_at_ms, created_at_ms
             FROM runtime_disputes WHERE bounty_id = ?1 ORDER BY created_at_ms DESC"
        ).map_err(|e| KernelError::Driver(format!("prepare disputes: {}", e)))?;
        let disputes = stmt
            .query_map(params![bounty_id], |r| {
                Ok(DisputeRecord {
                    dispute_id: r.get(0)?,
                    bounty_id: r.get(1)?,
                    opened_by: r.get(2)?,
                    status: DisputeStatus::from_str(&r.get::<_, String>(3)?),
                    evidence_json: r.get(4)?,
                    resolution: r.get(5)?,
                    resolved_by: r.get(6)?,
                    resolved_at_ms: r.get(7)?,
                    created_at_ms: r.get(8)?,
                })
            })
            .map_err(|e| KernelError::Driver(format!("query disputes: {}", e)))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(disputes)
    }

    fn resolve_dispute(
        &self,
        dispute_id: &str,
        resolution: &str,
        resolved_by: &str,
    ) -> Result<(), KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let now = Utc::now().timestamp_millis();
        let updated = conn
            .execute(
                "UPDATE runtime_disputes SET status = 'resolved', resolution = ?2, resolved_by = ?3, resolved_at_ms = ?4 WHERE dispute_id = ?1 AND status = 'open'",
                params![dispute_id, resolution, resolved_by, now],
            )
            .map_err(|e| KernelError::Driver(format!("resolve dispute: {}", e)))?;
        if updated == 0 {
            return Err(KernelError::Driver(format!(
                "dispute not found or already resolved: {}",
                dispute_id
            )));
        }
        Ok(())
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

fn attempt_status_to_str(status: &AttemptExecutionStatus) -> &'static str {
    match status {
        AttemptExecutionStatus::Queued => "queued",
        AttemptExecutionStatus::Leased => "leased",
        AttemptExecutionStatus::Running => "running",
        AttemptExecutionStatus::RetryBackoff => "retry_backoff",
        AttemptExecutionStatus::Completed => "completed",
        AttemptExecutionStatus::Failed => "failed",
        AttemptExecutionStatus::Cancelled => "cancelled",
    }
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

fn parse_retry_policy_record(
    strategy: Option<String>,
    backoff_ms: Option<i64>,
    max_backoff_ms: Option<i64>,
    multiplier: Option<f64>,
    max_retries: Option<i64>,
) -> Option<RetryPolicyConfig> {
    Some(RetryPolicyConfig {
        strategy: RetryStrategy::from_str(strategy?.as_str())?,
        backoff_ms: backoff_ms?,
        max_backoff_ms,
        multiplier,
        max_retries: max_retries?.max(0) as u32,
    })
}

fn map_rusqlite_err(err: rusqlite::Error) -> KernelError {
    KernelError::Driver(format!("sqlite runtime repo: {}", err))
}

fn ensure_sqlite_migration_table(conn: &Connection) -> Result<(), KernelError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_schema_migrations (
          version INTEGER PRIMARY KEY,
          name TEXT NOT NULL,
          applied_at_ms INTEGER NOT NULL
        );
        "#,
    )
    .map_err(|e| KernelError::Driver(format!("init sqlite runtime migration table: {}", e)))?;
    Ok(())
}

fn sqlite_current_schema_version(conn: &Connection) -> Result<i64, KernelError> {
    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM runtime_schema_migrations",
        [],
        |r| r.get(0),
    )
    .map_err(|e| KernelError::Driver(format!("read sqlite runtime schema version: {}", e)))
}

fn record_sqlite_migration(conn: &Connection, version: i64, name: &str) -> Result<(), KernelError> {
    let now = dt_to_ms(Utc::now());
    conn.execute(
        "INSERT OR IGNORE INTO runtime_schema_migrations(version, name, applied_at_ms)
         VALUES (?1, ?2, ?3)",
        params![version, name, now],
    )
    .map_err(|e| KernelError::Driver(format!("record sqlite runtime migration: {}", e)))?;
    Ok(())
}

fn apply_sqlite_runtime_migration_v1(conn: &Connection) -> Result<(), KernelError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_attempts (
          attempt_id TEXT PRIMARY KEY,
          run_id TEXT NOT NULL,
          attempt_no INTEGER NOT NULL,
          status TEXT NOT NULL,
          retry_at_ms INTEGER NULL
        );
        CREATE TABLE IF NOT EXISTS runtime_leases (
          lease_id TEXT PRIMARY KEY,
          attempt_id TEXT NOT NULL UNIQUE,
          worker_id TEXT NOT NULL,
          lease_expires_at_ms INTEGER NOT NULL,
          heartbeat_at_ms INTEGER NOT NULL,
          version INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS runtime_jobs (
          thread_id TEXT PRIMARY KEY,
          status TEXT NOT NULL,
          created_at_ms INTEGER NOT NULL,
          updated_at_ms INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS runtime_interrupts (
          interrupt_id TEXT PRIMARY KEY,
          thread_id TEXT NOT NULL,
          run_id TEXT NOT NULL,
          attempt_id TEXT NOT NULL,
          value_json TEXT NOT NULL,
          status TEXT NOT NULL,
          created_at_ms INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS runtime_step_reports (
          report_id INTEGER PRIMARY KEY AUTOINCREMENT,
          worker_id TEXT NOT NULL,
          attempt_id TEXT NOT NULL,
          action_id TEXT NOT NULL,
          status TEXT NOT NULL,
          dedupe_token TEXT NOT NULL,
          created_at_ms INTEGER NOT NULL,
          UNIQUE(attempt_id, dedupe_token)
        );
        CREATE TABLE IF NOT EXISTS runtime_audit_logs (
          audit_id INTEGER PRIMARY KEY AUTOINCREMENT,
          actor_type TEXT NOT NULL,
          actor_id TEXT NULL,
          actor_role TEXT NULL,
          action TEXT NOT NULL,
          resource_type TEXT NOT NULL,
          resource_id TEXT NULL,
          result TEXT NOT NULL,
          request_id TEXT NOT NULL,
          details_json TEXT NULL,
          created_at_ms INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS runtime_api_keys (
          key_id TEXT PRIMARY KEY,
          secret_hash TEXT NOT NULL,
          status TEXT NOT NULL,
          created_at_ms INTEGER NOT NULL,
          updated_at_ms INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_runtime_attempts_status_retry ON runtime_attempts(status, retry_at_ms);
        CREATE INDEX IF NOT EXISTS idx_runtime_leases_expiry ON runtime_leases(lease_expires_at_ms);
        CREATE INDEX IF NOT EXISTS idx_runtime_interrupts_status ON runtime_interrupts(status);
        CREATE INDEX IF NOT EXISTS idx_runtime_interrupts_thread ON runtime_interrupts(thread_id);
        CREATE INDEX IF NOT EXISTS idx_runtime_jobs_status ON runtime_jobs(status);
        CREATE INDEX IF NOT EXISTS idx_runtime_step_reports_attempt ON runtime_step_reports(attempt_id, created_at_ms DESC);
        CREATE INDEX IF NOT EXISTS idx_runtime_audit_logs_created ON runtime_audit_logs(created_at_ms DESC);
        CREATE INDEX IF NOT EXISTS idx_runtime_audit_logs_request ON runtime_audit_logs(request_id);
        CREATE INDEX IF NOT EXISTS idx_runtime_audit_logs_action ON runtime_audit_logs(action, created_at_ms DESC);
        CREATE INDEX IF NOT EXISTS idx_runtime_api_keys_status ON runtime_api_keys(status);
        "#,
    )
    .map_err(|e| KernelError::Driver(format!("apply sqlite runtime migration v1: {}", e)))?;
    Ok(())
}

fn apply_sqlite_runtime_migration_v2(conn: &Connection) -> Result<(), KernelError> {
    add_column_if_missing(
        conn,
        "runtime_interrupts",
        "resume_payload_hash",
        "TEXT NULL",
    )?;
    add_column_if_missing(
        conn,
        "runtime_interrupts",
        "resume_response_json",
        "TEXT NULL",
    )?;
    add_column_if_missing(conn, "runtime_interrupts", "resumed_at_ms", "INTEGER NULL")?;
    add_column_if_missing(
        conn,
        "runtime_api_keys",
        "role",
        "TEXT NOT NULL DEFAULT 'operator'",
    )?;
    Ok(())
}

fn apply_sqlite_runtime_migration_v3(conn: &Connection) -> Result<(), KernelError> {
    add_column_if_missing(conn, "runtime_attempts", "retry_strategy", "TEXT NULL")?;
    add_column_if_missing(conn, "runtime_attempts", "retry_backoff_ms", "INTEGER NULL")?;
    add_column_if_missing(
        conn,
        "runtime_attempts",
        "retry_max_backoff_ms",
        "INTEGER NULL",
    )?;
    add_column_if_missing(conn, "runtime_attempts", "retry_multiplier", "REAL NULL")?;
    add_column_if_missing(
        conn,
        "runtime_attempts",
        "retry_max_retries",
        "INTEGER NULL",
    )?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_attempt_retry_history (
          retry_id INTEGER PRIMARY KEY AUTOINCREMENT,
          attempt_id TEXT NOT NULL,
          attempt_no INTEGER NOT NULL,
          strategy TEXT NOT NULL,
          backoff_ms INTEGER NOT NULL,
          max_retries INTEGER NOT NULL,
          scheduled_at_ms INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_runtime_attempt_retry_history_attempt
          ON runtime_attempt_retry_history(attempt_id, retry_id ASC);
        "#,
    )
    .map_err(|e| KernelError::Driver(format!("apply sqlite runtime migration v3: {}", e)))?;
    Ok(())
}

fn apply_sqlite_runtime_migration_v4(conn: &Connection) -> Result<(), KernelError> {
    add_column_if_missing(
        conn,
        "runtime_attempts",
        "execution_timeout_ms",
        "INTEGER NULL",
    )?;
    add_column_if_missing(
        conn,
        "runtime_attempts",
        "timeout_terminal_status",
        "TEXT NULL",
    )?;
    add_column_if_missing(conn, "runtime_attempts", "started_at_ms", "INTEGER NULL")?;
    Ok(())
}

fn apply_sqlite_runtime_migration_v5(conn: &Connection) -> Result<(), KernelError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_dead_letters (
          attempt_id TEXT PRIMARY KEY,
          run_id TEXT NOT NULL,
          attempt_no INTEGER NOT NULL,
          terminal_status TEXT NOT NULL,
          reason TEXT NULL,
          dead_at_ms INTEGER NOT NULL,
          replay_status TEXT NOT NULL,
          replay_count INTEGER NOT NULL,
          last_replayed_at_ms INTEGER NULL
        );
        CREATE INDEX IF NOT EXISTS idx_runtime_dead_letters_status_dead_at
          ON runtime_dead_letters(replay_status, dead_at_ms DESC);
        "#,
    )
    .map_err(|e| KernelError::Driver(format!("apply sqlite runtime migration v5: {}", e)))?;
    Ok(())
}

fn apply_sqlite_runtime_migration_v6(conn: &Connection) -> Result<(), KernelError> {
    add_column_if_missing(
        conn,
        "runtime_attempts",
        "priority",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_runtime_attempts_status_priority_retry
         ON runtime_attempts(status, priority DESC, retry_at_ms)",
        [],
    )
    .map_err(|e| KernelError::Driver(format!("apply sqlite runtime migration v6: {}", e)))?;
    Ok(())
}

fn apply_sqlite_runtime_migration_v7(conn: &Connection) -> Result<(), KernelError> {
    add_column_if_missing(conn, "runtime_attempts", "tenant_id", "TEXT NULL")?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_runtime_attempts_tenant_status
         ON runtime_attempts(tenant_id, status, priority DESC)",
        [],
    )
    .map_err(|e| KernelError::Driver(format!("apply sqlite runtime migration v7: {}", e)))?;
    Ok(())
}

fn apply_sqlite_runtime_migration_v8(conn: &Connection) -> Result<(), KernelError> {
    add_column_if_missing(conn, "runtime_attempts", "trace_id", "TEXT NULL")?;
    add_column_if_missing(
        conn,
        "runtime_attempts",
        "trace_parent_span_id",
        "TEXT NULL",
    )?;
    add_column_if_missing(conn, "runtime_attempts", "trace_span_id", "TEXT NULL")?;
    add_column_if_missing(
        conn,
        "runtime_attempts",
        "trace_flags",
        "TEXT NOT NULL DEFAULT '01'",
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_runtime_attempts_trace_id
         ON runtime_attempts(trace_id, attempt_id)",
        [],
    )
    .map_err(|e| KernelError::Driver(format!("apply sqlite runtime migration v8: {}", e)))?;
    Ok(())
}

fn apply_sqlite_runtime_migration_v9(conn: &Connection) -> Result<(), KernelError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_replay_effects (
          fingerprint TEXT PRIMARY KEY,
          thread_id TEXT NOT NULL,
          replay_target TEXT NOT NULL,
          effect_type TEXT NOT NULL,
          status TEXT NOT NULL,
          execution_count INTEGER NOT NULL,
          created_at_ms INTEGER NOT NULL,
          completed_at_ms INTEGER NULL,
          response_json TEXT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_runtime_replay_effects_thread_created
          ON runtime_replay_effects(thread_id, created_at_ms);
        "#,
    )
    .map_err(|e| KernelError::Driver(format!("apply sqlite runtime migration v9: {}", e)))?;
    Ok(())
}

fn apply_sqlite_runtime_migration_v10(conn: &Connection) -> Result<(), KernelError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_a2a_sessions (
          sender_id TEXT PRIMARY KEY,
          protocol TEXT NOT NULL,
          protocol_version TEXT NOT NULL,
          enabled_capabilities_json TEXT NOT NULL,
          actor_type TEXT NULL,
          actor_id TEXT NULL,
          actor_role TEXT NULL,
          negotiated_at_ms INTEGER NOT NULL,
          expires_at_ms INTEGER NOT NULL,
          updated_at_ms INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_runtime_a2a_sessions_expires
          ON runtime_a2a_sessions(expires_at_ms);
        "#,
    )
    .map_err(|e| KernelError::Driver(format!("apply sqlite runtime migration v10: {}", e)))?;
    Ok(())
}

fn apply_sqlite_runtime_migration_v11(conn: &Connection) -> Result<(), KernelError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_a2a_compat_tasks (
          session_id TEXT PRIMARY KEY,
          sender_id TEXT NOT NULL,
          protocol_version TEXT NOT NULL,
          task_id TEXT NOT NULL,
          task_summary TEXT NOT NULL,
          dispatch_id TEXT NOT NULL,
          claimed_by_sender_id TEXT NULL,
          lease_expires_at_ms INTEGER NULL,
          enqueued_at_ms INTEGER NOT NULL,
          updated_at_ms INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_runtime_a2a_compat_tasks_sender_protocol
          ON runtime_a2a_compat_tasks(sender_id, protocol_version, enqueued_at_ms);
        CREATE INDEX IF NOT EXISTS idx_runtime_a2a_compat_tasks_lease
          ON runtime_a2a_compat_tasks(lease_expires_at_ms);
        "#,
    )
    .map_err(|e| KernelError::Driver(format!("apply sqlite runtime migration v11: {}", e)))?;
    Ok(())
}

/// Migration v12: EvoMap Bounty, Swarm, and Worker registry tables
fn apply_sqlite_runtime_migration_v12(conn: &Connection) -> Result<(), KernelError> {
    conn.execute_batch(
        r#"
        -- Bounty table for task rewards
        CREATE TABLE IF NOT EXISTS runtime_bounties (
          bounty_id TEXT PRIMARY KEY,
          title TEXT NOT NULL,
          description TEXT,
          reward INTEGER NOT NULL,
          status TEXT NOT NULL DEFAULT 'open',
          created_by TEXT NOT NULL,
          created_at_ms INTEGER NOT NULL,
          closed_at_ms INTEGER NULL,
          accepted_by TEXT NULL,
          accepted_at_ms INTEGER NULL
        );
        CREATE INDEX IF NOT EXISTS idx_runtime_bounties_status ON runtime_bounties(status);
        CREATE INDEX IF NOT EXISTS idx_runtime_bounties_created_by ON runtime_bounties(created_by);

        -- Swarm task decomposition table
        CREATE TABLE IF NOT EXISTS runtime_swarm_tasks (
          parent_task_id TEXT PRIMARY KEY,
          decomposition_json TEXT NOT NULL,
          proposer_id TEXT NOT NULL,
          proposer_reward_pct INTEGER NOT NULL DEFAULT 5,
          solver_reward_pct INTEGER NOT NULL DEFAULT 85,
          aggregator_reward_pct INTEGER NOT NULL DEFAULT 10,
          status TEXT NOT NULL DEFAULT 'pending',
          created_at_ms INTEGER NOT NULL,
          completed_at_ms INTEGER NULL
        );
        CREATE INDEX IF NOT EXISTS idx_runtime_swarm_tasks_proposer ON runtime_swarm_tasks(proposer_id);
        CREATE INDEX IF NOT EXISTS idx_runtime_swarm_tasks_status ON runtime_swarm_tasks(status);

        -- Worker registry table
        CREATE TABLE IF NOT EXISTS runtime_workers_registry (
          worker_id TEXT PRIMARY KEY,
          domains TEXT NOT NULL,
          max_load INTEGER NOT NULL DEFAULT 1,
          metadata_json TEXT,
          registered_at_ms INTEGER NOT NULL,
          last_heartbeat_ms INTEGER NULL,
          status TEXT NOT NULL DEFAULT 'active'
        );
        CREATE INDEX IF NOT EXISTS idx_runtime_workers_domains ON runtime_workers_registry(domains);
        CREATE INDEX IF NOT EXISTS idx_runtime_workers_status ON runtime_workers_registry(status);
        "#,
    )
    .map_err(|e| KernelError::Driver(format!("apply sqlite runtime migration v12: {}", e)))?;
    Ok(())
}

/// Migration v13: EvoMap Recipe, Organism, Session, Dispute tables
fn apply_sqlite_runtime_migration_v13(conn: &Connection) -> Result<(), KernelError> {
    conn.execute_batch(
        r#"
        -- Recipe table for reusable gene sequences
        CREATE TABLE IF NOT EXISTS runtime_recipes (
          recipe_id TEXT PRIMARY KEY,
          name TEXT NOT NULL,
          description TEXT,
          gene_sequence_json TEXT NOT NULL,
          author_id TEXT NOT NULL,
          forked_from TEXT NULL,
          created_at_ms INTEGER NOT NULL,
          updated_at_ms INTEGER NOT NULL,
          is_public INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_runtime_recipes_author ON runtime_recipes(author_id);
        
        -- Organism table for running recipes
        CREATE TABLE IF NOT EXISTS runtime_organisms (
          organism_id TEXT PRIMARY KEY,
          recipe_id TEXT NOT NULL,
          status TEXT NOT NULL DEFAULT 'pending',
          current_step INTEGER NOT NULL DEFAULT 0,
          total_steps INTEGER NOT NULL,
          created_at_ms INTEGER NOT NULL,
          completed_at_ms INTEGER NULL
        );
        CREATE INDEX IF NOT EXISTS idx_runtime_organisms_recipe ON runtime_organisms(recipe_id);
        CREATE INDEX IF NOT EXISTS idx_runtime_organisms_status ON runtime_organisms(status);
        
        -- Collaborative session table
        CREATE TABLE IF NOT EXISTS runtime_collab_sessions (
          session_id TEXT PRIMARY KEY,
          session_type TEXT NOT NULL,
          creator_id TEXT NOT NULL,
          status TEXT NOT NULL DEFAULT 'active',
          created_at_ms INTEGER NOT NULL,
          ended_at_ms INTEGER NULL
        );
        CREATE INDEX IF NOT EXISTS idx_runtime_collab_sessions_creator ON runtime_collab_sessions(creator_id);
        
        -- Collaborative messages table
        CREATE TABLE IF NOT EXISTS runtime_collab_messages (
          message_id TEXT PRIMARY KEY,
          session_id TEXT NOT NULL,
          sender_id TEXT NOT NULL,
          content TEXT NOT NULL,
          message_type TEXT NOT NULL DEFAULT 'message',
          sent_at_ms INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_runtime_collab_messages_session ON runtime_collab_messages(session_id);
        
        -- Dispute table
        CREATE TABLE IF NOT EXISTS runtime_disputes (
          dispute_id TEXT PRIMARY KEY,
          bounty_id TEXT NOT NULL,
          opened_by TEXT NOT NULL,
          status TEXT NOT NULL DEFAULT 'open',
          evidence_json TEXT,
          resolution TEXT NULL,
          resolved_by TEXT NULL,
          resolved_at_ms INTEGER NULL,
          created_at_ms INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_runtime_disputes_bounty ON runtime_disputes(bounty_id);
        CREATE INDEX IF NOT EXISTS idx_runtime_disputes_status ON runtime_disputes(status);
        "#,
    )
    .map_err(|e| KernelError::Driver(format!("apply sqlite runtime migration v13: {}", e)))?;
    Ok(())
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    column_def: &str,
) -> Result<(), KernelError> {
    let pragma = format!("PRAGMA table_info({})", table);
    let mut stmt = conn
        .prepare(&pragma)
        .map_err(|e| KernelError::Driver(format!("prepare table_info {}: {}", table, e)))?;
    let cols = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| KernelError::Driver(format!("query table_info {}: {}", table, e)))?;
    for col in cols {
        let name = col.map_err(map_rusqlite_err)?;
        if name == column {
            return Ok(());
        }
    }
    let alter = format!("ALTER TABLE {} ADD COLUMN {} {}", table, column, column_def);
    conn.execute(&alter, [])
        .map_err(|e| KernelError::Driver(format!("alter table {} add {}: {}", table, column, e)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use chrono::{Duration, Utc};
    use rusqlite::{Connection, OptionalExtension};

    use super::{
        apply_sqlite_runtime_migration_v1, ensure_sqlite_migration_table, record_sqlite_migration,
        A2aCompatTaskRow, A2aSessionRow, OrganismRow, RecipeRow, ReplayEffectClaim,
        RetryPolicyConfig, RetryStrategy, SqliteRuntimeRepository, TimeoutPolicyConfig,
        SQLITE_RUNTIME_SCHEMA_VERSION,
    };
    use crate::models::AttemptExecutionStatus;
    use crate::repository::RuntimeRepository;

    fn temp_sqlite_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("oris-runtime-{}-{}.db", name, uuid::Uuid::new_v4()))
    }

    fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
        let pragma = format!("PRAGMA table_info({})", table);
        let mut stmt = conn.prepare(&pragma).expect("prepare pragma table_info");
        let mut rows = stmt.query([]).expect("query pragma table_info");
        while let Some(row) = rows.next().expect("scan pragma row") {
            let col_name: String = row.get(1).expect("column name");
            if col_name == column {
                return true;
            }
        }
        false
    }

    fn table_exists(conn: &Connection, table: &str) -> bool {
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |r| r.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .unwrap_or(false)
    }

    fn migration_version(conn: &Connection) -> i64 {
        conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM runtime_schema_migrations",
            [],
            |r| r.get(0),
        )
        .expect("read migration version")
    }

    #[test]
    fn schema_migration_clean_init_reaches_latest_version() {
        let path = temp_sqlite_path("schema-clean-init");
        let path_str = path.to_string_lossy().to_string();
        let _repo = SqliteRuntimeRepository::new(&path_str).expect("create sqlite runtime repo");

        let conn = Connection::open(&path).expect("open sqlite db");
        assert_eq!(migration_version(&conn), SQLITE_RUNTIME_SCHEMA_VERSION);
        assert!(column_exists(
            &conn,
            "runtime_interrupts",
            "resume_payload_hash"
        ));
        assert!(column_exists(
            &conn,
            "runtime_interrupts",
            "resume_response_json"
        ));
        assert!(column_exists(&conn, "runtime_interrupts", "resumed_at_ms"));
        assert!(column_exists(&conn, "runtime_api_keys", "role"));
        assert!(column_exists(&conn, "runtime_attempts", "retry_strategy"));
        assert!(column_exists(&conn, "runtime_attempts", "retry_backoff_ms"));
        assert!(column_exists(
            &conn,
            "runtime_attempts",
            "retry_max_backoff_ms"
        ));
        assert!(column_exists(&conn, "runtime_attempts", "retry_multiplier"));
        assert!(column_exists(
            &conn,
            "runtime_attempts",
            "retry_max_retries"
        ));
        assert!(column_exists(
            &conn,
            "runtime_attempts",
            "execution_timeout_ms"
        ));
        assert!(column_exists(
            &conn,
            "runtime_attempts",
            "timeout_terminal_status"
        ));
        assert!(column_exists(&conn, "runtime_attempts", "started_at_ms"));
        assert!(column_exists(&conn, "runtime_attempts", "priority"));
        assert!(column_exists(&conn, "runtime_attempts", "tenant_id"));
        assert!(column_exists(&conn, "runtime_attempts", "trace_id"));
        assert!(column_exists(
            &conn,
            "runtime_attempts",
            "trace_parent_span_id"
        ));
        assert!(column_exists(&conn, "runtime_attempts", "trace_span_id"));
        assert!(column_exists(&conn, "runtime_attempts", "trace_flags"));
        assert!(table_exists(&conn, "runtime_attempt_retry_history"));
        assert!(table_exists(&conn, "runtime_dead_letters"));
        assert!(table_exists(&conn, "runtime_replay_effects"));
        assert!(table_exists(&conn, "runtime_a2a_sessions"));
        assert!(table_exists(&conn, "runtime_a2a_compat_tasks"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn schema_migration_incremental_upgrade_from_v1_to_latest() {
        let path = temp_sqlite_path("schema-upgrade");
        {
            let conn = Connection::open(&path).expect("open sqlite db");
            ensure_sqlite_migration_table(&conn).expect("ensure migration table");
            apply_sqlite_runtime_migration_v1(&conn).expect("apply v1 migration");
            record_sqlite_migration(&conn, 1, "baseline_runtime_tables")
                .expect("record v1 migration");
            assert_eq!(migration_version(&conn), 1);
            assert!(!column_exists(
                &conn,
                "runtime_interrupts",
                "resume_payload_hash"
            ));
            assert!(!column_exists(&conn, "runtime_api_keys", "role"));
        }

        let path_str = path.to_string_lossy().to_string();
        let _repo = SqliteRuntimeRepository::new(&path_str).expect("reopen and migrate sqlite db");

        let conn = Connection::open(&path).expect("open upgraded sqlite db");
        assert_eq!(migration_version(&conn), SQLITE_RUNTIME_SCHEMA_VERSION);
        assert!(column_exists(
            &conn,
            "runtime_interrupts",
            "resume_payload_hash"
        ));
        assert!(column_exists(
            &conn,
            "runtime_interrupts",
            "resume_response_json"
        ));
        assert!(column_exists(&conn, "runtime_interrupts", "resumed_at_ms"));
        assert!(column_exists(&conn, "runtime_api_keys", "role"));
        assert!(column_exists(&conn, "runtime_attempts", "retry_strategy"));
        assert!(column_exists(&conn, "runtime_attempts", "retry_backoff_ms"));
        assert!(column_exists(
            &conn,
            "runtime_attempts",
            "retry_max_backoff_ms"
        ));
        assert!(column_exists(&conn, "runtime_attempts", "retry_multiplier"));
        assert!(column_exists(
            &conn,
            "runtime_attempts",
            "retry_max_retries"
        ));
        assert!(column_exists(
            &conn,
            "runtime_attempts",
            "execution_timeout_ms"
        ));
        assert!(column_exists(
            &conn,
            "runtime_attempts",
            "timeout_terminal_status"
        ));
        assert!(column_exists(&conn, "runtime_attempts", "started_at_ms"));
        assert!(column_exists(&conn, "runtime_attempts", "priority"));
        assert!(column_exists(&conn, "runtime_attempts", "tenant_id"));
        assert!(column_exists(&conn, "runtime_attempts", "trace_id"));
        assert!(column_exists(
            &conn,
            "runtime_attempts",
            "trace_parent_span_id"
        ));
        assert!(column_exists(&conn, "runtime_attempts", "trace_span_id"));
        assert!(column_exists(&conn, "runtime_attempts", "trace_flags"));
        assert!(table_exists(&conn, "runtime_attempt_retry_history"));
        assert!(table_exists(&conn, "runtime_dead_letters"));
        assert!(table_exists(&conn, "runtime_replay_effects"));
        assert!(table_exists(&conn, "runtime_a2a_sessions"));
        assert!(table_exists(&conn, "runtime_a2a_compat_tasks"));

        let migration_v2: Option<String> = conn
            .query_row(
                "SELECT name FROM runtime_schema_migrations WHERE version = 2",
                [],
                |r| r.get(0),
            )
            .optional()
            .expect("query migration v2");
        assert_eq!(
            migration_v2.as_deref(),
            Some("interrupt_resume_and_api_key_role")
        );
        let migration_v3: Option<String> = conn
            .query_row(
                "SELECT name FROM runtime_schema_migrations WHERE version = 3",
                [],
                |r| r.get(0),
            )
            .optional()
            .expect("query migration v3");
        assert_eq!(
            migration_v3.as_deref(),
            Some("attempt_retry_policy_and_history")
        );
        let migration_v4: Option<String> = conn
            .query_row(
                "SELECT name FROM runtime_schema_migrations WHERE version = 4",
                [],
                |r| r.get(0),
            )
            .optional()
            .expect("query migration v4");
        assert_eq!(
            migration_v4.as_deref(),
            Some("attempt_execution_timeout_policy")
        );
        let migration_v5: Option<String> = conn
            .query_row(
                "SELECT name FROM runtime_schema_migrations WHERE version = 5",
                [],
                |r| r.get(0),
            )
            .optional()
            .expect("query migration v5");
        assert_eq!(migration_v5.as_deref(), Some("runtime_dead_letter_queue"));
        let migration_v6: Option<String> = conn
            .query_row(
                "SELECT name FROM runtime_schema_migrations WHERE version = 6",
                [],
                |r| r.get(0),
            )
            .optional()
            .expect("query migration v6");
        assert_eq!(
            migration_v6.as_deref(),
            Some("attempt_priority_dispatch_order")
        );
        let migration_v7: Option<String> = conn
            .query_row(
                "SELECT name FROM runtime_schema_migrations WHERE version = 7",
                [],
                |r| r.get(0),
            )
            .optional()
            .expect("query migration v7");
        assert_eq!(migration_v7.as_deref(), Some("attempt_tenant_rate_limits"));
        let migration_v8: Option<String> = conn
            .query_row(
                "SELECT name FROM runtime_schema_migrations WHERE version = 8",
                [],
                |r| r.get(0),
            )
            .optional()
            .expect("query migration v8");
        assert_eq!(migration_v8.as_deref(), Some("attempt_trace_context"));
        let migration_v9: Option<String> = conn
            .query_row(
                "SELECT name FROM runtime_schema_migrations WHERE version = 9",
                [],
                |r| r.get(0),
            )
            .optional()
            .expect("query migration v9");
        assert_eq!(migration_v9.as_deref(), Some("replay_effect_guard"));
        let migration_v10: Option<String> = conn
            .query_row(
                "SELECT name FROM runtime_schema_migrations WHERE version = 10",
                [],
                |r| r.get(0),
            )
            .optional()
            .expect("query migration v10");
        assert_eq!(migration_v10.as_deref(), Some("runtime_a2a_sessions"));
        let migration_v11: Option<String> = conn
            .query_row(
                "SELECT name FROM runtime_schema_migrations WHERE version = 11",
                [],
                |r| r.get(0),
            )
            .optional()
            .expect("query migration v11");
        assert_eq!(migration_v11.as_deref(), Some("runtime_a2a_compat_tasks"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn a2a_session_upsert_roundtrip_and_expiry_filtering() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite runtime repo");
        let now = Utc::now();

        repo.upsert_a2a_session(&A2aSessionRow {
            sender_id: "node-a".to_string(),
            protocol: "oris.a2a".to_string(),
            protocol_version: "0.1.0-experimental".to_string(),
            enabled_capabilities_json: "[\"EvolutionPublish\"]".to_string(),
            actor_type: Some("api_key".to_string()),
            actor_id: Some("key-a".to_string()),
            actor_role: Some("admin".to_string()),
            negotiated_at: now,
            expires_at: now + Duration::hours(1),
            updated_at: now,
        })
        .expect("upsert active a2a session");

        let active = repo
            .get_active_a2a_session("node-a", now)
            .expect("read active a2a session")
            .expect("active session exists");
        assert_eq!(active.sender_id, "node-a");
        assert_eq!(active.protocol, "oris.a2a");
        assert_eq!(active.protocol_version, "0.1.0-experimental");
        assert_eq!(active.enabled_capabilities_json, "[\"EvolutionPublish\"]");
        assert_eq!(active.actor_type.as_deref(), Some("api_key"));
        assert_eq!(active.actor_id.as_deref(), Some("key-a"));
        assert_eq!(active.actor_role.as_deref(), Some("admin"));

        repo.upsert_a2a_session(&A2aSessionRow {
            sender_id: "node-expired".to_string(),
            protocol: "oris.a2a".to_string(),
            protocol_version: "0.1.0-experimental".to_string(),
            enabled_capabilities_json: "[\"EvolutionFetch\"]".to_string(),
            actor_type: None,
            actor_id: None,
            actor_role: None,
            negotiated_at: now - Duration::hours(2),
            expires_at: now - Duration::minutes(1),
            updated_at: now - Duration::hours(2),
        })
        .expect("upsert expired a2a session");

        assert!(repo
            .get_active_a2a_session("node-expired", now)
            .expect("read expired a2a session")
            .is_none());
        let purged = repo
            .purge_expired_a2a_sessions(now)
            .expect("purge expired sessions");
        assert_eq!(purged, 1);
    }

    #[test]
    fn a2a_compat_task_claim_roundtrip_and_lease_reclaim() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite runtime repo");
        let now = Utc::now();

        assert_eq!(
            repo.a2a_compat_queue_depth()
                .expect("initial a2a compat queue depth"),
            0
        );

        repo.upsert_a2a_compat_task(&A2aCompatTaskRow {
            session_id: "session-1".to_string(),
            sender_id: "sender-a".to_string(),
            protocol_version: "1.0.0".to_string(),
            task_id: "task-1".to_string(),
            task_summary: "compat task".to_string(),
            dispatch_id: "dispatch-1".to_string(),
            claimed_by_sender_id: None,
            lease_expires_at: None,
            enqueued_at: now,
            updated_at: now,
        })
        .expect("upsert compat task");
        assert_eq!(
            repo.a2a_compat_queue_depth()
                .expect("a2a compat queue depth after enqueue"),
            1
        );

        let first = repo
            .claim_a2a_compat_task("sender-a", "1.0.0", now, 1_000, None)
            .expect("first claim");
        assert!(first.task.is_some());
        assert_eq!(
            first.task.as_ref().map(|task| task.task_id.as_str()),
            Some("task-1")
        );
        assert_eq!(first.retry_after_ms, None);

        let second = repo
            .claim_a2a_compat_task(
                "sender-a",
                "1.0.0",
                now + Duration::milliseconds(100),
                1_000,
                None,
            )
            .expect("second claim");
        assert!(second.task.is_none());
        assert!(second.retry_after_ms.unwrap_or(0) > 0);

        let reclaimed = repo
            .claim_a2a_compat_task(
                "sender-a",
                "1.0.0",
                now + Duration::milliseconds(1_200),
                1_000,
                None,
            )
            .expect("reclaimed claim");
        assert!(reclaimed.task.is_some());
        assert_eq!(
            reclaimed.task.as_ref().map(|task| task.session_id.as_str()),
            Some("session-1")
        );

        let touched = repo
            .touch_a2a_compat_task_lease(
                "session-1",
                "sender-a",
                now + Duration::milliseconds(1_250),
                2_000,
            )
            .expect("touch claim lease");
        assert!(touched);

        let removed = repo
            .remove_a2a_compat_task("session-1")
            .expect("remove compat task");
        assert_eq!(removed, 1);
        assert_eq!(
            repo.a2a_compat_queue_depth()
                .expect("a2a compat queue depth after remove"),
            0
        );

        let after_remove = repo
            .claim_a2a_compat_task(
                "sender-a",
                "1.0.0",
                now + Duration::milliseconds(1_300),
                1_000,
                None,
            )
            .expect("claim after remove");
        assert!(after_remove.task.is_none());
    }

    #[test]
    fn list_a2a_compat_tasks_returns_ordered_sender_protocol_slice() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite runtime repo");
        let now = Utc::now();

        repo.upsert_a2a_compat_task(&A2aCompatTaskRow {
            session_id: "session-a1".to_string(),
            sender_id: "sender-a".to_string(),
            protocol_version: "1.0.0".to_string(),
            task_id: "task-a1".to_string(),
            task_summary: "first".to_string(),
            dispatch_id: "dispatch-a1".to_string(),
            claimed_by_sender_id: None,
            lease_expires_at: None,
            enqueued_at: now,
            updated_at: now,
        })
        .expect("upsert sender-a first task");
        repo.upsert_a2a_compat_task(&A2aCompatTaskRow {
            session_id: "session-b1".to_string(),
            sender_id: "sender-b".to_string(),
            protocol_version: "1.0.0".to_string(),
            task_id: "task-b1".to_string(),
            task_summary: "other sender".to_string(),
            dispatch_id: "dispatch-b1".to_string(),
            claimed_by_sender_id: None,
            lease_expires_at: None,
            enqueued_at: now + Duration::milliseconds(1),
            updated_at: now + Duration::milliseconds(1),
        })
        .expect("upsert sender-b task");
        repo.upsert_a2a_compat_task(&A2aCompatTaskRow {
            session_id: "session-a2".to_string(),
            sender_id: "sender-a".to_string(),
            protocol_version: "1.0.0".to_string(),
            task_id: "task-a2".to_string(),
            task_summary: "second".to_string(),
            dispatch_id: "dispatch-a2".to_string(),
            claimed_by_sender_id: Some("sender-a".to_string()),
            lease_expires_at: Some(now + Duration::seconds(10)),
            enqueued_at: now + Duration::milliseconds(2),
            updated_at: now + Duration::milliseconds(2),
        })
        .expect("upsert sender-a second task");

        let sender_a_tasks = repo
            .list_a2a_compat_tasks("sender-a", "1.0.0")
            .expect("list sender-a tasks");
        assert_eq!(sender_a_tasks.len(), 2);
        assert_eq!(sender_a_tasks[0].task_id, "task-a1");
        assert_eq!(sender_a_tasks[1].task_id, "task-a2");
        assert_eq!(
            sender_a_tasks[1].claimed_by_sender_id.as_deref(),
            Some("sender-a")
        );

        let sender_b_tasks = repo
            .list_a2a_compat_tasks("sender-b", "1.0.0")
            .expect("list sender-b tasks");
        assert_eq!(sender_b_tasks.len(), 1);
        assert_eq!(sender_b_tasks[0].task_id, "task-b1");

        let mismatched_protocol = repo
            .list_a2a_compat_tasks("sender-a", "0.1.0-experimental")
            .expect("list sender-a mismatched protocol");
        assert!(mismatched_protocol.is_empty());
    }

    #[test]
    fn claim_a2a_compat_task_uses_session_id_tiebreaker_for_equal_enqueue_time() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite runtime repo");
        let now = Utc::now();

        for (session_id, task_id) in [("session-b", "task-b"), ("session-a", "task-a")] {
            repo.upsert_a2a_compat_task(&A2aCompatTaskRow {
                session_id: session_id.to_string(),
                sender_id: "sender-a".to_string(),
                protocol_version: "1.0.0".to_string(),
                task_id: task_id.to_string(),
                task_summary: format!("summary-{task_id}"),
                dispatch_id: format!("dispatch-{task_id}"),
                claimed_by_sender_id: None,
                lease_expires_at: None,
                enqueued_at: now,
                updated_at: now,
            })
            .expect("upsert a2a compat task");
        }

        let claim = repo
            .claim_a2a_compat_task("sender-a", "1.0.0", now, 1_000, None)
            .expect("claim with deterministic order");
        assert_eq!(
            claim.task.as_ref().map(|task| task.session_id.as_str()),
            Some("session-a")
        );
    }

    #[test]
    fn claim_a2a_compat_task_honors_requested_task_id_filter() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite runtime repo");
        let now = Utc::now();

        for (offset, task_id) in [(0_i64, "task-a"), (1_i64, "task-b")] {
            repo.upsert_a2a_compat_task(&A2aCompatTaskRow {
                session_id: format!("session-{task_id}"),
                sender_id: "sender-a".to_string(),
                protocol_version: "1.0.0".to_string(),
                task_id: task_id.to_string(),
                task_summary: format!("summary-{task_id}"),
                dispatch_id: format!("dispatch-{task_id}"),
                claimed_by_sender_id: None,
                lease_expires_at: None,
                enqueued_at: now + Duration::milliseconds(offset),
                updated_at: now + Duration::milliseconds(offset),
            })
            .expect("upsert a2a compat task");
        }

        let claim_specific = repo
            .claim_a2a_compat_task("sender-a", "1.0.0", now, 2_000, Some("task-b"))
            .expect("claim specific task");
        assert_eq!(
            claim_specific
                .task
                .as_ref()
                .map(|task| task.task_id.as_str()),
            Some("task-b")
        );
        assert!(claim_specific.retry_after_ms.is_none());

        let claim_specific_again = repo
            .claim_a2a_compat_task(
                "sender-a",
                "1.0.0",
                now + Duration::milliseconds(100),
                2_000,
                Some("task-b"),
            )
            .expect("claim specific task again");
        assert!(claim_specific_again.task.is_none());
        assert!(claim_specific_again.retry_after_ms.unwrap_or(0) > 0);

        let claim_missing_specific = repo
            .claim_a2a_compat_task(
                "sender-a",
                "1.0.0",
                now + Duration::milliseconds(200),
                2_000,
                Some("task-missing"),
            )
            .expect("claim missing specific task");
        assert!(claim_missing_specific.task.is_none());
        assert!(claim_missing_specific.retry_after_ms.is_none());
    }

    #[test]
    fn touch_a2a_compat_task_lease_requires_active_owner() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite runtime repo");
        let now = Utc::now();

        repo.upsert_a2a_compat_task(&A2aCompatTaskRow {
            session_id: "session-lease".to_string(),
            sender_id: "sender-a".to_string(),
            protocol_version: "1.0.0".to_string(),
            task_id: "task-lease".to_string(),
            task_summary: "lease task".to_string(),
            dispatch_id: "dispatch-lease".to_string(),
            claimed_by_sender_id: None,
            lease_expires_at: None,
            enqueued_at: now,
            updated_at: now,
        })
        .expect("upsert lease task");

        let touched_unclaimed = repo
            .touch_a2a_compat_task_lease("session-lease", "sender-a", now, 1_000)
            .expect("touch unclaimed lease");
        assert!(!touched_unclaimed);

        let claim = repo
            .claim_a2a_compat_task("sender-a", "1.0.0", now, 1_000, None)
            .expect("claim lease task");
        assert!(claim.task.is_some());

        let touched_intruder = repo
            .touch_a2a_compat_task_lease(
                "session-lease",
                "sender-intruder",
                now + Duration::milliseconds(10),
                1_000,
            )
            .expect("touch intruder lease");
        assert!(!touched_intruder);

        let touched_owner = repo
            .touch_a2a_compat_task_lease(
                "session-lease",
                "sender-a",
                now + Duration::milliseconds(10),
                1_000,
            )
            .expect("touch owner lease");
        assert!(touched_owner);

        let touched_expired = repo
            .touch_a2a_compat_task_lease(
                "session-lease",
                "sender-a",
                now + Duration::milliseconds(2_000),
                1_000,
            )
            .expect("touch expired lease");
        assert!(!touched_expired);
    }

    #[test]
    fn replay_effect_guard_persists_completed_effects_and_dedupes() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite runtime repo");
        let claim = repo
            .claim_replay_effect(
                "thread-replay-guard",
                "latest_state:abc123",
                "fingerprint-1",
                Utc::now(),
            )
            .expect("claim replay effect");
        assert_eq!(claim, ReplayEffectClaim::Acquired);

        repo.complete_replay_effect(
            "fingerprint-1",
            r#"{"thread_id":"thread-replay-guard","status":"completed"}"#,
            Utc::now(),
        )
        .expect("complete replay effect");

        let second_claim = repo
            .claim_replay_effect(
                "thread-replay-guard",
                "latest_state:abc123",
                "fingerprint-1",
                Utc::now(),
            )
            .expect("claim completed replay effect");
        match second_claim {
            ReplayEffectClaim::Completed(response_json) => {
                assert!(response_json.contains("\"thread_id\":\"thread-replay-guard\""));
            }
            other => panic!("expected completed replay effect, got {:?}", other),
        }

        let rows = repo
            .list_replay_effects_for_thread("thread-replay-guard")
            .expect("list replay effects");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].fingerprint, "fingerprint-1");
        assert_eq!(rows[0].replay_target, "latest_state:abc123");
        assert_eq!(rows[0].effect_type, "job_replay");
        assert_eq!(rows[0].status, "completed");
        assert_eq!(rows[0].execution_count, 1);
        assert!(rows[0].completed_at.is_some());
    }

    #[test]
    fn attempt_trace_context_round_trip_and_advances() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite repo");
        repo.enqueue_attempt("attempt-trace-1", "run-trace-1")
            .expect("enqueue trace attempt");
        repo.set_attempt_trace_context(
            "attempt-trace-1",
            "0123456789abcdef0123456789abcdef",
            Some("1111111111111111"),
            "2222222222222222",
            "01",
        )
        .expect("set trace context");

        let current = repo
            .get_attempt_trace_context("attempt-trace-1")
            .expect("get trace context")
            .expect("trace context");
        assert_eq!(current.trace_id, "0123456789abcdef0123456789abcdef");
        assert_eq!(current.parent_span_id.as_deref(), Some("1111111111111111"));
        assert_eq!(current.span_id, "2222222222222222");
        assert_eq!(current.trace_flags, "01");

        let advanced = repo
            .advance_attempt_trace("attempt-trace-1", "3333333333333333")
            .expect("advance trace")
            .expect("advanced trace");
        assert_eq!(advanced.parent_span_id.as_deref(), Some("2222222222222222"));
        assert_eq!(advanced.span_id, "3333333333333333");

        let latest = repo
            .latest_attempt_trace_for_run("run-trace-1")
            .expect("latest trace for run")
            .expect("run trace");
        assert_eq!(latest.parent_span_id.as_deref(), Some("2222222222222222"));
        assert_eq!(latest.span_id, "3333333333333333");
        assert_eq!(
            repo.latest_attempt_id_for_run("run-trace-1")
                .expect("latest attempt id"),
            Some("attempt-trace-1".into())
        );
    }

    #[test]
    fn ack_attempt_exponential_backoff_respects_cap_and_max_retries() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite repo");
        repo.enqueue_attempt("attempt-retry-exp", "run-retry-exp")
            .expect("enqueue retry attempt");
        let policy = RetryPolicyConfig {
            strategy: RetryStrategy::Exponential,
            backoff_ms: 100,
            max_backoff_ms: Some(250),
            multiplier: Some(3.0),
            max_retries: 2,
        };

        let first = repo
            .ack_attempt(
                "attempt-retry-exp",
                AttemptExecutionStatus::Failed,
                Some(&policy),
                Utc::now(),
            )
            .expect("schedule first retry");
        assert_eq!(first.status, AttemptExecutionStatus::RetryBackoff);
        assert_eq!(first.next_attempt_no, 2);

        let second = repo
            .ack_attempt(
                "attempt-retry-exp",
                AttemptExecutionStatus::Failed,
                None,
                Utc::now(),
            )
            .expect("schedule second retry");
        assert_eq!(second.status, AttemptExecutionStatus::RetryBackoff);
        assert_eq!(second.next_attempt_no, 3);

        let third = repo
            .ack_attempt(
                "attempt-retry-exp",
                AttemptExecutionStatus::Failed,
                None,
                Utc::now(),
            )
            .expect("final failure after max retries");
        assert_eq!(third.status, AttemptExecutionStatus::Failed);
        assert_eq!(third.next_attempt_no, 3);

        let snapshot = repo
            .get_attempt_retry_history("attempt-retry-exp")
            .expect("read retry history")
            .expect("retry history exists");
        assert_eq!(snapshot.current_attempt_no, 3);
        assert_eq!(snapshot.current_status, AttemptExecutionStatus::Failed);
        assert_eq!(snapshot.history.len(), 2);
        assert_eq!(snapshot.history[0].attempt_no, 2);
        assert_eq!(snapshot.history[0].backoff_ms, 100);
        assert_eq!(snapshot.history[1].attempt_no, 3);
        assert_eq!(snapshot.history[1].backoff_ms, 250);
    }

    #[test]
    fn transition_timed_out_attempts_applies_configured_terminal_status() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite repo");
        repo.enqueue_attempt("attempt-timeout-1", "run-timeout-1")
            .expect("enqueue timeout attempt");
        repo.set_attempt_timeout_policy(
            "attempt-timeout-1",
            &TimeoutPolicyConfig {
                timeout_ms: 1_000,
                on_timeout_status: AttemptExecutionStatus::Cancelled,
            },
        )
        .expect("set timeout policy");
        let lease = repo
            .upsert_lease(
                "attempt-timeout-1",
                "worker-timeout-1",
                Utc::now() + Duration::seconds(30),
            )
            .expect("lease timeout attempt");
        assert!(!lease.lease_id.is_empty());
        repo.set_attempt_started_at_for_test(
            "attempt-timeout-1",
            Some(Utc::now() - Duration::seconds(5)),
        )
        .expect("backdate started_at");

        let transitioned = repo
            .transition_timed_out_attempts(Utc::now())
            .expect("transition timed out attempts");
        assert_eq!(transitioned, 1);
        assert!(repo
            .get_lease_for_attempt("attempt-timeout-1")
            .expect("read lease")
            .is_none());
        let (attempt_no, status) = repo
            .get_attempt_status("attempt-timeout-1")
            .expect("read attempt status")
            .expect("attempt exists");
        assert_eq!(attempt_no, 1);
        assert_eq!(status, AttemptExecutionStatus::Cancelled);
    }

    #[test]
    fn final_failed_attempts_are_persisted_to_dead_letter_queue_and_replayable() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite repo");
        repo.enqueue_attempt("attempt-dlq-1", "run-dlq-1")
            .expect("enqueue dlq attempt");

        let outcome = repo
            .ack_attempt(
                "attempt-dlq-1",
                AttemptExecutionStatus::Failed,
                None,
                Utc::now(),
            )
            .expect("mark final failed");
        assert_eq!(outcome.status, AttemptExecutionStatus::Failed);

        let row = repo
            .get_dead_letter("attempt-dlq-1")
            .expect("read dead letter")
            .expect("dead letter exists");
        assert_eq!(row.run_id, "run-dlq-1");
        assert_eq!(row.terminal_status, "failed");
        assert_eq!(row.replay_status, "pending");
        assert_eq!(row.replay_count, 0);

        let replayed = repo
            .replay_dead_letter("attempt-dlq-1", Utc::now())
            .expect("replay dead letter");
        assert_eq!(replayed.replay_status, "replayed");
        assert_eq!(replayed.replay_count, 1);

        let (_, status) = repo
            .get_attempt_status("attempt-dlq-1")
            .expect("read replayed attempt status")
            .expect("attempt exists");
        assert_eq!(status, AttemptExecutionStatus::Queued);
    }

    #[test]
    fn list_dispatchable_attempts_prefers_higher_priority_first() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite repo");
        repo.enqueue_attempt("attempt-priority-low", "run-priority")
            .expect("enqueue low priority");
        repo.enqueue_attempt("attempt-priority-high", "run-priority")
            .expect("enqueue high priority");
        repo.set_attempt_priority("attempt-priority-low", 10)
            .expect("set low priority");
        repo.set_attempt_priority("attempt-priority-high", 90)
            .expect("set high priority");

        let rows = repo
            .list_dispatchable_attempts(Utc::now(), 10)
            .expect("list dispatchable attempts");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].attempt_id, "attempt-priority-high");
        assert_eq!(rows[1].attempt_id, "attempt-priority-low");
    }

    #[test]
    fn heartbeat_lease_with_version_rejects_split_brain_owner_or_stale_version() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite repo");
        repo.enqueue_attempt("attempt-split-brain", "run-split-brain")
            .expect("enqueue split-brain attempt");
        let lease = repo
            .upsert_lease(
                "attempt-split-brain",
                "worker-a",
                Utc::now() + Duration::seconds(30),
            )
            .expect("create lease");

        let wrong_owner = repo.heartbeat_lease_with_version(
            &lease.lease_id,
            "worker-b",
            lease.version,
            Utc::now(),
            Utc::now() + Duration::seconds(30),
        );
        assert!(wrong_owner.is_err());

        repo.heartbeat_lease_with_version(
            &lease.lease_id,
            "worker-a",
            lease.version,
            Utc::now(),
            Utc::now() + Duration::seconds(30),
        )
        .expect("owner heartbeat succeeds");

        let stale_version = repo.heartbeat_lease_with_version(
            &lease.lease_id,
            "worker-a",
            lease.version,
            Utc::now(),
            Utc::now() + Duration::seconds(30),
        );
        assert!(stale_version.is_err());

        let persisted = repo
            .get_lease_by_id(&lease.lease_id)
            .expect("read persisted lease")
            .expect("lease still exists");
        assert_eq!(persisted.version, 2);
    }

    #[test]
    fn expire_leases_and_requeue_respects_stale_cutoff() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite repo");
        let now = Utc::now();
        repo.enqueue_attempt("attempt-grace", "run-grace")
            .expect("enqueue attempt");
        repo.upsert_lease("attempt-grace", "worker-grace", now - Duration::seconds(1))
            .expect("create nearly expired lease");

        let not_yet_stale = repo
            .expire_leases_and_requeue(now - Duration::seconds(5))
            .expect("grace-aware expire");
        assert_eq!(not_yet_stale, 0);
        assert!(repo
            .get_lease_for_attempt("attempt-grace")
            .expect("read lease")
            .is_some());

        let expired = repo
            .expire_leases_and_requeue(now)
            .expect("expire after grace");
        assert_eq!(expired, 1);
        assert!(repo
            .get_lease_for_attempt("attempt-grace")
            .expect("read lease after expire")
            .is_none());

        let (_, status) = repo
            .get_attempt_status("attempt-grace")
            .expect("read attempt status")
            .expect("attempt exists");
        assert_eq!(status, AttemptExecutionStatus::Queued);
    }

    #[test]
    fn bounty_lifecycle_create_accept_close_roundtrip() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite repo");
        let now = Utc::now();
        let created = repo
            .create_bounty(
                "bounty-1",
                "Implement feature X",
                Some("details"),
                100,
                "alice",
                now,
            )
            .expect("create bounty");
        assert_eq!(created.status, "open");
        assert_eq!(created.accepted_by, None);

        let accepted = repo
            .accept_bounty("bounty-1", "worker-1", now + Duration::seconds(1))
            .expect("accept bounty");
        assert!(accepted);

        let after_accept = repo
            .get_bounty("bounty-1")
            .expect("read bounty after accept")
            .expect("bounty exists");
        assert_eq!(after_accept.status, "accepted");
        assert_eq!(after_accept.accepted_by.as_deref(), Some("worker-1"));

        let closed = repo
            .close_bounty("bounty-1", now + Duration::seconds(2))
            .expect("close bounty");
        assert!(closed);

        let after_close = repo
            .get_bounty("bounty-1")
            .expect("read bounty after close")
            .expect("bounty exists");
        assert_eq!(after_close.status, "closed");
        assert!(after_close.closed_at.is_some());
    }

    #[test]
    fn bounty_lifecycle_invalid_transitions_are_rejected() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite repo");
        let now = Utc::now();
        repo.create_bounty("bounty-2", "Implement feature Y", None, 50, "alice", now)
            .expect("create bounty");

        let close_while_open = repo
            .close_bounty("bounty-2", now + Duration::seconds(1))
            .expect("close open bounty");
        assert!(!close_while_open);

        let first_accept = repo
            .accept_bounty("bounty-2", "worker-2", now + Duration::seconds(2))
            .expect("accept open bounty");
        assert!(first_accept);

        let second_accept = repo
            .accept_bounty("bounty-2", "worker-3", now + Duration::seconds(3))
            .expect("accept accepted bounty");
        assert!(!second_accept);

        let missing_close = repo
            .close_bounty("bounty-missing", now + Duration::seconds(4))
            .expect("close missing bounty");
        assert!(!missing_close);
    }

    #[test]
    fn swarm_task_create_and_get_roundtrip() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite repo");
        let now = Utc::now();
        let decomposition_json =
            r#"{"child_tasks":[{"task_id":"c1","description":"d1","role":"solver"}]}"#;

        let created = repo
            .create_swarm_task(
                "parent-1",
                decomposition_json,
                "alice",
                5,
                85,
                10,
                "pending",
                now,
            )
            .expect("create swarm task");
        assert_eq!(created.parent_task_id, "parent-1");
        assert_eq!(created.proposer_reward_pct, 5);
        assert_eq!(created.solver_reward_pct, 85);
        assert_eq!(created.aggregator_reward_pct, 10);
        assert_eq!(created.status, "pending");

        let fetched = repo
            .get_swarm_task("parent-1")
            .expect("get swarm task")
            .expect("swarm task exists");
        assert_eq!(fetched.decomposition_json, decomposition_json);
        assert_eq!(fetched.proposer_id, "alice");
    }

    #[test]
    fn worker_registration_upsert_and_read_roundtrip() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite repo");
        let now = Utc::now();
        let row = repo
            .upsert_worker_registration(
                "worker-1",
                r#"["docs","ci"]"#,
                2,
                Some(r#"{"region":"cn"}"#),
                "active",
                now,
            )
            .expect("upsert worker");
        assert_eq!(row.worker_id, "worker-1");
        assert_eq!(row.max_load, 2);
        assert_eq!(row.status, "active");

        let fetched = repo
            .get_worker_registration("worker-1")
            .expect("get worker")
            .expect("worker exists");
        assert_eq!(fetched.domains_json, r#"["docs","ci"]"#);
        assert_eq!(fetched.max_load, 2);
    }

    #[test]
    fn worker_active_claim_count_tracks_unexpired_leases() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite repo");
        let now = Utc::now();
        repo.upsert_a2a_compat_task(&A2aCompatTaskRow {
            session_id: "session-worker-1".to_string(),
            sender_id: "worker-1".to_string(),
            protocol_version: "1.0.0".to_string(),
            task_id: "task-worker-1".to_string(),
            task_summary: "task".to_string(),
            dispatch_id: "dispatch-worker-1".to_string(),
            claimed_by_sender_id: Some("worker-1".to_string()),
            lease_expires_at: Some(now + Duration::seconds(30)),
            enqueued_at: now,
            updated_at: now,
        })
        .expect("insert claimed task");

        let active = repo
            .count_active_claims_for_worker("worker-1", now)
            .expect("count active claims");
        assert_eq!(active, 1);

        let after_expiry = repo
            .count_active_claims_for_worker("worker-1", now + Duration::seconds(31))
            .expect("count claims after expiry");
        assert_eq!(after_expiry, 0);
    }

    #[test]
    fn dispute_lifecycle_persists_and_settles_bounty() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite repo");
        let now = Utc::now();
        repo.create_bounty("bounty-d1", "b", None, 100, "alice", now)
            .expect("create bounty");

        let dispute = repo
            .create_dispute("dispute-1", "bounty-d1", "alice", "bad output", now)
            .expect("create dispute");
        assert_eq!(dispute.status, "open");

        let evidence_ok = repo
            .append_dispute_evidence("dispute-1", "alice", r#"{"type":"log"}"#)
            .expect("append evidence");
        assert!(evidence_ok);

        let resolved = repo
            .resolve_dispute(
                "dispute-1",
                "arbiter-1",
                "claimant_win",
                now + Duration::seconds(1),
            )
            .expect("resolve dispute")
            .expect("resolved row");
        assert_eq!(resolved.status, "resolved");
        assert_eq!(resolved.resolution.as_deref(), Some("claimant_win"));

        let settled = repo
            .settle_bounty_via_dispute("bounty-d1", "settled_claimant_win", now)
            .expect("settle bounty");
        assert!(settled);

        let bounty = repo
            .get_bounty("bounty-d1")
            .expect("get bounty")
            .expect("bounty exists");
        assert_eq!(bounty.status, "settled_claimant_win");
    }

    #[test]
    fn dispute_invalid_transitions_are_rejected() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite repo");
        let now = Utc::now();
        repo.create_bounty("bounty-d2", "b", None, 100, "alice", now)
            .expect("create bounty");
        repo.create_dispute("dispute-2", "bounty-d2", "alice", "desc", now)
            .expect("create dispute");
        repo.resolve_dispute("dispute-2", "arbiter", "agent_win", now)
            .expect("resolve first");

        let second_resolve = repo
            .resolve_dispute("dispute-2", "arbiter", "split", now + Duration::seconds(1))
            .expect("resolve second");
        assert!(second_resolve.is_none());

        let evidence_after_resolve = repo
            .append_dispute_evidence("dispute-2", "alice", r#"{"k":"v"}"#)
            .expect("evidence after resolve");
        assert!(!evidence_after_resolve);
    }

    #[test]
    fn recipe_crud_lifecycle() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite repo");
        let now = Utc::now();

        // Create recipe
        let recipe = RecipeRow {
            recipe_id: "recipe-1".to_string(),
            name: "Test Recipe".to_string(),
            description: Some("A test recipe".to_string()),
            gene_sequence_json: r#"{"steps": ["step1", "step2"]}"#.to_string(),
            author_id: "author-1".to_string(),
            forked_from: None,
            created_at: now,
            updated_at: now,
            is_public: true,
        };
        repo.create_recipe(&recipe).expect("create recipe");

        // Get recipe
        let retrieved = repo.get_recipe("recipe-1").expect("get recipe");
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.name, "Test Recipe");
        assert_eq!(retrieved.author_id, "author-1");

        // List by author
        let list = repo
            .list_recipes_by_author("author-1")
            .expect("list recipes");
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn organism_lifecycle() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite repo");
        let now = Utc::now();

        // First create a recipe (organism requires recipe)
        let recipe = RecipeRow {
            recipe_id: "recipe-org-1".to_string(),
            name: "Org Test Recipe".to_string(),
            description: None,
            gene_sequence_json: r#"{"steps": ["step1", "step2", "step3"]}"#.to_string(),
            author_id: "author-1".to_string(),
            forked_from: None,
            created_at: now,
            updated_at: now,
            is_public: false,
        };
        repo.create_recipe(&recipe).expect("create recipe");

        // Create organism
        let organism = OrganismRow {
            organism_id: "organism-1".to_string(),
            recipe_id: "recipe-org-1".to_string(),
            status: "running".to_string(),
            current_step: 0,
            total_steps: 3,
            created_at: now,
            completed_at: None,
        };
        repo.create_organism(&organism).expect("create organism");

        // Get organism
        let retrieved = repo.get_organism("organism-1").expect("get organism");
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.status, "running");
        assert_eq!(retrieved.current_step, 0);

        // Update status
        let updated = repo
            .update_organism_status("organism-1", "running", 1)
            .expect("update status");
        assert!(updated);

        // Verify update
        let after = repo
            .get_organism("organism-1")
            .expect("get after update")
            .unwrap();
        assert_eq!(after.current_step, 1);

        // Complete organism
        let completed = repo
            .update_organism_status("organism-1", "completed", 3)
            .expect("complete");
        assert!(completed);

        let final_state = repo.get_organism("organism-1").expect("get final").unwrap();
        assert_eq!(final_state.status, "completed");
        assert!(final_state.completed_at.is_some());
    }

    #[test]
    fn recipe_fork_lifecycle() {
        let repo = SqliteRuntimeRepository::new(":memory:").expect("create sqlite repo");
        let now = Utc::now();

        // Create original recipe
        let original = RecipeRow {
            recipe_id: "original-recipe".to_string(),
            name: "Original".to_string(),
            description: Some("Original recipe".to_string()),
            gene_sequence_json: r#"{"version": 1}"#.to_string(),
            author_id: "author-original".to_string(),
            forked_from: None,
            created_at: now,
            updated_at: now,
            is_public: true,
        };
        repo.create_recipe(&original).expect("create original");

        // Fork the recipe
        let forked = RecipeRow {
            recipe_id: "forked-recipe".to_string(),
            name: "Forked Original".to_string(),
            description: Some("Forked from original".to_string()),
            gene_sequence_json: r#"{"version": 1, "forked": true}"#.to_string(),
            author_id: "author-forker".to_string(),
            forked_from: Some("original-recipe".to_string()),
            created_at: now,
            updated_at: now,
            is_public: true,
        };
        repo.create_recipe(&forked).expect("create fork");

        // Verify fork
        let retrieved = repo.get_recipe("forked-recipe").expect("get fork").unwrap();
        assert_eq!(retrieved.forked_from, Some("original-recipe".to_string()));
        assert_eq!(retrieved.author_id, "author-forker");
    }
}
