//! SQLite-backed kernel stores for event log and snapshots.
//!
//! This module is feature-gated behind `sqlite-persistence`.

#[cfg(feature = "sqlite-persistence")]
use std::marker::PhantomData;
#[cfg(feature = "sqlite-persistence")]
use std::path::{Path, PathBuf};
#[cfg(feature = "sqlite-persistence")]
use std::sync::Mutex;
#[cfg(feature = "sqlite-persistence")]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "sqlite-persistence")]
use rusqlite::{params, Connection, OptionalExtension};
#[cfg(feature = "sqlite-persistence")]
use serde::{de::DeserializeOwned, Serialize};

#[cfg(feature = "sqlite-persistence")]
use crate::kernel::event::{Event, EventStore, KernelError, SequencedEvent};
#[cfg(feature = "sqlite-persistence")]
use crate::kernel::identity::{RunId, Seq};
#[cfg(feature = "sqlite-persistence")]
use crate::kernel::snapshot::{Snapshot, SnapshotStore};

#[cfg(feature = "sqlite-persistence")]
fn map_event_err(prefix: &str, err: impl std::fmt::Display) -> KernelError {
    KernelError::EventStore(format!("{prefix}: {err}"))
}

#[cfg(feature = "sqlite-persistence")]
fn map_snapshot_err(prefix: &str, err: impl std::fmt::Display) -> KernelError {
    KernelError::SnapshotStore(format!("{prefix}: {err}"))
}

#[cfg(feature = "sqlite-persistence")]
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// SQLite-backed event log store.
#[cfg(feature = "sqlite-persistence")]
pub struct SqliteEventStore {
    db_path: PathBuf,
    lock: Mutex<()>,
}

#[cfg(feature = "sqlite-persistence")]
impl SqliteEventStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            db_path: path.into(),
            lock: Mutex::new(()),
        }
    }

    fn open_connection(&self) -> Result<Connection, KernelError> {
        if let Some(parent) = Path::new(&self.db_path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| map_event_err("create parent dir", e))?;
        }
        let conn =
            Connection::open(&self.db_path).map_err(|e| map_event_err("open sqlite db", e))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| map_event_err("set journal_mode", e))?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(|e| map_event_err("set synchronous", e))?;
        self.ensure_schema(&conn)?;
        Ok(conn)
    }

    fn ensure_schema(&self, conn: &Connection) -> Result<(), KernelError> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS kernel_events (
                run_id TEXT NOT NULL,
                seq INTEGER NOT NULL,
                event_json TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                PRIMARY KEY (run_id, seq)
            );
            CREATE INDEX IF NOT EXISTS idx_kernel_events_run_seq
            ON kernel_events (run_id, seq);
            ",
        )
        .map_err(|e| map_event_err("ensure schema", e))?;
        Ok(())
    }
}

#[cfg(feature = "sqlite-persistence")]
impl EventStore for SqliteEventStore {
    fn append(&self, run_id: &RunId, events: &[Event]) -> Result<Seq, KernelError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| map_event_err("lock poisoned", "FATAL: mutex poisoned - previous holder panicked while holding lock; state may be corrupted"))?;
        let mut conn = self.open_connection()?;

        if events.is_empty() {
            let head: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(seq), 0) FROM kernel_events WHERE run_id = ?1",
                    params![run_id],
                    |row| row.get(0),
                )
                .map_err(|e| map_event_err("read head", e))?;
            return Ok(head as Seq);
        }

        let tx = conn
            .transaction()
            .map_err(|e| map_event_err("begin tx", e))?;
        let current_head: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) FROM kernel_events WHERE run_id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .map_err(|e| map_event_err("read head", e))?;

        let mut last_seq = current_head as Seq;
        for (idx, event) in events.iter().enumerate() {
            let seq = current_head + idx as i64 + 1;
            let json =
                serde_json::to_string(event).map_err(|e| map_event_err("serialize event", e))?;
            tx.execute(
                "INSERT INTO kernel_events (run_id, seq, event_json, created_at_ms) VALUES (?1, ?2, ?3, ?4)",
                params![run_id, seq, json, now_ms()],
            )
            .map_err(|e| map_event_err("insert event", e))?;
            last_seq = seq as Seq;
        }

        tx.commit().map_err(|e| map_event_err("commit tx", e))?;
        Ok(last_seq)
    }

    fn scan(&self, run_id: &RunId, from: Seq) -> Result<Vec<SequencedEvent>, KernelError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| map_event_err("lock poisoned", "FATAL: mutex poisoned - previous holder panicked while holding lock; state may be corrupted"))?;
        let conn = self.open_connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT seq, event_json FROM kernel_events
                 WHERE run_id = ?1 AND seq >= ?2
                 ORDER BY seq ASC",
            )
            .map_err(|e| map_event_err("prepare scan", e))?;
        let rows = stmt
            .query_map(params![run_id, from as i64], |row| {
                let seq: i64 = row.get(0)?;
                let json: String = row.get(1)?;
                let event: Event = serde_json::from_str(&json).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        json.len(),
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?;
                Ok(SequencedEvent {
                    seq: seq as Seq,
                    event,
                })
            })
            .map_err(|e| map_event_err("query scan", e))?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| map_event_err("row decode", e))?);
        }
        Ok(out)
    }

    fn head(&self, run_id: &RunId) -> Result<Seq, KernelError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| map_event_err("lock poisoned", "FATAL: mutex poisoned - previous holder panicked while holding lock; state may be corrupted"))?;
        let conn = self.open_connection()?;
        let head: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) FROM kernel_events WHERE run_id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .map_err(|e| map_event_err("read head", e))?;
        Ok(head as Seq)
    }
}

