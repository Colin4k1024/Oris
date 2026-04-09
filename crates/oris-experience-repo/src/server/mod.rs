//! HTTP server for Experience Repository.

mod handlers;

use std::collections::HashMap;

use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

pub use handlers::{create_routes, AppState};

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// API keys for authentication (key -> agent_id)
    pub api_keys: HashMap<String, String>,
    /// Server bind address
    pub bind_addr: String,
    /// Gene store path
    pub store_path: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            api_keys: HashMap::new(),
            bind_addr: "127.0.0.1:8080".to_string(),
            store_path: ":memory:".to_string(),
        }
    }
}

impl ServerConfig {
    /// Add an API key for authentication.
    pub fn with_api_key(mut self, key: impl Into<String>, agent_id: impl Into<String>) -> Self {
        self.api_keys.insert(key.into(), agent_id.into());
        self
    }

    /// Set the bind address.
    pub fn with_bind_addr(mut self, addr: impl Into<String>) -> Self {
        self.bind_addr = addr.into();
        self
    }

    /// Set the gene store path.
    pub fn with_store_path(mut self, path: impl Into<String>) -> Self {
        self.store_path = path.into();
        self
    }

    /// Set multiple API keys at once.
    pub fn with_api_keys(mut self, keys: HashMap<String, String>) -> Self {
        self.api_keys = keys;
        self
    }
}

/// Experience Repository HTTP Server.
#[derive(Debug, Clone)]
pub struct ExperienceRepoServer {
    config: ServerConfig,
}

impl ExperienceRepoServer {
    /// Create a new server with configuration.
    pub fn new(config: ServerConfig) -> Self {
        Self { config }
    }

    /// Start the server.
    pub async fn serve(self) -> anyhow::Result<()> {
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        let app = create_routes(self.config.clone())
            .layer(cors)
            .layer(TraceLayer::new_for_http());

        let addr = self.config.bind_addr.clone();
        let listener = TcpListener::bind(&addr).await?;
        tracing::info!("Experience Repository server listening on {}", addr);

        axum::serve(listener, app).await?;
        Ok(())
    }
}
