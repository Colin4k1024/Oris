use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::{json, Value};

#[derive(Debug, Clone, ValueEnum)]
enum OutputFormat {
    Json,
    Table,
}

impl Default for OutputFormat {
    fn default() -> Self {
        Self::Json
    }
}

#[derive(Debug, Parser)]
#[command(name = "oris-operator-cli")]
#[command(about = "Operator CLI for Oris execution APIs")]
struct Cli {
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    server: String,
    /// Bearer token for API authentication (sets Authorization header).
    #[arg(long, env = "ORIS_API_TOKEN")]
    token: Option<String>,
    /// Output format: json (default) or table.
    #[arg(long, default_value = "json")]
    format: OutputFormat,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run {
        thread_id: String,
        #[arg(long)]
        input: Option<String>,
        #[arg(long)]
        idempotency_key: Option<String>,
        /// Optional dispatch priority (higher = dispatched sooner).
        #[arg(long)]
        priority: Option<i32>,
    },
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    Inspect {
        thread_id: String,
    },
    Resume {
        thread_id: String,
        #[arg(long, help = "JSON value string, e.g. '{\"approved\":true}'")]
        value: String,
        #[arg(long)]
        checkpoint_id: Option<String>,
    },
    Replay {
        thread_id: String,
        #[arg(long)]
        checkpoint_id: Option<String>,
    },
    Cancel {
        thread_id: String,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Dead-letter queue operations.
    Dlq {
        #[command(subcommand)]
        action: DlqCommand,
    },
}

#[derive(Debug, Subcommand)]
enum DlqCommand {
    /// List dead-letter queue entries.
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// Inspect a single DLQ entry.
    Inspect { attempt_id: String },
    /// Replay a dead-letter attempt.
    Replay { attempt_id: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = Client::builder().build()?;
    let base = cli.server.trim_end_matches('/');
    let token = cli.token.as_deref();
    let format = &cli.format;

    let result = match cli.command {
        Command::Run {
            thread_id,
            input,
            idempotency_key,
            priority,
        } => {
            let req = auth(
                client.post(format!("{}/v1/jobs/run", base)).json(&json!({
                    "thread_id": thread_id,
                    "input": input,
                    "idempotency_key": idempotency_key,
                    "priority": priority
                })),
                token,
            );
            send_and_decode(req).await?
        }
        Command::List {
            status,
            limit,
            offset,
        } => {
            let mut query: Vec<(&str, String)> =
                vec![("limit", limit.to_string()), ("offset", offset.to_string())];
            if let Some(status) = status {
                query.push(("status", status));
            }
            let req = auth(client.get(format!("{}/v1/jobs", base)).query(&query), token);
            send_and_decode(req).await?
        }
        Command::Inspect { thread_id } => {
            let req = auth(client.get(format!("{}/v1/jobs/{}", base, thread_id)), token);
            send_and_decode(req).await?
        }
        Command::Resume {
            thread_id,
            value,
            checkpoint_id,
        } => {
            let parsed_value: Value =
                serde_json::from_str(&value).context("`--value` must be valid JSON")?;
            let req = auth(
                client
                    .post(format!("{}/v1/jobs/{}/resume", base, thread_id))
                    .json(&json!({
                        "value": parsed_value,
                        "checkpoint_id": checkpoint_id
                    })),
                token,
            );
            send_and_decode(req).await?
        }
        Command::Replay {
            thread_id,
            checkpoint_id,
        } => {
            let req = auth(
                client
                    .post(format!("{}/v1/jobs/{}/replay", base, thread_id))
                    .json(&json!({
                        "checkpoint_id": checkpoint_id
                    })),
                token,
            );
            send_and_decode(req).await?
        }
        Command::Cancel { thread_id, reason } => {
            let req = auth(
                client
                    .post(format!("{}/v1/jobs/{}/cancel", base, thread_id))
                    .json(&json!({ "reason": reason })),
                token,
            );
            send_and_decode(req).await?
        }
        Command::Dlq { action } => match action {
            DlqCommand::List { limit, offset } => {
                let req = auth(
                    client
                        .get(format!("{}/v1/dlq", base))
                        .query(&[("limit", limit.to_string()), ("offset", offset.to_string())]),
                    token,
                );
                send_and_decode(req).await?
            }
            DlqCommand::Inspect { attempt_id } => {
                let req = auth(client.get(format!("{}/v1/dlq/{}", base, attempt_id)), token);
                send_and_decode(req).await?
            }
            DlqCommand::Replay { attempt_id } => {
                let req = auth(
                    client.post(format!("{}/v1/dlq/{}/replay", base, attempt_id)),
                    token,
                );
                send_and_decode(req).await?
            }
        },
    };

    print_output(&result, format);
    Ok(())
}

fn auth(req: RequestBuilder, token: Option<&str>) -> RequestBuilder {
    if let Some(t) = token {
        req.header("Authorization", format!("Bearer {}", t))
    } else {
        req
    }
}

fn print_output(value: &Value, format: &OutputFormat) {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(value).unwrap()),
        OutputFormat::Table => print_table(value),
    }
}

fn print_table(value: &Value) {
    match value {
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    println!();
                }
                print_table(item);
            }
        }
        Value::Object(map) => {
            let key_width = map.keys().map(|k| k.len()).max().unwrap_or(10) + 2;
            for (k, v) in map {
                let val = match v {
                    Value::String(s) => s.clone(),
                    Value::Null => "(null)".to_string(),
                    other => other.to_string(),
                };
                println!("{:<width$} {}", format!("{}:", k), val, width = key_width);
            }
        }
        Value::String(s) => println!("{}", s),
        other => println!("{}", other),
    }
}

async fn send_and_decode(req: RequestBuilder) -> Result<Value> {
    let resp = req.send().await?;
    decode_response(resp.status(), resp.text().await?)
}

fn decode_response(status: StatusCode, body: String) -> Result<Value> {
    let parsed = serde_json::from_str::<Value>(&body).unwrap_or_else(|_| json!({ "raw": body }));
    if !status.is_success() {
        return Err(anyhow!("request failed status={} body={}", status, parsed));
    }
    Ok(parsed)
}