/// SQLite-backed snapshot store.
#[cfg(feature = "sqlite-persistence")]
pub struct SqliteSnapshotStore<S> {
    db_path: PathBuf,
    lock: Mutex<()>,
    _state: PhantomData<S>,
}

#[cfg(feature = "sqlite-persistence")]
impl<S> SqliteSnapshotStore<S> {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            db_path: path.into(),
            lock: Mutex::new(()),
            _state: PhantomData,
        }
    }

    fn open_connection(&self) -> Result<Connection, KernelError> {
        if let Some(parent) = Path::new(&self.db_path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| map_snapshot_err("create parent dir", e))?;
        }
        let conn =
            Connection::open(&self.db_path).map_err(|e| map_snapshot_err("open sqlite db", e))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| map_snapshot_err("set journal_mode", e))?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(|e| map_snapshot_err("set synchronous", e))?;
        self.ensure_schema(&conn)?;
        Ok(conn)
    }

    fn ensure_schema(&self, conn: &Connection) -> Result<(), KernelError> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS kernel_snapshots (
                run_id TEXT NOT NULL,
                at_seq INTEGER NOT NULL,
                state_json TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                PRIMARY KEY (run_id, at_seq)
            );
            CREATE INDEX IF NOT EXISTS idx_kernel_snapshots_run_seq
            ON kernel_snapshots (run_id, at_seq DESC);
            ",
        )
        .map_err(|e| map_snapshot_err("ensure schema", e))?;
        Ok(())
    }
}

#[cfg(feature = "sqlite-persistence")]
impl<S> SnapshotStore<S> for SqliteSnapshotStore<S>
where
    S: Clone + Send + Sync + Serialize + DeserializeOwned + 'static,
{
    fn load_latest(&self, run_id: &RunId) -> Result<Option<Snapshot<S>>, KernelError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| map_snapshot_err("lock poisoned", "FATAL: mutex poisoned - previous holder panicked while holding lock; state may be corrupted"))?;
        let conn = self.open_connection()?;
        let row = conn
            .query_row(
                "SELECT at_seq, state_json
                 FROM kernel_snapshots
                 WHERE run_id = ?1
                 ORDER BY at_seq DESC
                 LIMIT 1",
                params![run_id],
                |row| {
                    let at_seq: i64 = row.get(0)?;
                    let state_json: String = row.get(1)?;
                    Ok((at_seq, state_json))
                },
            )
            .optional()
            .map_err(|e| map_snapshot_err("load latest snapshot", e))?;

        match row {
            Some((at_seq, state_json)) => {
                let state = serde_json::from_str(&state_json)
                    .map_err(|e| map_snapshot_err("decode state", e))?;
                Ok(Some(Snapshot {
                    run_id: run_id.clone(),
                    at_seq: at_seq as Seq,
                    state,
                }))
            }
            None => Ok(None),
        }
    }

    fn save(&self, snapshot: &Snapshot<S>) -> Result<(), KernelError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| map_snapshot_err("lock poisoned", "FATAL: mutex poisoned - previous holder panicked while holding lock; state may be corrupted"))?;
        let conn = self.open_connection()?;
        let json = serde_json::to_string(&snapshot.state)
            .map_err(|e| map_snapshot_err("encode state", e))?;
        conn.execute(
            "INSERT INTO kernel_snapshots (run_id, at_seq, state_json, created_at_ms)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (run_id, at_seq)
             DO UPDATE SET state_json = excluded.state_json, created_at_ms = excluded.created_at_ms",
            params![snapshot.run_id, snapshot.at_seq as i64, json, now_ms()],
        )
        .map_err(|e| map_snapshot_err("save snapshot", e))?;
        Ok(())
    }
}

