use oris_experience_repo::{
    control_plane::ExperienceControlPlane,
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
    let server = ExperienceMcpServer::new(Arc::new(Mutex::new(ExperienceControlPlane::open(
        database,
    )?)));
    let scopes = std::env::var("ORIS_MCP_SCOPES")
        .unwrap_or_else(|_| "experience:read,experience:write".into());
    let scopes: std::collections::HashSet<_> = scopes.split(',').map(str::trim).collect();
    let auth = McpAuth {
        agent_id: std::env::var("ORIS_AGENT_ID").unwrap_or_else(|_| "local-agent".into()),
        can_read: scopes.contains("experience:read"),
        can_write: scopes.contains("experience:write"),
        can_govern: scopes.contains("experience:govern"),
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
