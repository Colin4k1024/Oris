//! Evolution CLI - Gene Pool management tool
//!
//! Provides commands for managing the local Gene Pool via Unix socket IPC.
//!
//! # Commands
//!
//! - `list` - List genes in the pool
//! - `query` - Query genes by pattern
//! - `revert` - Revert a gene
//! - `ping` - Check server status

use anyhow::Result;
use clap::{Parser, Subcommand};
use oris_evo_ipc_protocol::{GeneQuery, JsonRpcRequest, RevertReason};
use std::io::{Read, Write};
use std::path::PathBuf;

/// Default socket path
fn default_socket_path() -> PathBuf {
    let proj_dirs = directories::ProjectDirs::from("ai", "oris", "evolution")
        .expect("Could not determine project directories");
    proj_dirs.data_local_dir().join("evolution.sock")
}

/// Connect to the server and send a request
fn send_request(socket_path: &PathBuf, request: JsonRpcRequest) -> Result<serde_json::Value> {
    let mut stream = std::os::unix::net::UnixStream::connect(socket_path)?;
    let request_json = serde_json::to_string(&request)?;
    writeln!(stream, "{}", request_json)?;

    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let parsed: serde_json::Value = serde_json::from_str(response.trim())?;

    // Check for error in response
    if let Some(error) = parsed.get("error") {
        anyhow::bail!("Server error: {}", error);
    }

    Ok(parsed)
}

#[derive(Parser)]
#[command(name = "evolution-cli")]
#[command(about = "Manage Oris Evolution Gene Pool", long_about = None)]
struct Cli {
    /// Socket path (defaults to ~/.local/share/oris/evolution/evolution.sock)
    #[arg(short, long)]
    socket: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List genes in the pool
    List {
        /// Maximum number of genes to return
        #[arg(short, long, default_value = "100")]
        limit: usize,
        /// Offset for pagination
        #[arg(short, long, default_value = "0")]
        offset: usize,
    },
    /// Query genes by pattern
    Query {
        /// Search pattern
        pattern: String,
        /// Maximum number of results
        #[arg(short, long)]
        limit: Option<usize>,
    },
    /// Revert a gene (remove from pool)
    Revert {
        /// Gene ID to revert
        gene_id: String,
        /// Reason for revert
        reason: String,
    },
    /// Check server status
    Ping,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let socket_path = cli.socket.unwrap_or_else(default_socket_path);

    match cli.command {
        Commands::List { limit, offset } => {
            let request = JsonRpcRequest::list(Some(limit), Some(offset));
            let response = send_request(&socket_path, request)?;
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        Commands::Query { pattern, limit } => {
            let query = GeneQuery {
                pattern,
                limit,
                min_confidence: None,
            };
            let request = JsonRpcRequest::query(query);
            let response = send_request(&socket_path, request)?;
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        Commands::Revert { gene_id, reason } => {
            let gene_uuid = uuid::Uuid::parse_str(&gene_id)
                .map_err(|_| anyhow::anyhow!("Invalid gene ID format"))?;
            let revert_reason = RevertReason {
                gene_id: gene_uuid,
                reason,
                confidence_drop: None,
            };
            let request = JsonRpcRequest::revert(revert_reason);
            let response = send_request(&socket_path, request)?;
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        Commands::Ping => {
            let request = JsonRpcRequest::ping(None);
            let response = send_request(&socket_path, request)?;
            if let Some(result) = response.get("result") {
                println!("Server is running!");
                println!("Version: {}", result["version"]);
                println!("Timestamp: {}", result["timestamp"]);
            } else {
                anyhow::bail!("Server ping failed");
            }
        }
    }

    Ok(())
}
