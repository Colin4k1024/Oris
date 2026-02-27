//! Runtime storage backend selection and startup health checks.

use std::collections::HashMap;

#[cfg(feature = "kernel-postgres")]
use chrono::Utc;

#[cfg(feature = "kernel-postgres")]
use sqlx::postgres::PgPoolOptions;

#[cfg(feature = "kernel-postgres")]
use super::PostgresRuntimeRepository;
#[cfg(feature = "kernel-postgres")]
use super::RuntimeRepository;
use super::SqliteRuntimeRepository;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeStorageBackend {
    Sqlite,
    Postgres,
}

#[derive(Clone, Debug)]
pub struct RuntimeStorageConfig {
    pub backend: RuntimeStorageBackend,
    pub sqlite_db_path: String,
    pub postgres_dsn: Option<String>,
    pub postgres_schema: String,
    pub postgres_require_schema: bool,
}

impl RuntimeStorageConfig {
    pub fn from_env(default_sqlite_db_path: &str) -> Result<Self, String> {
        let mut envs = HashMap::new();
        for key in [
            "ORIS_RUNTIME_BACKEND",
            "ORIS_SQLITE_DB",
            "ORIS_RUNTIME_DSN",
            "ORIS_POSTGRES_DSN",
            "ORIS_POSTGRES_SCHEMA",
            "ORIS_POSTGRES_REQUIRE_SCHEMA",
        ] {
            if let Ok(value) = std::env::var(key) {
                envs.insert(key.to_string(), value);
            }
        }
        Self::from_env_map(default_sqlite_db_path, &envs)
    }

    fn from_env_map(
        default_sqlite_db_path: &str,
        envs: &HashMap<String, String>,
    ) -> Result<Self, String> {
        let backend_raw = envs
            .get("ORIS_RUNTIME_BACKEND")
            .map(|v| v.trim().to_ascii_lowercase())
            .unwrap_or_else(|| "sqlite".to_string());
        let backend = match backend_raw.as_str() {
            "sqlite" => RuntimeStorageBackend::Sqlite,
            "postgres" => RuntimeStorageBackend::Postgres,
            other => {
                return Err(format!(
                    "invalid ORIS_RUNTIME_BACKEND='{}'. expected one of: sqlite, postgres",
                    other
                ));
            }
        };

        let sqlite_db_path = envs
            .get("ORIS_SQLITE_DB")
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| default_sqlite_db_path.to_string());
        let postgres_dsn = envs
            .get("ORIS_POSTGRES_DSN")
            .or_else(|| envs.get("ORIS_RUNTIME_DSN"))
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let postgres_schema = envs
            .get("ORIS_POSTGRES_SCHEMA")
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "public".to_string());
        let postgres_require_schema = envs
            .get("ORIS_POSTGRES_REQUIRE_SCHEMA")
            .map(|v| parse_bool(v))
            .unwrap_or(true);

        if matches!(backend, RuntimeStorageBackend::Postgres) && postgres_dsn.is_none() {
            return Err(
                "ORIS_RUNTIME_BACKEND=postgres requires ORIS_POSTGRES_DSN (or ORIS_RUNTIME_DSN)"
                    .to_string(),
            );
        }

        Ok(Self {
            backend,
            sqlite_db_path,
            postgres_dsn,
            postgres_schema,
            postgres_require_schema,
        })
    }

    pub async fn startup_health_check(&self) -> Result<(), String> {
        match self.backend {
            RuntimeStorageBackend::Sqlite => {
                SqliteRuntimeRepository::new(&self.sqlite_db_path).map_err(|e| {
                    format!(
                        "runtime backend sqlite health check failed for ORIS_SQLITE_DB='{}': {}",
                        self.sqlite_db_path, e
                    )
                })?;
                Ok(())
            }
            RuntimeStorageBackend::Postgres => self.postgres_health_check().await,
        }
    }

    async fn postgres_health_check(&self) -> Result<(), String> {
        #[cfg(not(feature = "kernel-postgres"))]
        {
            Err(
                "ORIS_RUNTIME_BACKEND=postgres requires feature 'kernel-postgres'. Rebuild with --features \"kernel-postgres\"."
                    .to_string(),
            )
        }
        #[cfg(feature = "kernel-postgres")]
        {
            let dsn = self
                .postgres_dsn
                .as_deref()
                .ok_or_else(|| "postgres dsn is missing".to_string())?;
            let pool = PgPoolOptions::new()
                .max_connections(1)
                .connect(dsn)
                .await
                .map_err(|e| format!("failed to connect postgres dsn '{}': {}", dsn, e))?;

            let schema_exists = sqlx::query_scalar::<_, i64>(
                "SELECT 1 FROM pg_namespace WHERE nspname = $1 LIMIT 1",
            )
            .bind(&self.postgres_schema)
            .fetch_optional(&pool)
            .await
            .map_err(|e| {
                format!(
                    "failed to query postgres schema '{}': {}",
                    self.postgres_schema, e
                )
            })?
            .is_some();

            if self.postgres_require_schema && !schema_exists {
                return Err(format!(
                    "postgres schema '{}' does not exist (ORIS_POSTGRES_REQUIRE_SCHEMA=true). create schema first or set ORIS_POSTGRES_REQUIRE_SCHEMA=false",
                    self.postgres_schema
                ));
            }

            let repo = PostgresRuntimeRepository::new(dsn.to_string())
                .with_schema(self.postgres_schema.clone());
            repo.list_dispatchable_attempts(Utc::now(), 1)
                .map_err(|e| {
                    format!(
                        "runtime backend postgres health check failed for schema '{}': {}",
                        self.postgres_schema, e
                    )
                })?;
            Ok(())
        }
    }
}

fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;

    use super::{RuntimeStorageBackend, RuntimeStorageConfig};

    fn temp_sqlite_path() -> PathBuf {
        std::env::temp_dir().join(format!("oris-backend-config-{}.db", uuid::Uuid::new_v4()))
    }

    #[test]
    fn parse_defaults_to_sqlite_backend() {
        let envs = HashMap::new();
        let cfg =
            RuntimeStorageConfig::from_env_map("default.db", &envs).expect("parse default config");
        assert_eq!(cfg.backend, RuntimeStorageBackend::Sqlite);
        assert_eq!(cfg.sqlite_db_path, "default.db");
        assert_eq!(cfg.postgres_schema, "public");
        assert!(cfg.postgres_require_schema);
    }

    #[test]
    fn parse_invalid_backend_fails() {
        let envs = HashMap::from([("ORIS_RUNTIME_BACKEND".to_string(), "mysql".to_string())]);
        let err = RuntimeStorageConfig::from_env_map("default.db", &envs)
            .expect_err("invalid backend must fail");
        assert!(err.contains("invalid ORIS_RUNTIME_BACKEND"));
    }

    #[test]
    fn parse_postgres_without_dsn_fails() {
        let envs = HashMap::from([("ORIS_RUNTIME_BACKEND".to_string(), "postgres".to_string())]);
        let err = RuntimeStorageConfig::from_env_map("default.db", &envs)
            .expect_err("missing postgres dsn must fail");
        assert!(err.contains("ORIS_POSTGRES_DSN"));
    }

    #[test]
    fn parse_postgres_custom_schema_and_require_schema_flag() {
        let envs = HashMap::from([
            ("ORIS_RUNTIME_BACKEND".to_string(), "postgres".to_string()),
            (
                "ORIS_POSTGRES_DSN".to_string(),
                "postgres://localhost/test".to_string(),
            ),
            (
                "ORIS_POSTGRES_SCHEMA".to_string(),
                "oris_runtime".to_string(),
            ),
            (
                "ORIS_POSTGRES_REQUIRE_SCHEMA".to_string(),
                "false".to_string(),
            ),
        ]);
        let cfg =
            RuntimeStorageConfig::from_env_map("default.db", &envs).expect("parse postgres config");
        assert_eq!(cfg.backend, RuntimeStorageBackend::Postgres);
        assert_eq!(cfg.postgres_schema, "oris_runtime");
        assert!(!cfg.postgres_require_schema);
    }

    #[tokio::test]
    async fn sqlite_startup_health_check_accepts_valid_path() {
        let path = temp_sqlite_path();
        let cfg = RuntimeStorageConfig {
            backend: RuntimeStorageBackend::Sqlite,
            sqlite_db_path: path.to_string_lossy().to_string(),
            postgres_dsn: None,
            postgres_schema: "public".to_string(),
            postgres_require_schema: true,
        };
        cfg.startup_health_check()
            .await
            .expect("sqlite health check should pass");
        let _ = fs::remove_file(path);
    }
}
