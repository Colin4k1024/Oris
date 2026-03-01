//! SQLite-backed runtime repository for Phase 3 worker APIs.

#![cfg(feature = "sqlite-persistence")]

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, TimeZone, Utc};
use rusqlite::{params, Connection, ErrorCode, OptionalExtension};

use crate::kernel::event::KernelError;
use crate::kernel::identity::{RunId, Seq};

use super::models::{AttemptDispatchRecord, AttemptExecutionStatus, LeaseRecord};
use super::repository::RuntimeRepository;

const SQLITE_RUNTIME_SCHEMA_VERSION: i64 = 9;

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

    fn expire_leases_and_requeue(&self, now: DateTime<Utc>) -> Result<u64, KernelError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut stmt = conn
            .prepare("SELECT attempt_id FROM runtime_leases WHERE lease_expires_at_ms < ?1")
            .map_err(|e| KernelError::Driver(format!("prepare expired lease query: {}", e)))?;
        let rows = stmt
            .query_map(params![dt_to_ms(now)], |r| r.get::<_, String>(0))
            .map_err(|e| KernelError::Driver(format!("query expired leases: {}", e)))?;
        let mut expired_attempts = Vec::new();
        for row in rows {
            expired_attempts.push(row.map_err(map_rusqlite_err)?);
        }
        for attempt_id in &expired_attempts {
            conn.execute(
                "DELETE FROM runtime_leases WHERE attempt_id = ?1",
                params![attempt_id],
            )
            .map_err(|e| KernelError::Driver(format!("delete expired lease: {}", e)))?;
            conn.execute(
                "UPDATE runtime_attempts
                 SET status = 'queued'
                 WHERE attempt_id = ?1
                   AND status NOT IN ('completed', 'failed', 'cancelled')",
                params![attempt_id],
            )
            .map_err(|e| KernelError::Driver(format!("requeue attempt: {}", e)))?;
        }
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
        ReplayEffectClaim, RetryPolicyConfig, RetryStrategy, SqliteRuntimeRepository,
        TimeoutPolicyConfig, SQLITE_RUNTIME_SCHEMA_VERSION,
    };
    use crate::kernel::runtime::models::AttemptExecutionStatus;
    use crate::kernel::runtime::repository::RuntimeRepository;

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

        let _ = fs::remove_file(path);
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
}
