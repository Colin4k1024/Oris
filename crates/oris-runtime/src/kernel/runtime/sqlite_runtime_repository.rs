//! SQLite-backed runtime repository for Phase 3 worker APIs.

#![cfg(feature = "sqlite-persistence")]

use std::sync::{Arc, Mutex};

use chrono::{DateTime, TimeZone, Utc};
use rusqlite::{params, Connection, ErrorCode};

use crate::kernel::event::KernelError;
use crate::kernel::identity::{RunId, Seq};

use super::models::{AttemptDispatchRecord, AttemptExecutionStatus, LeaseRecord};
use super::repository::RuntimeRepository;

#[derive(Clone)]
pub struct SqliteRuntimeRepository {
    conn: Arc<Mutex<Connection>>,
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
              created_at_ms INTEGER NOT NULL,
              resume_payload_hash TEXT NULL,
              resume_response_json TEXT NULL,
              resumed_at_ms INTEGER NULL
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
              role TEXT NOT NULL DEFAULT 'operator',
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
        .map_err(|e| KernelError::Driver(format!("init sqlite runtime schema: {}", e)))?;
        // Backward-compatible upgrades for older local SQLite files.
        add_column_if_missing(
            &conn,
            "runtime_interrupts",
            "resume_payload_hash",
            "TEXT NULL",
        )?;
        add_column_if_missing(
            &conn,
            "runtime_interrupts",
            "resume_response_json",
            "TEXT NULL",
        )?;
        add_column_if_missing(&conn, "runtime_interrupts", "resumed_at_ms", "INTEGER NULL")?;
        add_column_if_missing(
            &conn,
            "runtime_api_keys",
            "role",
            "TEXT NOT NULL DEFAULT 'operator'",
        )?;
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
            "UPDATE runtime_attempts SET status = ?2 WHERE attempt_id = ?1",
            params![attempt_id, status_str],
        )
        .map_err(|e| KernelError::Driver(format!("mark attempt status: {}", e)))?;
        Ok(())
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
        let conn = self
            .conn
            .lock()
            .map_err(|_| KernelError::Driver("sqlite runtime repo lock poisoned".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT audit_id, actor_type, actor_id, actor_role, action, resource_type, resource_id, result, request_id, details_json, created_at_ms
                 FROM runtime_audit_logs
                 ORDER BY audit_id DESC
                 LIMIT ?1",
            )
            .map_err(|e| KernelError::Driver(format!("prepare list_audit_logs: {}", e)))?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
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
            })
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
                 ORDER BY a.attempt_no ASC, a.attempt_id ASC
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
                 SET status = 'leased'
                 WHERE attempt_id = ?1 AND status IN ('queued', 'retry_backoff')",
                params![attempt_id],
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

fn map_rusqlite_err(err: rusqlite::Error) -> KernelError {
    KernelError::Driver(format!("sqlite runtime repo: {}", err))
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
