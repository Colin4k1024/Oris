use oris_hub::{HubConfig, HubServer};
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("oris_hub=info".parse().unwrap()),
        )
        .init();

    let bind_addr: SocketAddr = std::env::var("HUB_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".to_string())
        .parse()
        .expect("HUB_ADDR must be a valid socket address (e.g. 0.0.0.0:3000)");

    let db_path = std::env::var("HUB_DB_PATH").unwrap_or_else(|_| "hub.db".to_string());

    let config = HubConfig {
        bind_addr,
        db_path,
        ..HubConfig::default()
    };

    println!("Hub listening on {bind_addr}");
    HubServer::new(config).run().await?;
    Ok(())
}
