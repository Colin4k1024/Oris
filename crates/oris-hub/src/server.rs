use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::api::{build_router, AppState};
use crate::discovery::DiscoveryService;
use crate::error::HubError;
use crate::federation::FederationEngine;
use crate::middleware::rate_limit::create_limiter;
use crate::registry::{RegistryService, SqliteRegistryStore};
use crate::subscription::{SubscriptionManager, SubscriptionStore, WebhookDispatcher};

#[derive(Debug, Clone)]
pub struct HubConfig {
    pub bind_addr: SocketAddr,
    pub db_path: String,
    pub subscription_db_path: String,
    pub gc_interval_seconds: u64,
    pub api_keys: Vec<String>,
    pub signature_max_age_seconds: i64,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 3000)),
            db_path: ":memory:".to_string(),
            subscription_db_path: "hub-subscriptions.db".to_string(),
            gc_interval_seconds: 30,
            api_keys: vec!["dev-local-api-key".to_string()],
            signature_max_age_seconds: 300,
        }
    }
}

pub struct HubServer {
    config: HubConfig,
}

impl HubServer {
    pub fn new(config: HubConfig) -> Self {
        Self { config }
    }

    pub async fn run(self) -> Result<(), HubError> {
        let store = Arc::new(SqliteRegistryStore::new(&self.config.db_path)?);
        let registry = Arc::new(RegistryService::new(store));
        let discovery = DiscoveryService::new(Arc::clone(&registry));
        let federation = FederationEngine::new(Arc::clone(&registry));

        let sub_store = Arc::new(SubscriptionStore::new(&self.config.subscription_db_path)?);
        let dispatcher = Arc::new(WebhookDispatcher::new());
        let subscriptions = SubscriptionManager::new(sub_store, dispatcher);

        let state = Arc::new(AppState {
            registry: Arc::clone(&registry),
            discovery,
            federation,
            subscriptions,
            token_store: crate::middleware::TokenStore::with_tokens(self.config.api_keys.clone()),
            signature_max_age_seconds: self.config.signature_max_age_seconds,
        });

        let limiter = create_limiter(100);
        let app = build_router(state)
            .layer(axum::Extension(limiter))
            .layer(TraceLayer::new_for_http());

        let gc_registry = Arc::clone(&registry);
        let gc_interval = self.config.gc_interval_seconds;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(gc_interval));
            loop {
                interval.tick().await;
                let _ = gc_registry.gc().await;
            }
        });

        let listener = TcpListener::bind(self.config.bind_addr)
            .await
            .map_err(|e| HubError::Internal(e.to_string()))?;

        info!(addr = %self.config.bind_addr, "Hub server starting");
        axum::serve(listener, app)
            .await
            .map_err(|e| HubError::Internal(e.to_string()))?;

        Ok(())
    }
}
