use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use reqwest::{Client, Response};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::json;
use tracing::{debug, info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct WorkerPollData {
    decision: String,
    attempt_id: Option<String>,
    lease_id: Option<String>,
    lease_expires_at: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let server_url =
        std::env::var("ORIS_SERVER_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into());
    let worker_id =
        std::env::var("ORIS_WORKER_ID").unwrap_or_else(|_| "tokio-worker-example-1".into());
    let max_active_leases = std::env::var("ORIS_MAX_ACTIVE_LEASES")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(1);
    let poll_interval = std::env::var("ORIS_POLL_INTERVAL_SECONDS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(2);

    let client = Client::builder().timeout(Duration::from_secs(15)).build()?;
    info!(
        "worker start server_url={} worker_id={} max_active_leases={}",
        server_url, worker_id, max_active_leases
    );

    loop {
        let poll = worker_poll(&client, &server_url, &worker_id, max_active_leases).await?;
        match poll.decision.as_str() {
            "dispatched" => {
                let attempt_id = poll
                    .attempt_id
                    .clone()
                    .context("missing attempt_id when decision=dispatched")?;
                let lease_id = poll
                    .lease_id
                    .clone()
                    .context("missing lease_id when decision=dispatched")?;
                info!(
                    "attempt dispatched attempt_id={} lease_id={} lease_expires_at={}",
                    attempt_id,
                    lease_id,
                    poll.lease_expires_at
                        .clone()
                        .unwrap_or_else(|| "unknown".into())
                );

                worker_heartbeat(&client, &server_url, &worker_id, &lease_id).await?;
                execute_attempt(&attempt_id).await?;
                worker_ack(&client, &server_url, &worker_id, &attempt_id, "completed").await?;
                info!("attempt completed attempt_id={}", attempt_id);
            }
            "backpressure" => {
                debug!("worker backpressure; skipping this cycle");
            }
            "noop" => {
                debug!("no work available");
            }
            other => {
                warn!("unknown poll decision={}", other);
            }
        }

        tokio::time::sleep(Duration::from_secs(poll_interval)).await;
    }
}

async fn execute_attempt(attempt_id: &str) -> Result<()> {
    // Replace this with your actual business execution logic.
    info!("executing attempt {}", attempt_id);
    tokio::time::sleep(Duration::from_millis(250)).await;
    Ok(())
}

async fn worker_poll(
    client: &Client,
    server_url: &str,
    worker_id: &str,
    max_active_leases: u32,
) -> Result<WorkerPollData> {
    let url = format!("{}/v1/workers/poll", server_url.trim_end_matches('/'));
    let resp = client
        .post(url)
        .json(&json!({
            "worker_id": worker_id,
            "max_active_leases": max_active_leases
        }))
        .send()
        .await?;
    let envelope: ApiEnvelope<WorkerPollData> = decode_oris_response(resp).await?;
    Ok(envelope.data)
}

async fn worker_heartbeat(
    client: &Client,
    server_url: &str,
    worker_id: &str,
    lease_id: &str,
) -> Result<()> {
    let url = format!(
        "{}/v1/workers/{}/heartbeat",
        server_url.trim_end_matches('/'),
        worker_id
    );
    let resp = client
        .post(url)
        .json(&json!({
            "lease_id": lease_id,
            "lease_ttl_seconds": 30
        }))
        .send()
        .await?;
    let _: serde_json::Value = decode_oris_response(resp).await?;
    Ok(())
}

async fn worker_ack(
    client: &Client,
    server_url: &str,
    worker_id: &str,
    attempt_id: &str,
    terminal_status: &str,
) -> Result<()> {
    let url = format!(
        "{}/v1/workers/{}/ack",
        server_url.trim_end_matches('/'),
        worker_id
    );
    let resp = client
        .post(url)
        .json(&json!({
            "attempt_id": attempt_id,
            "terminal_status": terminal_status
        }))
        .send()
        .await?;
    let _: serde_json::Value = decode_oris_response(resp).await?;
    Ok(())
}

async fn decode_oris_response<T: DeserializeOwned>(resp: Response) -> Result<T> {
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        return Err(anyhow!("request failed status={} body={}", status, body));
    }
    let value = serde_json::from_str::<T>(&body).context("failed to parse JSON response")?;
    Ok(value)
}
