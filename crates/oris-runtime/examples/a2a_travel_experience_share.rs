//! A2A travel experience sharing demo (online LLM, Qwen by default).
//!
//! Flow:
//! 1. Agent A (travel-reporter) creates a Beijing->Shanghai itinerary with LLM.
//! 2. Agent A reports successful task completion to persist reusable experience.
//! 3. Agent B (travel-consumer) fetches and reuses the shared experience.
//! 4. Agent B solves the same task with injected experience hints.

use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

use oris_runtime::language_models::init_chat_model;
use reqwest::Client;
use serde_json::{json, Value};

type DynError = Box<dyn std::error::Error + Send + Sync>;

const A2A_PROTOCOL_NAME: &str = "oris.a2a";
const A2A_PROTOCOL_VERSION: &str = "1.0.0";
const TASK_CLASS_ID: &str = "travel.itinerary.cn.beijing-shanghai";
const TASK_LABEL: &str = "北京到上海行程规划";

fn mk_err(message: impl Into<String>) -> DynError {
    std::io::Error::other(message.into()).into()
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn endpoint(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn to_compact_summary(input: &str, max_chars: usize) -> String {
    let single_line = input.replace('\n', " ").trim().to_string();
    let truncated = single_line.chars().take(max_chars).collect::<String>();
    truncated.trim().to_string()
}

fn strategy_entries(asset: &Value) -> Vec<String> {
    asset["gene"]["strategy"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn strategy_value(strategy: &[String], key: &str) -> Option<String> {
    strategy.iter().find_map(|entry| {
        let (entry_key, entry_value) = entry.split_once('=')?;
        if entry_key.trim() == key {
            let normalized = entry_value.trim();
            if normalized.is_empty() {
                None
            } else {
                Some(normalized.to_string())
            }
        } else {
            None
        }
    })
}

fn is_reported_experience_for_task(asset: &Value, task_class_id: &str) -> bool {
    if asset["kind"] != "gene" || asset["gene"]["state"] != "Promoted" {
        return false;
    }
    let strategy = strategy_entries(asset);
    let has_origin = strategy
        .iter()
        .any(|entry| entry == "asset_origin=reported_experience");
    let expected_task_class_entry = format!("task_class={task_class_id}");
    let has_task_class = strategy
        .iter()
        .any(|entry| entry == &expected_task_class_entry);
    has_origin && has_task_class
}

fn reported_assets_for_task(fetch_response: &Value, task_class_id: &str) -> Vec<Value> {
    fetch_response["data"]["assets"]
        .as_array()
        .map(|assets| {
            assets
                .iter()
                .filter(|asset| is_reported_experience_for_task(asset, task_class_id))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn contains_beijing_shanghai(text: &str) -> bool {
    text.contains("北京") && text.contains("上海")
}

async fn post_json(
    client: &Client,
    base_url: &str,
    path: &str,
    payload: Value,
) -> Result<Value, DynError> {
    let response = client
        .post(endpoint(base_url, path))
        .header("content-type", "application/json")
        .json(&payload)
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(mk_err(format!(
            "request failed: {} {} -> status={} body={}",
            "POST", path, status, body
        )));
    }
    let json: Value = serde_json::from_str(&body)
        .map_err(|err| mk_err(format!("invalid JSON response for {}: {}", path, err)))?;
    Ok(json)
}

async fn handshake(
    client: &Client,
    base_url: &str,
    agent_id: &str,
    capabilities: &[&str],
) -> Result<Value, DynError> {
    let payload = json!({
        "agent_id": agent_id,
        "role": "Planner",
        "capability_level": "A4",
        "supported_protocols": [
            {
                "name": A2A_PROTOCOL_NAME,
                "version": A2A_PROTOCOL_VERSION
            }
        ],
        "advertised_capabilities": capabilities
    });
    let response = post_json(client, base_url, "/a2a/hello", payload).await?;
    if response["data"]["accepted"] != Value::Bool(true) {
        return Err(mk_err(format!(
            "handshake rejected for agent {}: {}",
            agent_id, response
        )));
    }
    Ok(response)
}

async fn fetch_assets(
    client: &Client,
    base_url: &str,
    sender_id: &str,
    signals: &[&str],
) -> Result<Value, DynError> {
    let payload = json!({
        "sender_id": sender_id,
        "protocol_version": A2A_PROTOCOL_VERSION,
        "signals": signals,
        "include_tasks": false
    });
    post_json(client, base_url, "/a2a/fetch", payload).await
}

#[tokio::main]
async fn main() -> Result<(), DynError> {
    let base_url =
        env::var("ORIS_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18081".to_string());
    let model = env::var("TRAVEL_MODEL").unwrap_or_else(|_| "qwen:qwen3-max".to_string());
    if model.starts_with("qwen:")
        && env::var("QWEN_API_KEY")
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        return Err(mk_err("QWEN_API_KEY is required for qwen models"));
    }

    let reporter_id = "travel-reporter";
    let consumer_id = "travel-consumer";
    let task_id = format!("travel-bj-sh-task-{}", now_ms());
    let dispatch_id = format!("dispatch-{task_id}");
    let capsule_id = format!("travel-capsule-{task_id}");

    let client = Client::builder().build()?;

    handshake(
        &client,
        &base_url,
        reporter_id,
        &["Coordination", "SupervisedDevloop", "ReplayFeedback"],
    )
    .await?;
    handshake(&client, &base_url, consumer_id, &["EvolutionFetch"]).await?;

    let before_fetch = fetch_assets(&client, &base_url, consumer_id, &[TASK_CLASS_ID]).await?;
    let before_reported = reported_assets_for_task(&before_fetch, TASK_CLASS_ID);
    if !before_reported.is_empty() {
        return Err(mk_err(format!(
            "expected no reported experience before sharing, found {}",
            before_reported.len()
        )));
    }

    let llm = init_chat_model(&model, Some(0.2), Some(1200), None, None, None, None, None).await?;

    let reporter_prompt = "你是资深旅行规划师。请为用户制定“北京到上海”的3天详细行程，要求包含：\
交通方式建议（高铁/飞机对比并给出推荐）、每日时间安排、住宿区域建议、预算拆分、\
注意事项与备选方案。请使用中文，输出清晰结构化内容。";
    let reporter_plan = llm.invoke(reporter_prompt).await?;
    if !contains_beijing_shanghai(&reporter_plan) {
        return Err(mk_err("reporter output does not contain 北京 and 上海"));
    }

    post_json(
        &client,
        &base_url,
        "/a2a/tasks/distribute",
        json!({
            "sender_id": reporter_id,
            "protocol_version": A2A_PROTOCOL_VERSION,
            "task_id": task_id,
            "task_summary": "基于真实推理沉淀北京到上海行程经验",
            "dispatch_id": dispatch_id,
            "summary": "travel experience distribution"
        }),
    )
    .await?;

    let claim = post_json(
        &client,
        &base_url,
        "/a2a/tasks/claim",
        json!({
            "sender_id": reporter_id,
            "protocol_version": A2A_PROTOCOL_VERSION
        }),
    )
    .await?;
    if claim["data"]["claimed"] != Value::Bool(true) {
        return Err(mk_err(format!("claim failed: {}", claim)));
    }

    let report_summary = to_compact_summary(&reporter_plan, 300);
    post_json(
        &client,
        &base_url,
        "/a2a/tasks/report",
        json!({
            "sender_id": reporter_id,
            "protocol_version": A2A_PROTOCOL_VERSION,
            "task_id": task_id,
            "status": "succeeded",
            "summary": report_summary,
            "retryable": false,
            "used_capsule": true,
            "capsule_id": capsule_id,
            "reasoning_steps_avoided": 8,
            "task_class_id": TASK_CLASS_ID,
            "task_label": TASK_LABEL
        }),
    )
    .await?;

    let after_fetch = fetch_assets(&client, &base_url, consumer_id, &[TASK_CLASS_ID]).await?;
    let after_reported = reported_assets_for_task(&after_fetch, TASK_CLASS_ID);
    if after_reported.is_empty() {
        return Err(mk_err(
            "expected at least one reported experience after sharing",
        ));
    }

    let shared_asset = after_reported
        .first()
        .ok_or_else(|| mk_err("missing shared asset"))?;
    let shared_gene_id = shared_asset["gene"]["id"]
        .as_str()
        .ok_or_else(|| mk_err("shared asset missing gene.id"))?
        .to_string();
    let shared_strategy = strategy_entries(shared_asset);
    let shared_summary = strategy_value(&shared_strategy, "summary").unwrap_or_default();
    let shared_task_label =
        strategy_value(&shared_strategy, "task_label").unwrap_or_else(|| TASK_LABEL.to_string());

    let consumer_prompt = format!(
        "你是一名旅行助手。你可复用如下共享经验：task_label={shared_task_label}; summary={shared_summary}; strategy={shared_strategy:?}。\
请在此基础上完成同类任务：为用户制定北京到上海3天行程，给出交通选择、逐日安排、预算分配和风险提示。\
请使用中文并结构化输出。"
    );
    let consumer_plan = llm.invoke(&consumer_prompt).await?;
    if consumer_plan.trim().is_empty() {
        return Err(mk_err("consumer output is empty"));
    }
    if !contains_beijing_shanghai(&consumer_plan) {
        return Err(mk_err("consumer output does not contain 北京 and 上海"));
    }

    let evidence = json!({
        "base_url": base_url,
        "model": model,
        "task_class_id": TASK_CLASS_ID,
        "task_label": TASK_LABEL,
        "before_fetch_reported_count": before_reported.len(),
        "after_fetch_reported_count": after_reported.len(),
        "shared_gene_id": shared_gene_id,
        "reporter_contains_city_markers": contains_beijing_shanghai(&reporter_plan),
        "consumer_contains_city_markers": contains_beijing_shanghai(&consumer_plan),
        "reporter_plan_preview": to_compact_summary(&reporter_plan, 180),
        "consumer_plan_preview": to_compact_summary(&consumer_plan, 180),
    });

    println!("TRAVEL_EXPERIENCE_SHARE_EVIDENCE_BEGIN");
    println!("{}", serde_json::to_string_pretty(&evidence)?);
    println!("TRAVEL_EXPERIENCE_SHARE_EVIDENCE_END");
    println!(
        "PASS: shared experience persisted and reused (before={}, after={}, gene={})",
        before_reported.len(),
        after_reported.len(),
        evidence["shared_gene_id"].as_str().unwrap_or_default()
    );

    Ok(())
}
