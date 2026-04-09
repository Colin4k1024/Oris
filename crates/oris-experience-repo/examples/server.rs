//! Example: Run Experience Repository server.

use oris_experience_repo::{ExperienceRepoServer, ServerConfig};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Create server configuration with API keys
    let mut api_keys = HashMap::new();
    api_keys.insert("test-api-key".to_string(), "agent-001".to_string());
    api_keys.insert("another-key".to_string(), "agent-002".to_string());

    let config = ServerConfig::default()
        .with_api_keys(api_keys)
        .with_bind_addr("127.0.0.1:8080")
        .with_store_path(".oris/experience_repo.db");

    // Create and run server
    let server = ExperienceRepoServer::new(config);
    server.serve().await?;

    Ok(())
}
