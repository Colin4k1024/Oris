use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use evo_oris_repo::ExampleResult;
use oris_evokernel::{adapters::RuntimeSignalExtractorAdapter, detect_from_intake_events};
use oris_intake::{server::WebhookServer, IntakeEvent};
use tokio::sync::mpsc;
use tower::util::ServiceExt;

#[tokio::main]
async fn main() -> ExampleResult<()> {
    println!("=== Intake Webhook Demo ===\n");

    let (tx, mut rx) = mpsc::channel::<IntakeEvent>(8);
    let app = WebhookServer::new(tx).into_router();

    let payload = serde_json::json!({
        "action": "completed",
        "workflow": "ci",
        "run_id": 424242,
        "conclusion": "failure",
        "repository": {
            "full_name": "Colin4k1024/Oris",
            "html_url": "https://github.com/Colin4k1024/Oris"
        },
        "workflow_run": {
            "head_branch": "main",
            "head_sha": "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            "html_url": "https://github.com/Colin4k1024/Oris/actions/runs/424242",
            "logs_url": "https://github.com/Colin4k1024/Oris/actions/runs/424242/logs",
            "artifacts_url": "https://github.com/Colin4k1024/Oris/actions/runs/424242/artifacts"
        }
    });
    let payload_bytes = serde_json::to_vec_pretty(&payload)?;

    let request = Request::builder()
        .method(Method::POST)
        .uri("/webhooks/github")
        .header("content-type", "application/json")
        .body(Body::from(payload_bytes))?;

    let response = app.oneshot(request).await?;
    println!("webhook response: {}", response.status());
    if response.status() != StatusCode::OK {
        return Err("webhook request failed".into());
    }

    let event = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await?
        .ok_or("no IntakeEvent received from webhook server")?;

    println!("\n[1] Parsed IntakeEvent");
    println!("{}", serde_json::to_string_pretty(&event)?);

    let mut enriched_event = event.clone();
    enriched_event
        .description
        .push_str("\nerror[E0308]: mismatched types\nexpected `String`, found `u32`");

    let extractor = RuntimeSignalExtractorAdapter::default();
    let detected = detect_from_intake_events(&[enriched_event], &extractor);

    println!("\n[2] Triggered Detect-Stage Input");
    println!("signals_ready_for_pipeline={}", detected.len());
    for signal in &detected {
        println!(
            "- kind={:?}, confidence={:.2}, content={}...",
            signal.signal_type,
            signal.confidence,
            signal.description.chars().take(64).collect::<String>()
        );
    }

    println!("\n=== Intake Webhook Demo Complete ===");
    Ok(())
}
