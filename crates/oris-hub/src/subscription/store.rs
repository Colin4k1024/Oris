use chrono::Utc;
use rusqlite::Connection;
use std::sync::Mutex;
use uuid::Uuid;

use super::types::{CreateSubscriptionRequest, Subscription};
use crate::error::HubError;

pub struct SubscriptionStore {
    conn: Mutex<Connection>,
}

impl SubscriptionStore {
    pub fn new(path: &str) -> Result<Self, HubError> {
        let conn = if path == ":memory:" {
            Connection::open_in_memory()
        } else {
            Connection::open(path)
        }
        .map_err(|e| HubError::Storage(e.to_string()))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS subscriptions (
                id TEXT PRIMARY KEY,
                subscriber_node_id TEXT NOT NULL,
                callback_url TEXT NOT NULL,
                filter_task_class TEXT,
                filter_min_confidence REAL,
                filter_source_nodes TEXT,
                created_at TEXT NOT NULL,
                active INTEGER NOT NULL DEFAULT 1
            );",
        )
        .map_err(|e| HubError::Storage(e.to_string()))?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn create(&self, req: &CreateSubscriptionRequest) -> Result<Subscription, HubError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| HubError::Storage(e.to_string()))?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let source_nodes_json = req
            .filter
            .source_nodes
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());

        conn.execute(
            "INSERT INTO subscriptions (id, subscriber_node_id, callback_url, filter_task_class, filter_min_confidence, filter_source_nodes, created_at, active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)",
            rusqlite::params![
                id,
                req.subscriber_node_id,
                req.callback_url,
                req.filter.task_class,
                req.filter.min_confidence,
                source_nodes_json,
                now.to_rfc3339(),
            ],
        )?;

        Ok(Subscription {
            id,
            subscriber_node_id: req.subscriber_node_id.clone(),
            callback_url: req.callback_url.clone(),
            filter: req.filter.clone(),
            created_at: now,
            active: true,
        })
    }

    pub fn list(&self, subscriber_node_id: Option<&str>) -> Result<Vec<Subscription>, HubError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| HubError::Storage(e.to_string()))?;

        let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match subscriber_node_id {
            Some(node_id) => (
                "SELECT id, subscriber_node_id, callback_url, filter_task_class, filter_min_confidence, filter_source_nodes, created_at, active FROM subscriptions WHERE subscriber_node_id = ?1 AND active = 1",
                vec![Box::new(node_id.to_string())],
            ),
            None => (
                "SELECT id, subscriber_node_id, callback_url, filter_task_class, filter_min_confidence, filter_source_nodes, created_at, active FROM subscriptions WHERE active = 1",
                vec![],
            ),
        };

        let mut stmt = conn.prepare(sql)?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(params_refs.as_slice(), |row| Ok(row_to_subscription(row)))
            .map_err(|e| HubError::Storage(e.to_string()))?
            .filter_map(|r| r.ok())
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows)
    }

    pub fn list_active(&self) -> Result<Vec<Subscription>, HubError> {
        self.list(None)
    }

    pub fn delete(&self, id: &str) -> Result<(), HubError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| HubError::Storage(e.to_string()))?;
        conn.execute(
            "UPDATE subscriptions SET active = 0 WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<Subscription>, HubError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| HubError::Storage(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, subscriber_node_id, callback_url, filter_task_class, filter_min_confidence, filter_source_nodes, created_at, active FROM subscriptions WHERE id = ?1"
        )?;

        let result = stmt.query_row(rusqlite::params![id], |row| Ok(row_to_subscription(row)));

        match result {
            Ok(sub) => Ok(Some(sub?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(HubError::Storage(e.to_string())),
        }
    }
}

fn row_to_subscription(row: &rusqlite::Row) -> Result<Subscription, HubError> {
    use crate::subscription::types::SubscriptionFilter;

    let source_nodes_str: Option<String> =
        row.get(5).map_err(|e| HubError::Storage(e.to_string()))?;
    let source_nodes: Option<Vec<String>> =
        source_nodes_str.and_then(|s| serde_json::from_str(&s).ok());

    let created_str: String = row.get(6).map_err(|e| HubError::Storage(e.to_string()))?;
    let active_int: i32 = row.get(7).map_err(|e| HubError::Storage(e.to_string()))?;

    Ok(Subscription {
        id: row.get(0).map_err(|e| HubError::Storage(e.to_string()))?,
        subscriber_node_id: row.get(1).map_err(|e| HubError::Storage(e.to_string()))?,
        callback_url: row.get(2).map_err(|e| HubError::Storage(e.to_string()))?,
        filter: SubscriptionFilter {
            task_class: row.get(3).map_err(|e| HubError::Storage(e.to_string()))?,
            min_confidence: row.get(4).map_err(|e| HubError::Storage(e.to_string()))?,
            source_nodes,
        },
        created_at: chrono::DateTime::parse_from_rfc3339(&created_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now()),
        active: active_int != 0,
    })
}