#[cfg(all(test, feature = "sqlite-persistence"))]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{SqliteEventStore, SqliteSnapshotStore};
    use crate::kernel::{Event, EventStore, Snapshot, SnapshotStore};

    fn test_db_path(name: &str) -> std::path::PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        std::env::temp_dir().join(format!("oris-kernel-{name}-{ts}.sqlite"))
    }

    #[test]
    fn sqlite_event_store_roundtrip() {
        let path = test_db_path("events");
        let store = SqliteEventStore::new(&path);
        let run_id = "run-sqlite-events".to_string();

        assert_eq!(store.head(&run_id).unwrap(), 0);
        let seq = store
            .append(
                &run_id,
                &[Event::StateUpdated {
                    step_id: Some("n1".into()),
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

    #[test]
    fn sqlite_snapshot_store_roundtrip() {
        let path = test_db_path("snapshots");
        let store: SqliteSnapshotStore<serde_json::Value> = SqliteSnapshotStore::new(&path);
        let run_id = "run-sqlite-snapshots".to_string();

        store
            .save(&Snapshot {
                run_id: run_id.clone(),
                at_seq: 9,
                state: serde_json::json!({"k": "v"}),
            })
            .unwrap();

        let latest = store.load_latest(&run_id).unwrap().unwrap();
        assert_eq!(latest.at_seq, 9);
        assert_eq!(latest.state["k"], "v");
    }

    // -----------------------------------------------------------------------
    // Issue #373: SQLite crash-recovery tests
    // -----------------------------------------------------------------------

    #[test]
    fn crash_recovery_events_survive_reopen() {
        let path = test_db_path("crash-events");
        let run_id = "run-crash-events".to_string();

        // Phase 1: Write events then "crash" (drop store)
        {
            let store = SqliteEventStore::new(&path);
            store
                .append(
                    &run_id,
                    &[
                        Event::StateUpdated {
                            step_id: Some("n1".into()),
                            payload: serde_json::json!({"v": 1}),
                        },
                        Event::StateUpdated {
                            step_id: Some("n2".into()),
                            payload: serde_json::json!({"v": 2}),
                        },
                        Event::StateUpdated {
                            step_id: Some("n3".into()),
                            payload: serde_json::json!({"v": 3}),
                        },
                    ],
                )
                .unwrap();
        } // store dropped — simulates crash

        // Phase 2: Reopen and verify all events survived
        {
            let store = SqliteEventStore::new(&path);
            assert_eq!(store.head(&run_id).unwrap(), 3);

            let events = store.scan(&run_id, 1).unwrap();
            assert_eq!(events.len(), 3);
            assert_eq!(events[0].seq, 1);
            assert_eq!(events[2].seq, 3);
        }
    }

    #[test]
    fn crash_recovery_snapshots_survive_reopen() {
        let path = test_db_path("crash-snapshots");
        let run_id = "run-crash-snapshots".to_string();

        // Phase 1: Save snapshot then "crash"
        {
            let store: SqliteSnapshotStore<serde_json::Value> = SqliteSnapshotStore::new(&path);
            store
                .save(&Snapshot {
                    run_id: run_id.clone(),
                    at_seq: 5,
                    state: serde_json::json!({"counter": 42}),
                })
                .unwrap();
        } // store dropped

        // Phase 2: Reopen and verify snapshot survived
        {
            let store: SqliteSnapshotStore<serde_json::Value> = SqliteSnapshotStore::new(&path);
            let snap = store.load_latest(&run_id).unwrap().unwrap();
            assert_eq!(snap.at_seq, 5);
            assert_eq!(snap.state["counter"], 42);
        }
    }

    #[test]
    fn crash_recovery_replay_from_snapshot_after_reopen() {
        let path = test_db_path("crash-replay");
        let run_id = "run-crash-replay".to_string();

        // Phase 1: Write events and snapshot, then "crash"
        {
            let event_store = SqliteEventStore::new(&path);
            let snap_store: SqliteSnapshotStore<serde_json::Value> =
                SqliteSnapshotStore::new(&path);

            // Append 5 events
            event_store
                .append(
                    &run_id,
                    &[
                        Event::StateUpdated {
                            step_id: Some("n1".into()),
                            payload: serde_json::json!({"v": 1}),
                        },
                        Event::StateUpdated {
                            step_id: Some("n2".into()),
                            payload: serde_json::json!({"v": 2}),
                        },
                    ],
                )
                .unwrap();

            // Save snapshot at seq 2
            snap_store
                .save(&Snapshot {
                    run_id: run_id.clone(),
                    at_seq: 2,
                    state: serde_json::json!({"v": 2}),
                })
                .unwrap();

            // Append 3 more events (tail beyond snapshot)
            event_store
                .append(
                    &run_id,
                    &[
                        Event::StateUpdated {
                            step_id: Some("n3".into()),
                            payload: serde_json::json!({"v": 3}),
                        },
                        Event::StateUpdated {
                            step_id: Some("n4".into()),
                            payload: serde_json::json!({"v": 4}),
                        },
                        Event::Completed,
                    ],
                )
                .unwrap();
        } // stores dropped — crash

        // Phase 2: Reopen and verify recovery path
        {
            let event_store = SqliteEventStore::new(&path);
            let snap_store: SqliteSnapshotStore<serde_json::Value> =
                SqliteSnapshotStore::new(&path);

            // Load snapshot — should be at seq 2
            let snap = snap_store.load_latest(&run_id).unwrap().unwrap();
            assert_eq!(snap.at_seq, 2);

            // Scan tail events (seq > 2)
            let tail = event_store.scan(&run_id, snap.at_seq + 1).unwrap();
            assert_eq!(tail.len(), 3); // events 3, 4, 5

            // Verify total event count
            let all = event_store.scan(&run_id, 1).unwrap();
            assert_eq!(all.len(), 5);
            assert_eq!(event_store.head(&run_id).unwrap(), 5);
        }
    }

    #[test]
    fn crash_recovery_multiple_snapshots_latest_wins() {
        let path = test_db_path("crash-multi-snap");
        let run_id = "run-crash-multi".to_string();

        // Phase 1: Save multiple snapshots
        {
            let store: SqliteSnapshotStore<serde_json::Value> = SqliteSnapshotStore::new(&path);
            store
                .save(&Snapshot {
                    run_id: run_id.clone(),
                    at_seq: 3,
                    state: serde_json::json!({"v": 3}),
                })
                .unwrap();
            store
                .save(&Snapshot {
                    run_id: run_id.clone(),
                    at_seq: 7,
                    state: serde_json::json!({"v": 7}),
                })
                .unwrap();
            store
                .save(&Snapshot {
                    run_id: run_id.clone(),
                    at_seq: 5,
                    state: serde_json::json!({"v": 5}),
                })
                .unwrap();
        } // crash

        // Phase 2: Latest snapshot (highest at_seq) should be returned
        {
            let store: SqliteSnapshotStore<serde_json::Value> = SqliteSnapshotStore::new(&path);
            let snap = store.load_latest(&run_id).unwrap().unwrap();
            assert_eq!(snap.at_seq, 7);
            assert_eq!(snap.state["v"], 7);
        }
    }

    #[test]
    fn crash_recovery_append_after_reopen_continues_sequence() {
        let path = test_db_path("crash-append");
        let run_id = "run-crash-append".to_string();

        // Phase 1: Append some events
        {
            let store = SqliteEventStore::new(&path);
            store
                .append(
                    &run_id,
                    &[Event::StateUpdated {
                        step_id: Some("n1".into()),
                        payload: serde_json::json!({"v": 1}),
                    }],
                )
                .unwrap();
        } // crash

        // Phase 2: Reopen and append more — sequence must continue
        {
            let store = SqliteEventStore::new(&path);
            let seq = store
                .append(
                    &run_id,
                    &[Event::StateUpdated {
                        step_id: Some("n2".into()),
                        payload: serde_json::json!({"v": 2}),
                    }],
                )
                .unwrap();
            assert_eq!(seq, 2); // continues from 1
            assert_eq!(store.head(&run_id).unwrap(), 2);

            let all = store.scan(&run_id, 1).unwrap();
            assert_eq!(all.len(), 2);
        }
    }
}
