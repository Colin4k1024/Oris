use std::sync::Arc;

use crate::discovery::DiscoveryService;
use crate::federation::FederationEngine;
use crate::middleware::TokenStore;
use crate::registry::RegistryService;
use crate::subscription::SubscriptionManager;

pub struct AppState {
    pub registry: Arc<RegistryService>,
    pub discovery: DiscoveryService,
    pub federation: FederationEngine,
    pub subscriptions: SubscriptionManager,
    pub token_store: TokenStore,
}
