//! Experience Repository CLI - API Key management tool
//!
//! Provides commands for managing API keys in the Experience Repository.
//!
//! # Commands
//!
//! - `admin init` - Initialize first admin key via POST /keys
//! - `key create <agent_id>` - Create new API key
//! - `key list` - List all API keys via GET /keys
//! - `key revoke <key_id>` - Revoke key via DELETE /keys/:key_id
//! - `key rotate <key_id>` - Rotate key via POST /keys/:key_id/rotate

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// CLI for Experience Repository API Key management.
#[derive(Debug, Parser)]
#[command(name = "oris-exp-repo-cli")]
#[command(about = "CLI for managing Experience Repository API keys")]
struct Cli {
    /// Base URL for the Experience Repository API.
    #[arg(long, default_value = "http://localhost:8080", env = "ORIS_EXP_REPO_URL")]
    url: String,

    /// API key for authentication (uses X-Api-Key header).
    #[arg(long, env = "ORIS_EXP_REPO_API_KEY")]
    api_key: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Initialize first admin key (POST /keys).
    Admin {
        /// Agent ID for the admin key.
        #[arg(long)]
        agent_id: String,

        /// Optional TTL in days.
        #[arg(long)]
        ttl_days: Option<i64>,

        /// Optional description.
        #[arg(long)]
        description: Option<String>,
    },
    /// Key management commands.
    Key {
        #[command(subcommand)]
        action: KeyCommands,
    },
}

#[derive(Debug, Subcommand)]
enum KeyCommands {
    /// Create a new API key (POST /keys).
    Create {
        /// Agent ID this key belongs to.
        agent_id: String,

        /// Optional TTL in days.
        #[arg(long)]
        ttl_days: Option<i64>,

        /// Optional description.
        #[arg(long)]
        description: Option<String>,
    },
    /// List all API keys (GET /keys).
    List,
    /// Revoke an API key (DELETE /keys/:key_id).
    Revoke {
        /// Key ID to revoke.
        key_id: String,
    },
    /// Rotate an API key (POST /keys/:key_id/rotate).
    Rotate {
        /// Key ID to rotate.
        key_id: String,

        /// Optional TTL in days for the new key.
        #[arg(long)]
        ttl_days: Option<i64>,
    },
}

// ============================================================================
// API Request/Response Types
// ============================================================================

#[derive(Debug, Serialize)]
struct CreateKeyRequest {
    agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ttl_days: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

#[derive(Debug, Serialize)]
struct RotateKeyRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    ttl_days: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct CreateKeyResponse {
    key_id: String,
    api_key: String,
    agent_id: String,
    created_at: String,
    #[serde(rename = "expiresAt")]
    expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RotateKeyResponse {
    key_id: String,
    api_key: String,
    #[serde(rename = "rotatedAt")]
    rotated_at: String,
}

#[derive(Debug, Deserialize)]
struct ListKeysResponse {
    keys: Vec<ApiKeyInfo>,
}

#[derive(Debug, Deserialize)]
struct ApiKeyInfo {
    #[serde(rename = "keyId")]
    key_id: String,
    #[serde(rename = "agentId")]
    agent_id: String,
    status: String,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "expiresAt")]
    expires_at: Option<String>,
    #[serde(rename = "lastUsedAt")]
    last_used_at: Option<String>,
    description: Option<String>,
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = Client::builder().build()?;
    let base = cli.url.trim_end_matches('/');

