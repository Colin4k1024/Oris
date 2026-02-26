//! SQLite-backed idempotency helper for execution API.

use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension};

#[derive(Clone, Debug)]
pub struct IdempotencyRecord {
    pub operation: String,
    pub thread_id: String,
    pub payload_hash: String,
    pub response_json: String,
}

#[derive(Clone)]
pub struct SqliteIdempotencyStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteIdempotencyStore {
    pub fn new(db_path: &str) -> Result<Self, String> {
        let conn = Connection::open(db_path)
            .map_err(|e| format!("failed to open idempotency sqlite db: {}", e))?;
        let this = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        this.ensure_schema()?;
        Ok(this)
    }

    fn ensure_schema(&self) -> Result<(), String> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| "idempotency sqlite lock poisoned".to_string())?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS execution_idempotency (
              idempotency_key TEXT PRIMARY KEY,
              operation TEXT NOT NULL,
              thread_id TEXT NOT NULL,
              payload_hash TEXT NOT NULL,
              response_json TEXT NOT NULL,
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );
            "#,
        )
        .map_err(|e| format!("failed to init idempotency schema: {}", e))?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<Option<IdempotencyRecord>, String> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| "idempotency sqlite lock poisoned".to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT operation, thread_id, payload_hash, response_json
                 FROM execution_idempotency WHERE idempotency_key = ?1",
            )
            .map_err(|e| format!("failed to prepare idempotency get: {}", e))?;
        let row = stmt
            .query_row(params![key], |r| {
                Ok(IdempotencyRecord {
                    operation: r.get(0)?,
                    thread_id: r.get(1)?,
                    payload_hash: r.get(2)?,
                    response_json: r.get(3)?,
                })
            })
            .optional()
            .map_err(|e| format!("failed to query idempotency key: {}", e))?;
        Ok(row)
    }

    pub fn put(&self, key: &str, record: &IdempotencyRecord) -> Result<(), String> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| "idempotency sqlite lock poisoned".to_string())?;
        conn.execute(
            "INSERT OR REPLACE INTO execution_idempotency
             (idempotency_key, operation, thread_id, payload_hash, response_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                key,
                record.operation,
                record.thread_id,
                record.payload_hash,
                record.response_json
            ],
        )
        .map_err(|e| format!("failed to persist idempotency key: {}", e))?;
        Ok(())
    }
}
