use oris_experience_repo::{
    control_plane::ExperienceControlPlane,
    key_service::KeyStore,
    mcp::{ExperienceMcpServer, JsonRpcRequest, McpAuth},
};
use std::sync::Arc;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::Mutex,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let database =
        std::env::var("ORIS_EXPERIENCE_DB").unwrap_or_else(|_| ".oris/experience_repo.db".into());
    let parent = std::path::Path::new(&database)
        .parent()
        .filter(|p| !p.as_os_str().is_empty());
    if let Some(parent) = parent {
        std::fs::create_dir_all(parent)?;
    }
    let key_database = std::env::var("ORIS_EXPERIENCE_KEY_DB")
        .unwrap_or_else(|_| ".oris/experience_keys.db".into());
    if let Some(parent) = std::path::Path::new(&key_database)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let server = ExperienceMcpServer::new(Arc::new(Mutex::new(ExperienceControlPlane::open(
        database,
    )?)))
    .with_key_store(Arc::new(Mutex::new(KeyStore::open(key_database)?)));
    let scopes = std::env::var("ORIS_MCP_SCOPES")
        .unwrap_or_else(|_| "experience:read,experience:write".into());
    let scopes: std::collections::HashSet<_> = scopes.split(',').map(str::trim).collect();
    let all_scopes = scopes.contains("*");
    let auth = McpAuth {
        agent_id: std::env::var("ORIS_AGENT_ID").unwrap_or_else(|_| "local-agent".into()),
        can_read: all_scopes || scopes.contains("experience:read"),
        can_write: all_scopes || scopes.contains("experience:write"),
        can_govern: all_scopes || scopes.contains("experience:govern"),
        can_admin: all_scopes || scopes.contains("experience:admin"),
    };
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(req) => server.handle(req, &auth).await,
            Err(error) => Some(oris_experience_repo::mcp::JsonRpcResponse {
                jsonrpc: "2.0",
                id: None,
                result: None,
                error: Some(oris_experience_repo::mcp::JsonRpcError {
                    code: -32700,
                    message: error.to_string(),
                    data: None,
                }),
            }),
        };
        if let Some(response) = response {
            stdout
                .write_all(serde_json::to_string(&response)?.as_bytes())
                .await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}