    match cli.command {
        Commands::Admin {
            agent_id,
            ttl_days,
            description,
        } => {
            let request = CreateKeyRequest {
                agent_id,
                ttl_days,
                description,
            };
            let resp = client
                .post(format!("{}/keys", base))
                .json(&request)
                .send()
                .await?;
            let status = resp.status();
            let body = resp.text().await?;
            let parsed: serde_json::Value =
                serde_json::from_str(&body).unwrap_or_else(|_| serde_json::json!({ "raw": body }));

            if !status.is_success() {
                return Err(anyhow!("request failed: status={} body={}", status, parsed));
            }

            let result: CreateKeyResponse = serde_json::from_value(parsed)?;
            println!("Admin key created successfully!");
            println!("Key ID: {}", result.key_id);
            println!("API Key: {}", result.api_key);
            println!("Agent ID: {}", result.agent_id);
            println!("Created at: {}", result.created_at);
            if let Some(expires) = result.expires_at {
                println!("Expires at: {}", expires);
            }
            println!("\nIMPORTANT: Save the API key now - it will not be shown again!");
        }
        Commands::Key { action } => match action {
            KeyCommands::Create {
                agent_id,
                ttl_days,
                description,
            } => {
                let api_key = cli.api_key.ok_or_else(|| anyhow!("--api-key is required"))?;
                let request = CreateKeyRequest {
                    agent_id,
                    ttl_days,
                    description,
                };
                let resp = client
                    .post(format!("{}/keys", base))
                    .header("X-Api-Key", api_key)
                    .json(&request)
                    .send()
                    .await?;
                let status = resp.status();
                let body = resp.text().await?;
                let parsed: serde_json::Value =
                    serde_json::from_str(&body).unwrap_or_else(|_| serde_json::json!({ "raw": body }));

                if !status.is_success() {
                    return Err(anyhow!("request failed: status={} body={}", status, parsed));
                }

                let result: CreateKeyResponse = serde_json::from_value(parsed)?;
                println!("Key created successfully!");
                println!("Key ID: {}", result.key_id);
                println!("API Key: {}", result.api_key);
                println!("Agent ID: {}", result.agent_id);
                if let Some(expires) = result.expires_at {
                    println!("Expires at: {}", expires);
                }
                println!("\nIMPORTANT: Save the API key now - it will not be shown again!");
            }
            KeyCommands::List => {
                let api_key = cli.api_key.ok_or_else(|| anyhow!("--api-key is required"))?;
                let resp = client
                    .get(format!("{}/keys", base))
                    .header("X-Api-Key", api_key)
                    .send()
                    .await?;
                let status = resp.status();
                let body = resp.text().await?;
                let parsed: serde_json::Value =
                    serde_json::from_str(&body).unwrap_or_else(|_| serde_json::json!({ "raw": body }));

                if !status.is_success() {
                    return Err(anyhow!("request failed: status={} body={}", status, parsed));
                }

                let result: ListKeysResponse = serde_json::from_value(parsed)?;
                if result.keys.is_empty() {
                    println!("No API keys found.");
                } else {
                    println!("API Keys:");
                    for key in result.keys {
                        println!("  Key ID: {}", key.key_id);
                        println!("  Agent ID: {}", key.agent_id);
                        println!("  Status: {}", key.status);
                        println!("  Created: {}", key.created_at);
                        if let Some(expires) = key.expires_at {
                            println!("  Expires: {}", expires);
                        }
                        if let Some(last_used) = key.last_used_at {
                            println!("  Last used: {}", last_used);
                        }
                        if let Some(desc) = key.description {
                            println!("  Description: {}", desc);
                        }
                        println!();
                    }
                }
            }
            KeyCommands::Revoke { key_id } => {
                let api_key = cli.api_key.ok_or_else(|| anyhow!("--api-key is required"))?;
                let resp = client
                    .delete(format!("{}/keys/{}", base, key_id))
                    .header("X-Api-Key", api_key)
                    .send()
                    .await?;
                let status = resp.status();
                let body = resp.text().await?;

                if !status.is_success() {
                    let parsed: serde_json::Value =
                        serde_json::from_str(&body).unwrap_or_else(|_| serde_json::json!({ "raw": body }));
                    return Err(anyhow!("request failed: status={} body={}", status, parsed));
                }

                println!("Key {} revoked successfully.", key_id);
            }
            KeyCommands::Rotate { key_id, ttl_days } => {
                let api_key = cli.api_key.ok_or_else(|| anyhow!("--api-key is required"))?;
                let request = RotateKeyRequest { ttl_days };
                let resp = client
                    .post(format!("{}/keys/{}/rotate", base, key_id))
                    .header("X-Api-Key", api_key)
                    .json(&request)
                    .send()
                    .await?;
                let status = resp.status();
                let body = resp.text().await?;
                let parsed: serde_json::Value =
                    serde_json::from_str(&body).unwrap_or_else(|_| serde_json::json!({ "raw": body }));

                if !status.is_success() {
                    return Err(anyhow!("request failed: status={} body={}", status, parsed));
                }

                let result: RotateKeyResponse = serde_json::from_value(parsed)?;
                println!("Key rotated successfully!");
                println!("Key ID: {}", result.key_id);
                println!("New API Key: {}", result.api_key);
                println!("Rotated at: {}", result.rotated_at);
                println!("\nIMPORTANT: Save the new API key now - it will not be shown again!");
            }
        },
    }

    Ok(())
}
