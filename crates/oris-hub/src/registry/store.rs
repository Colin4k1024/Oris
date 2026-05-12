use async_trait::async_trait;
use chrono::Utc;
use rusqlite::Connection;
use std::sync::Mutex;

use super::types::{NodeInfo, NodeStatus};
use crate::error::HubError;

#[async_trait]
pub trait RegistryStore: Send + Sync {
    async fn upsert_node(&self, node: &NodeInfo) -> Result<(), HubError>;
    async fn get_node(&self, node_id: &str) -> Result<Option<NodeInfo>, HubError>;
    async fn list_nodes(&self) -> Result<Vec<NodeInfo>, HubError>;
    async fn refresh_heartbeat(
        &self,
        node_id: &str,
        status: Option<NodeStatus>,
    ) -> Result<(), HubError>;
    async fn remove_node(&self, node_id: &str) -> Result<(), HubError>;
    async fn gc_expired_nodes(&self) -> Result<u64, HubError>;
}

pub struct SqliteRegistryStore {
    conn: Mutex<Connection>,
}

impl SqliteRegistryStore {
    pub fn new(path: &str) -> Result<Self, HubError> {
        let conn = if path == ":memory:" {
            Connection::open_in_memory()
        } else {
            Connection::open(path)
        }
        .map_err(|e| HubError::Storage(e.to_string()))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS nodes (
                node_id TEXT PRIMARY KEY,
                endpoint TEXT NOT NULL,
                public_key TEXT NOT NULL,
                capabilities TEXT NOT NULL DEFAULT '[]',
                region TEXT,
                version TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                registered_at TEXT NOT NULL,
                last_heartbeat TEXT NOT NULL,
                ttl_seconds INTEGER NOT NULL DEFAULT 60
            );",
        )
        .map_err(|e| HubError::Storage(e.to_string()))?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

#[async_trait]
impl RegistryStore for SqliteRegistryStore {
    async fn upsert_node(&self, node: &NodeInfo) -> Result<(), HubError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| HubError::Storage(e.to_string()))?;
        let capabilities_json = serde_json::to_string(&node.capabilities)
            .map_err(|e| HubError::Storage(e.to_string()))?;
        let status_str =
            serde_json::to_string(&node.status).map_err(|e| HubError::Storage(e.to_string()))?;

        conn.execute(
            "INSERT INTO nodes (node_id, endpoint, public_key, capabilities, region, version, status, registered_at, last_heartbeat, ttl_seconds)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(node_id) DO UPDATE SET
                endpoint = excluded.endpoint,
                public_key = excluded.public_key,
                capabilities = excluded.capabilities,
                region = excluded.region,
                version = excluded.version,
                status = excluded.status,
                last_heartbeat = excluded.last_heartbeat,
                ttl_seconds = excluded.ttl_seconds",
            rusqlite::params![
                node.node_id,
                node.endpoint,
                node.public_key,
                capabilities_json,
                node.region,
                node.version,
                status_str.trim_matches('"'),
                node.registered_at.to_rfc3339(),
                node.last_heartbeat.to_rfc3339(),
                node.ttl_seconds,
            ],
        )?;
        Ok(())
    }

    async fn get_node(&self, node_id: &str) -> Result<Option<NodeInfo>, HubError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| HubError::Storage(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT node_id, endpoint, public_key, capabilities, region, version, status, registered_at, last_heartbeat, ttl_seconds FROM nodes WHERE node_id = ?1"
        )?;

        let result = stmt.query_row(rusqlite::params![node_id], |row| Ok(row_to_node_info(row)));

        match result {
            Ok(node) => Ok(Some(node?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(HubError::Storage(e.to_string())),
        }
    }

    async fn list_nodes(&self) -> Result<Vec<NodeInfo>, HubError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| HubError::Storage(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT node_id, endpoint, public_key, capabilities, region, version, status, registered_at, last_heartbeat, ttl_seconds FROM nodes"
        )?;

        let nodes = stmt
            .query_map([], |row| Ok(row_to_node_info(row)))
            .map_err(|e| HubError::Storage(e.to_string()))?
            .filter_map(|r| r.ok())
            .filter_map(|r| r.ok())
            .collect();

        Ok(nodes)
    }

    async fn refresh_heartbeat(
        &self,
        node_id: &str,
        status: Option<NodeStatus>,
    ) -> Result<(), HubError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| HubError::Storage(e.to_string()))?;
        let now = Utc::now().to_rfc3339();

        if let Some(s) = status {
            let status_str =
                serde_json::to_string(&s).map_err(|e| HubError::Storage(e.to_string()))?;
            conn.execute(
                "UPDATE nodes SET last_heartbeat = ?1, status = ?2 WHERE node_id = ?3",
                rusqlite::params![now, status_str.trim_matches('"'), node_id],
            )?;
        } else {
            conn.execute(
                "UPDATE nodes SET last_heartbeat = ?1 WHERE node_id = ?2",
                rusqlite::params![now, node_id],
            )?;
        }
        Ok(())
    }

    async fn remove_node(&self, node_id: &str) -> Result<(), HubError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| HubError::Storage(e.to_string()))?;
        conn.execute(
            "DELETE FROM nodes WHERE node_id = ?1",
            rusqlite::params![node_id],
        )?;
        Ok(())
    }

    async fn gc_expired_nodes(&self) -> Result<u64, HubError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| HubError::Storage(e.to_string()))?;
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let deleted = conn.execute(
            "DELETE FROM nodes WHERE datetime(substr(last_heartbeat, 1, 19), '+' || ttl_seconds || ' seconds') < datetime(?1)",
            rusqlite::params![now],
        )?;
        Ok(deleted as u64)
    }
}

fn row_to_node_info(row: &rusqlite::Row) -> Result<NodeInfo, HubError> {
    let capabilities_str: String = row.get(3).map_err(|e| HubError::Storage(e.to_string()))?;
    let capabilities: Vec<String> = serde_json::from_str(&capabilities_str).unwrap_or_default();
    let status_str: String = row.get(6).map_err(|e| HubError::Storage(e.to_string()))?;
    let status: NodeStatus =
        serde_json::from_str(&format!("\"{}\"", status_str)).unwrap_or(NodeStatus::Active);
    let registered_str: String = row.get(7).map_err(|e| HubError::Storage(e.to_string()))?;
    let heartbeat_str: String = row.get(8).map_err(|e| HubError::Storage(e.to_string()))?;

    Ok(NodeInfo {
        node_id: row.get(0).map_err(|e| HubError::Storage(e.to_string()))?,
        endpoint: row.get(1).map_err(|e| HubError::Storage(e.to_string()))?,
        public_key: row.get(2).map_err(|e| HubError::Storage(e.to_string()))?,
        capabilities,
        region: row.get(4).map_err(|e| HubError::Storage(e.to_string()))?,
        version: row.get(5).map_err(|e| HubError::Storage(e.to_string()))?,
        status,
        registered_at: chrono::DateTime::parse_from_rfc3339(&registered_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        last_heartbeat: chrono::DateTime::parse_from_rfc3339(&heartbeat_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        ttl_seconds: row
            .get::<_, i64>(9)
            .map_err(|e| HubError::Storage(e.to_string()))? as u64,
    })
}
