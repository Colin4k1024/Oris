use axum::{
    http::{header, HeaderName, HeaderValue, Method},
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::CorsLayer;

use super::handlers;
use super::state::AppState;
use crate::dashboard::handlers as dashboard_handlers;
use crate::middleware::auth::{verify_api_key, verify_ed25519_signature};
use crate::middleware::rate_limit::check_rate_limit;

pub fn build_router(state: Arc<AppState>) -> Router {
    let signed_routes = Router::new()
        .route("/hub/nodes", post(handlers::register_node))
        .route("/hub/nodes/{node_id}/heartbeat", put(handlers::heartbeat))
        .route("/hub/nodes/{node_id}", delete(handlers::deregister_node))
        .route("/hub/events/gene_promoted", post(handlers::gene_promoted))
        .layer(middleware::from_fn(check_rate_limit))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            verify_ed25519_signature,
        ));

    let authenticated_routes = Router::new()
        .route("/hub/nodes", get(handlers::discover_nodes))
        .route("/hub/nodes/{node_id}", get(handlers::get_node))
        .route("/hub/search", post(handlers::federated_search))
        .route("/hub/stats", get(handlers::get_stats))
        .route("/hub/subscriptions", post(handlers::create_subscription))
        .route("/hub/subscriptions", get(handlers::list_subscriptions))
        .route(
            "/hub/subscriptions/{sub_id}",
            delete(handlers::delete_subscription),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            verify_api_key,
        ));

    let dashboard_routes = Router::new()
        .route("/dashboard", get(dashboard_handlers::overview))
        .route("/dashboard/nodes", get(dashboard_handlers::nodes))
        .route(
            "/dashboard/nodes/{node_id}",
            get(dashboard_handlers::node_detail),
        )
        .route(
            "/dashboard/subscriptions",
            get(dashboard_handlers::subscriptions),
        )
        .route("/dashboard/search", get(dashboard_handlers::search))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            verify_api_key,
        ));

    // Build CORS layer
    let cors = {
        let origins_str = std::env::var("ORIS_HUB_CORS_ORIGINS").unwrap_or_default();
        let layer = CorsLayer::new()
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
            .allow_headers([
                header::CONTENT_TYPE,
                header::AUTHORIZATION,
                HeaderName::from_static("x-oen-signature"),
                HeaderName::from_static("x-oen-timestamp"),
            ])
            .max_age(Duration::from_secs(3600));

        if origins_str.is_empty() {
            layer // No Access-Control-Allow-Origin header = same-origin only
        } else {
            let origins: Vec<HeaderValue> = origins_str
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            layer.allow_origin(origins)
        }
    };

    Router::new()
        .merge(signed_routes)
        .merge(authenticated_routes)
        .merge(dashboard_routes)
        .layer(cors)
        .with_state(state)
}
