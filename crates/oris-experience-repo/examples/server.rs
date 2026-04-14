//! Example: Run Experience Repository server.

use oris_experience_repo::key_service::KeyStore;
use oris_experience_repo::{ExperienceRepoServer, ServerConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Create server configuration
    let config = ServerConfig::default()
        .with_bind_addr("127.0.0.1:8080")
        .with_store_path(".oris/experience_repo.db")
        .with_key_store_path(".oris/key_store.db");

    // Create an initial API key
    {
        let key_store = KeyStore::open(".oris/key_store.db")?;
        let (raw_key, key_info) =
            key_store.create_key("admin", Some("Initial admin key".to_string()), None)?;
        println!("Created initial API key:");
        println!("  Key ID: {}", key_info.key_id);
        println!("  API Key: {}", raw_key);
        println!("  Agent ID: {}", key_info.agent_id);
        println!("\nSave this API key - it won't be shown again!");
    }

    // Create and run server
    let server = ExperienceRepoServer::new(config);
    server.serve().await?;

    Ok(())
}
