//! Evolution Server - Unix Domain Socket server

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use directories::ProjectDirs;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::error::Error;
use crate::handlers::RequestHandler;
use crate::pipeline::PipelineDriver;

/// Evolution server that listens on Unix Domain Socket
pub struct EvoServer {
    /// Socket path
    socket_path: PathBuf,
    /// Request handler
    handler: Arc<RequestHandler>,
    /// Shutdown signal
    shutdown: Arc<RwLock<bool>>,
}

impl EvoServer {
    /// Create and start a new evolution server
    pub async fn create_and_start(store_path: &str) -> Result<Self> {
        let socket_path = Self::default_socket_path()?;
        let server = Self::create_with_socket(socket_path, store_path).await?;
        Ok(server)
    }

    /// Create a new server with a custom socket and store path
    pub async fn create_with_socket(socket_path: PathBuf, store_path: &str) -> Result<Self> {
        // Initialize the pipeline
        let pipeline = PipelineDriver::new(store_path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create pipeline: {}", e))?;

        let handler = RequestHandler::new(pipeline);

        Ok(Self {
            socket_path,
            handler: Arc::new(handler),
            shutdown: Arc::new(RwLock::new(false)),
        })
    }

    /// Get the default socket path
    fn default_socket_path() -> Result<PathBuf> {
        let proj_dirs = ProjectDirs::from("ai", "oris", "evolution")
            .ok_or_else(|| Error::Config("Could not determine project directories".to_string()))?;

        let socket_dir = proj_dirs.data_local_dir();
        Ok(socket_dir.join("evolution.sock"))
    }

    /// Get the socket path
    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }

    /// Start the server
    pub async fn serve(&self) -> Result<()> {
        info!(socket_path = %self.socket_path.display(), "Starting evolution server");

        // Ensure parent directory exists
        if let Some(parent) = self.socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Remove existing socket file if present
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)?;
        }

        let listener = UnixListener::bind(&self.socket_path)?;
        info!(socket_path = %self.socket_path.display(), "Server listening");

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, _)) => {
                            let handler = self.handler.clone();
                            let shutdown = self.shutdown.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(stream, handler, shutdown).await {
                                    error!(error = %e, "Connection handler error");
                                }
                            });
                        }
                        Err(e) => {
                            error!(error = %e, "Failed to accept connection");
                        }
                    }
                }
                _ = self.wait_for_shutdown() => {
                    info!("Shutdown signal received");
                    break;
                }
            }
        }

        // Clean up socket file
        if self.socket_path.exists() {
            let _ = std::fs::remove_file(&self.socket_path);
        }

        Ok(())
    }

    /// Wait for shutdown signal
    async fn wait_for_shutdown(&self) {
        let shutdown = self.shutdown.clone();
        loop {
            if *shutdown.read().await {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }

    /// Request server shutdown
    pub async fn shutdown(&self) {
        let mut shutdown = self.shutdown.write().await;
        *shutdown = true;
    }
}

/// Handle a single connection
async fn handle_connection(
    stream: UnixStream,
    handler: Arc<RequestHandler>,
    _shutdown: Arc<RwLock<bool>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                // EOF - client disconnected
                break;
            }
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                // Parse JSON-RPC request
                match oris_evo_ipc_protocol::JsonRpcRequest::from_json(trimmed) {
                    Ok(request) => {
                        let response = handler.handle(request).await;
                        let response_json = response.to_json()?;

                        // Send response followed by newline
                        writer.write_all(response_json.as_bytes()).await?;
                        writer.write_all(b"\n").await?;
                        writer.flush().await?;
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to parse request");
                        let error_response = oris_evo_ipc_protocol::JsonRpcResponse::error(
                            uuid::Uuid::new_v4(),
                            oris_evo_ipc_protocol::response::ErrorCode::ParseError,
                            &format!("Parse error: {}", e),
                        );

                        let response_json = error_response.to_json()?;
                        writer.write_all(response_json.as_bytes()).await?;
                        writer.write_all(b"\n").await?;
                        writer.flush().await?;
                    }
                }
            }
            Err(e) => {
                error!(error = %e, "Read error");
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_server_creation() {
        // This would need a temporary directory for the store
        // Skipping for now as it requires actual DB setup
    }
}
