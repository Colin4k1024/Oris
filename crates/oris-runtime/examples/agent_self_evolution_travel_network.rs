//! Agent self-evolution travel demo with cross-agent experience sharing.
//!
//! Run:
//! `QWEN_API_KEY=... cargo run -p oris-runtime --example agent_self_evolution_travel_network --features "full-evolution-experimental"`
//!
//! What this example shows:
//! 1. Producer agent (Qwen3-Max) generates a Beijing -> Shanghai long-trip plan.
//! 2. EvoKernel captures reusable mutation experience into producer store.
//! 3. Producer publishes promoted assets via EvolutionNetworkNode.
//! 4. Consumer imports envelope, replays similar signals, then solves a similar long task.

#[cfg(feature = "full-evolution-experimental")]
use std::collections::{BTreeMap, BTreeSet};
#[cfg(feature = "full-evolution-experimental")]
use std::fs;
#[cfg(feature = "full-evolution-experimental")]
use std::path::{Path, PathBuf};
#[cfg(feature = "full-evolution-experimental")]
use std::sync::Arc;

#[cfg(feature = "full-evolution-experimental")]
use async_trait::async_trait;
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::agent::{create_agent, UnifiedAgent};
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::agent_contract::MutationProposal;
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::error::ToolError;
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::evolution::{
    CommandValidator, EvoEvolutionStore as EvolutionStore, EvoKernel,
    EvoSandboxPolicy as SandboxPolicy, EvoSelectorInput as SelectorInput, EvolutionNetworkNode,
    FetchQuery, JsonlEvolutionStore, LocalProcessSandbox, PublishRequest, ValidationPlan,
    ValidationStage,
};
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::governor::{DefaultGovernor, GovernorConfig};
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::kernel::{
    AllowAllPolicy, InMemoryEventStore, Kernel, KernelMode, KernelState, NoopActionExecutor,
    NoopStepFn, StateUpdatedOnlyReducer,
};
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::schemas::messages::Message;
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::tools::Tool;
#[cfg(feature = "full-evolution-experimental")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "full-evolution-experimental")]
use serde_json::{json, Value};

#[cfg(not(feature = "full-evolution-experimental"))]
fn main() {
    eprintln!("This example requires feature `full-evolution-experimental`.\n");
    eprintln!(
        "Run: QWEN_API_KEY=... cargo run -p oris-runtime --example agent_self_evolution_travel_network --features \"full-evolution-experimental\""
    );
}

#[cfg(feature = "full-evolution-experimental")]
type ExampleResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[cfg(feature = "full-evolution-experimental")]
const FIXED_SIGNAL_TAGS: [&str; 5] = [
    "travel_planning",
    "route:beijing-shanghai",
    "trip:longline",
    "city:beijing",
    "city:shanghai",
];

#[cfg(feature = "full-evolution-experimental")]
const NORMALIZED_SELECTOR_SIGNALS: [&str; 5] = [
    "travel planning",
    "route beijing shanghai",
    "trip longline",
    "city beijing",
    "city shanghai",
];

#[cfg(feature = "full-evolution-experimental")]
#[derive(Debug)]
struct DemoPaths {
    run_id: String,
    demo_runs_root: PathBuf,
    run_root: PathBuf,
    latest_store_root: PathBuf,
    latest_producer_store_root: PathBuf,
    latest_consumer_store_root: PathBuf,
    producer_store_root: PathBuf,
    consumer_store_root: PathBuf,
    producer_sandbox_root: PathBuf,
    consumer_sandbox_root: PathBuf,
    producer_workspace_root: PathBuf,
    consumer_workspace_root: PathBuf,
    producer_plan_path: PathBuf,
    consumer_plan_path: PathBuf,
    producer_events_summary_path: PathBuf,
    consumer_events_summary_path: PathBuf,
    store_upgrade_summary_path: PathBuf,
    solidification_summary_path: PathBuf,
    report_path: PathBuf,
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Debug, Default)]
struct EventSummary {
    total: usize,
    counts: BTreeMap<String, usize>,
    key_events: Vec<String>,
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Debug, Default)]
struct QualityCheckResult {
    checks: Vec<(String, bool)>,
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Debug, Default, Clone, Serialize)]
struct SideMigrationSummary {
    scanned_sources: usize,
    import_attempted_sources: usize,
    import_success_sources: usize,
    skipped_empty_sources: usize,
    imported_asset_ids: usize,
    failed_sources: Vec<String>,
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Debug, Default, Clone, Serialize)]
struct StoreUpgradeSummary {
    producer: SideMigrationSummary,
    consumer: SideMigrationSummary,
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Debug, Default, Clone, Serialize)]
struct GateMatrix {
    producer_quality_passed: bool,
    consumer_import_success: bool,
    replay_hit: bool,
    consumer_quality_passed: bool,
    all_passed: bool,
}

#[cfg(feature = "full-evolution-experimental")]
impl GateMatrix {
    fn new(
        producer_quality_passed: bool,
        consumer_import_success: bool,
        replay_hit: bool,
        consumer_quality_passed: bool,
    ) -> Self {
        let all_passed = producer_quality_passed
            && consumer_import_success
            && replay_hit
            && consumer_quality_passed;
        Self {
            producer_quality_passed,
            consumer_import_success,
            replay_hit,
            consumer_quality_passed,
            all_passed,
        }
    }
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Debug, Default, Clone, Serialize)]
struct SolidificationSummary {
    eligible: bool,
    skipped_reason: Option<String>,
    reported_gene_id: Option<String>,
    reported_imported_asset_ids: usize,
    producer_latest_sync_imported_asset_ids: usize,
    consumer_latest_sync_imported_asset_ids: usize,
    latest_consumer_publish_assets: usize,
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ExampleState;

#[cfg(feature = "full-evolution-experimental")]
impl KernelState for ExampleState {
    fn version(&self) -> u32 {
        1
    }
}

#[cfg(feature = "full-evolution-experimental")]
struct IntercityTransportTool;

#[cfg(feature = "full-evolution-experimental")]
#[async_trait]
impl Tool for IntercityTransportTool {
    fn name(&self) -> String {
        "intercity_transport_options".to_string()
    }

    fn description(&self) -> String {
        "Provide deterministic Beijing-Shanghai intercity transport options with duration and cost."
            .to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "origin": {"type": "string"},
                "destination": {"type": "string"}
            },
            "required": ["origin", "destination"]
        })
    }

    async fn run(&self, input: Value) -> Result<String, ToolError> {
        let origin = input
            .get("origin")
            .and_then(Value::as_str)
            .unwrap_or("北京");
        let destination = input
            .get("destination")
            .and_then(Value::as_str)
            .unwrap_or("上海");
        let payload = json!({
            "origin": origin,
            "destination": destination,
            "options": [
                {"mode": "高铁", "duration_hours": 4.8, "price_cny": 620, "note": "市区到市区，稳定可靠"},
                {"mode": "飞机", "duration_hours": 2.2, "price_cny": 950, "note": "需额外机场往返时间"},
                {"mode": "夜间卧铺列车", "duration_hours": 12.0, "price_cny": 460, "note": "节省酒店一晚费用"}
            ]
        });
        serde_json::to_string_pretty(&payload).map_err(|e| ToolError::ExecutionError(e.to_string()))
    }
}

#[cfg(feature = "full-evolution-experimental")]
struct ShanghaiHotelCatalogTool;

#[cfg(feature = "full-evolution-experimental")]
#[async_trait]
impl Tool for ShanghaiHotelCatalogTool {
    fn name(&self) -> String {
        "shanghai_hotel_catalog".to_string()
    }

    fn description(&self) -> String {
        "Return deterministic Shanghai hotel candidates grouped by budget tier.".to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "nights": {"type": "integer"},
                "budget_tier": {"type": "string", "enum": ["economy", "comfort", "premium"]}
            },
            "required": ["nights"]
        })
    }

    async fn run(&self, input: Value) -> Result<String, ToolError> {
        let nights = input.get("nights").and_then(Value::as_u64).unwrap_or(6);
        let tier = input
            .get("budget_tier")
            .and_then(Value::as_str)
            .unwrap_or("comfort");
        let hotels = match tier {
            "economy" => vec![
                json!({"name": "上海静安商务酒店", "price_per_night_cny": 360}),
                json!({"name": "上海人民广场智选酒店", "price_per_night_cny": 420}),
            ],
            "premium" => vec![
                json!({"name": "外滩景观酒店", "price_per_night_cny": 980}),
                json!({"name": "陆家嘴高层酒店", "price_per_night_cny": 1280}),
            ],
            _ => vec![
                json!({"name": "南京西路精选酒店", "price_per_night_cny": 580}),
                json!({"name": "徐汇地铁口酒店", "price_per_night_cny": 640}),
            ],
        };
        let payload = json!({
            "city": "上海",
            "nights": nights,
            "budget_tier": tier,
            "candidates": hotels
        });
        serde_json::to_string_pretty(&payload).map_err(|e| ToolError::ExecutionError(e.to_string()))
    }
}

#[cfg(feature = "full-evolution-experimental")]
struct TripBudgetTool;

#[cfg(feature = "full-evolution-experimental")]
#[async_trait]
impl Tool for TripBudgetTool {
    fn name(&self) -> String {
        "trip_budget_breakdown".to_string()
    }

    fn description(&self) -> String {
        "Compute deterministic CNY budget breakdown for long-trip planning.".to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "total_budget_cny": {"type": "number"},
                "days": {"type": "integer"}
            },
            "required": ["total_budget_cny", "days"]
        })
    }

    async fn run(&self, input: Value) -> Result<String, ToolError> {
        let total = input
            .get("total_budget_cny")
            .and_then(Value::as_f64)
            .unwrap_or(8000.0);
        let days = input
            .get("days")
            .and_then(Value::as_u64)
            .unwrap_or(7)
            .max(1);

        let transportation = (total * 0.22).round();
        let lodging = (total * 0.38).round();
        let food = (total * 0.20).round();
        let attractions = (total * 0.12).round();
        let buffer = (total - transportation - lodging - food - attractions).round();

        let payload = json!({
            "days": days,
            "total_budget_cny": total,
            "breakdown": {
                "交通": transportation,
                "住宿": lodging,
                "餐饮": food,
                "景点与活动": attractions,
                "机动预算": buffer
            },
            "avg_daily_budget_cny": (total / days as f64).round()
        });
        serde_json::to_string_pretty(&payload).map_err(|e| ToolError::ExecutionError(e.to_string()))
    }
}

#[cfg(feature = "full-evolution-experimental")]
struct RouteSkeletonTool;

#[cfg(feature = "full-evolution-experimental")]
#[async_trait]
impl Tool for RouteSkeletonTool {
    fn name(&self) -> String {
        "beijing_shanghai_route_skeleton".to_string()
    }

    fn description(&self) -> String {
        "Generate deterministic long-trip route skeleton from Beijing to Shanghai.".to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "days": {"type": "integer"}
            },
            "required": ["days"]
        })
    }

    async fn run(&self, input: Value) -> Result<String, ToolError> {
        let days = input
            .get("days")
            .and_then(Value::as_u64)
            .unwrap_or(7)
            .max(2);
        let mut itinerary = Vec::new();
        for day in 1..=days {
            let item = if day == 1 {
                "北京出发，乘高铁抵达上海，晚间外滩步行".to_string()
            } else if day == days {
                "上海收尾行程与返程准备".to_string()
            } else {
                format!("上海深度游第{}天，覆盖城市经典片区", day - 1)
            };
            itinerary.push(json!({"day": day, "summary": item}));
        }
        let payload = json!({
            "origin": "北京",
            "destination": "上海",
            "days": days,
            "daily_skeleton": itinerary
        });
        serde_json::to_string_pretty(&payload).map_err(|e| ToolError::ExecutionError(e.to_string()))
    }
}

#[cfg(feature = "full-evolution-experimental")]
fn build_local_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(IntercityTransportTool),
        Arc::new(ShanghaiHotelCatalogTool),
        Arc::new(TripBudgetTool),
        Arc::new(RouteSkeletonTool),
    ]
}

#[cfg(feature = "full-evolution-experimental")]
fn make_agent(system_prompt: &str) -> ExampleResult<UnifiedAgent> {
    let tools = build_local_tools();
    let agent = create_agent("qwen:qwen3-max", &tools, Some(system_prompt), None)?
        .with_max_iterations(20)
        .with_break_if_error(true);
    Ok(agent)
}

#[cfg(feature = "full-evolution-experimental")]
fn preview(text: &str, limit: usize) -> String {
    let mut out = String::new();
    for ch in text.chars().take(limit) {
        out.push(ch);
    }
    if text.chars().count() > limit {
        out.push_str("...");
    }
    out
}

#[cfg(feature = "full-evolution-experimental")]
fn timestamp_run_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("run-{now}")
}

#[cfg(feature = "full-evolution-experimental")]
fn resolve_demo_paths() -> ExampleResult<DemoPaths> {
    let workspace_root = std::env::current_dir()?;
    let demo_runs_root = std::env::var("ORIS_TRAVEL_DEMO_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace_root.join("docs/evokernel/demo_runs"));
    let latest_store_root = std::env::var("ORIS_TRAVEL_LATEST_STORE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace_root.join("docs/evokernel/latest-store"));
    let run_id = std::env::var("ORIS_TRAVEL_DEMO_RUN_ID").unwrap_or_else(|_| timestamp_run_id());
    let run_root = demo_runs_root.join(&run_id);
    let producer_store_root = run_root.join("producer-store");
    let consumer_store_root = run_root.join("consumer-store");
    let latest_producer_store_root = latest_store_root.join("producer");
    let latest_consumer_store_root = latest_store_root.join("consumer");
    let sandbox_base = std::env::var("ORIS_TRAVEL_DEMO_SANDBOX_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("oris-travel-demo-sandbox"));
    let producer_sandbox_root = sandbox_base.join(&run_id).join("producer");
    let consumer_sandbox_root = sandbox_base.join(&run_id).join("consumer");
    let producer_workspace_root = run_root.join("producer-workspace");
    let consumer_workspace_root = run_root.join("consumer-workspace");

    Ok(DemoPaths {
        run_id,
        demo_runs_root,
        run_root: run_root.clone(),
        latest_store_root,
        latest_producer_store_root,
        latest_consumer_store_root,
        producer_store_root,
        consumer_store_root,
        producer_sandbox_root,
        consumer_sandbox_root,
        producer_workspace_root,
        consumer_workspace_root,
        producer_plan_path: run_root.join("producer_plan.md"),
        consumer_plan_path: run_root.join("consumer_plan.md"),
        producer_events_summary_path: run_root.join("producer_events_summary.json"),
        consumer_events_summary_path: run_root.join("consumer_events_summary.json"),
        store_upgrade_summary_path: run_root.join("store_upgrade_summary.json"),
        solidification_summary_path: run_root.join("solidification_summary.json"),
        report_path: run_root.join("validation_report.md"),
    })
}

#[cfg(feature = "full-evolution-experimental")]
fn prepare_demo_dirs(paths: &DemoPaths) -> ExampleResult<()> {
    fs::create_dir_all(&paths.demo_runs_root)?;
    fs::create_dir_all(&paths.run_root)?;
    fs::create_dir_all(&paths.latest_store_root)?;
    fs::create_dir_all(&paths.latest_producer_store_root)?;
    fs::create_dir_all(&paths.latest_consumer_store_root)?;
    for dir in [
        &paths.producer_store_root,
        &paths.consumer_store_root,
        &paths.producer_sandbox_root,
        &paths.consumer_sandbox_root,
        &paths.producer_workspace_root,
        &paths.consumer_workspace_root,
    ] {
        if dir.exists() {
            let _ = fs::remove_dir_all(dir);
        }
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

#[cfg(feature = "full-evolution-experimental")]
fn log_phase(title: &str) {
    println!("\n----- {title} -----");
}

#[cfg(feature = "full-evolution-experimental")]
fn quality_gate(plan: &str, total_days: u32) -> ExampleResult<QualityCheckResult> {
    let checks = vec![
        ("包含“北京”".to_string(), plan.contains("北京")),
        ("包含“上海”".to_string(), plan.contains("上海")),
        (
            "包含“交通方案”章节".to_string(),
            plan.contains("交通方案") || plan.contains("交通"),
        ),
        (
            "包含“日程表”章节".to_string(),
            plan.contains("日程表") || plan.contains("行程"),
        ),
        (
            "包含“住宿建议”章节".to_string(),
            plan.contains("住宿建议") || plan.contains("住宿"),
        ),
        (
            "包含“预算拆分”章节".to_string(),
            plan.contains("预算拆分") || plan.contains("预算"),
        ),
        (
            "包含“风险与备选”章节".to_string(),
            plan.contains("风险") && plan.contains("备选"),
        ),
    ];
    let missing = checks
        .iter()
        .filter_map(|(name, passed)| {
            if *passed {
                None
            } else {
                Some(name.to_string())
            }
        })
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!("quality gate failed, missing sections: {:?}", missing).into());
    }

    let first_day_ok = plan.contains("第1天") || plan.to_ascii_lowercase().contains("day 1");
    let last_day_marker = format!("第{}天", total_days);
    let last_day_ok = plan.contains(&last_day_marker)
        || plan
            .to_ascii_lowercase()
            .contains(&format!("day {total_days}"));
    if !first_day_ok || !last_day_ok {
        return Err(format!(
            "quality gate failed, day markers missing: first_day_ok={first_day_ok}, last_day_ok={last_day_ok}"
        )
        .into());
    }

    let mut out = QualityCheckResult { checks };
    out.checks.push(("包含“第1天”".to_string(), first_day_ok));
    out.checks
        .push((format!("包含“第{}天”", total_days), last_day_ok));
    Ok(out)
}

#[cfg(feature = "full-evolution-experimental")]
fn summarize_event_file(path: &Path) -> ExampleResult<EventSummary> {
    let mut summary = EventSummary::default();
    if !path.exists() {
        return Ok(summary);
    }
    for (idx, line) in fs::read_to_string(path)?.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line)?;
        let kind = value
            .get("event")
            .and_then(|event| event.get("kind"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        *summary.counts.entry(kind.clone()).or_insert(0) += 1;
        summary.total += 1;

        let should_keep = matches!(
            kind.as_str(),
            "remote_asset_imported"
                | "promotion_evaluated"
                | "gene_promoted"
                | "capsule_released"
                | "capsule_reused"
                | "capsule_quarantined"
        );
        if should_keep {
            let excerpt = preview(line, 240);
            summary
                .key_events
                .push(format!("seq_line={} {}", idx + 1, excerpt));
        }
    }
    Ok(summary)
}

#[cfg(feature = "full-evolution-experimental")]
fn write_summary_json(path: &Path, summary: &EventSummary) -> ExampleResult<()> {
    let payload = json!({
        "total": summary.total,
        "counts": summary.counts,
        "key_events": summary.key_events,
    });
    fs::write(path, serde_json::to_string_pretty(&payload)?)?;
    Ok(())
}

#[cfg(feature = "full-evolution-experimental")]
fn write_store_upgrade_summary(path: &Path, summary: &StoreUpgradeSummary) -> ExampleResult<()> {
    fs::write(path, serde_json::to_string_pretty(summary)?)?;
    Ok(())
}

#[cfg(feature = "full-evolution-experimental")]
fn write_solidification_summary(path: &Path, summary: &SolidificationSummary) -> ExampleResult<()> {
    fs::write(path, serde_json::to_string_pretty(summary)?)?;
    Ok(())
}

#[cfg(feature = "full-evolution-experimental")]
fn discover_historical_store_dirs(
    demo_runs_root: &Path,
    current_run_id: &str,
    side_store_name: &str,
) -> ExampleResult<Vec<PathBuf>> {
    if !demo_runs_root.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(demo_runs_root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name == current_run_id)
            .unwrap_or(false)
        {
            continue;
        }
        let candidate = path.join(side_store_name);
        if candidate.join("events.jsonl").exists() {
            out.push(candidate);
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(feature = "full-evolution-experimental")]
fn store_sender_id(prefix: &str, store_path: &Path) -> String {
    let run = store_path
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("unknown-run");
    let side = store_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown-side");
    format!("{prefix}-{run}-{side}")
}

#[cfg(feature = "full-evolution-experimental")]
fn migrate_sources_to_latest(
    source_dirs: &[PathBuf],
    latest_node: &EvolutionNetworkNode,
    sender_prefix: &str,
) -> SideMigrationSummary {
    let mut summary = SideMigrationSummary {
        scanned_sources: source_dirs.len(),
        ..Default::default()
    };
    for source_dir in source_dirs {
        let source_store = Arc::new(JsonlEvolutionStore::new(source_dir.to_path_buf()));
        let source_node = EvolutionNetworkNode::new(source_store as Arc<dyn EvolutionStore>);
        let sender_id = store_sender_id(sender_prefix, source_dir);
        let envelope = match source_node.publish_local_assets(sender_id.clone()) {
            Ok(envelope) => envelope,
            Err(err) => {
                summary.failed_sources.push(format!(
                    "{} => publish error: {}",
                    source_dir.display(),
                    err
                ));
                continue;
            }
        };
        if envelope.assets.is_empty() {
            summary.skipped_empty_sources += 1;
            continue;
        }
        summary.import_attempted_sources += 1;
        match latest_node.accept_publish_request(&PublishRequest {
            sender_id: envelope.sender_id.clone(),
            assets: envelope.assets.clone(),
        }) {
            Ok(outcome) => {
                summary.import_success_sources += 1;
                summary.imported_asset_ids += outcome.imported_asset_ids.len();
            }
            Err(err) => {
                summary.failed_sources.push(format!(
                    "{} => import error: {}",
                    source_dir.display(),
                    err
                ));
            }
        }
    }
    summary
}

#[cfg(feature = "full-evolution-experimental")]
fn build_store_upgrade_summary(
    paths: &DemoPaths,
    latest_producer_node: &EvolutionNetworkNode,
    latest_consumer_node: &EvolutionNetworkNode,
) -> ExampleResult<StoreUpgradeSummary> {
    let producer_dirs =
        discover_historical_store_dirs(&paths.demo_runs_root, &paths.run_id, "producer-store")?;
    let consumer_dirs =
        discover_historical_store_dirs(&paths.demo_runs_root, &paths.run_id, "consumer-store")?;
    Ok(StoreUpgradeSummary {
        producer: migrate_sources_to_latest(
            &producer_dirs,
            latest_producer_node,
            "migrate-producer",
        ),
        consumer: migrate_sources_to_latest(
            &consumer_dirs,
            latest_consumer_node,
            "migrate-consumer",
        ),
    })
}

#[cfg(feature = "full-evolution-experimental")]
fn reported_gene_id(run_id: &str, capsule_id: &str) -> String {
    let normalized = capsule_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    format!("reported-travel-{run_id}-{normalized}")
}

#[cfg(feature = "full-evolution-experimental")]
fn asset_kind_counts(envelope_assets: usize, fetched_assets: usize) -> String {
    format!("publish_assets={envelope_assets}, fetched_assets={fetched_assets}")
}

#[cfg(feature = "full-evolution-experimental")]
fn write_validation_report(
    paths: &DemoPaths,
    producer_plan: &str,
    consumer_plan: &str,
    producer_quality: &QualityCheckResult,
    consumer_quality: &QualityCheckResult,
    producer_summary: &EventSummary,
    consumer_summary: &EventSummary,
    latest_producer_summary: &EventSummary,
    latest_consumer_summary: &EventSummary,
    store_upgrade_summary: &StoreUpgradeSummary,
    gate_matrix: &GateMatrix,
    solidification_summary: &SolidificationSummary,
    capture_gene_id: &str,
    capture_capsule_id: &str,
    imported_asset_ids: usize,
    used_capsule: bool,
    fallback_to_planner: bool,
    replay_reason: &str,
) -> ExampleResult<()> {
    let mut report = String::new();
    report.push_str("# 验证报告：Agent Self-Evolution Travel Network\n\n");
    report.push_str("## 运行目录\n");
    report.push_str(&format!("- run_id: `{}`\n", paths.run_id));
    report.push_str(&format!(
        "- demo_runs_root: `{}`\n",
        paths.demo_runs_root.display()
    ));
    report.push_str(&format!("- run_root: `{}`\n", paths.run_root.display()));
    report.push_str(&format!(
        "- latest_store_root: `{}`\n",
        paths.latest_store_root.display()
    ));
    report.push_str(&format!(
        "- latest_producer_store: `{}`\n",
        paths.latest_producer_store_root.display()
    ));
    report.push_str(&format!(
        "- latest_consumer_store: `{}`\n",
        paths.latest_consumer_store_root.display()
    ));
    report.push_str(&format!(
        "- producer_store: `{}`\n",
        paths.producer_store_root.display()
    ));
    report.push_str(&format!(
        "- consumer_store: `{}`\n",
        paths.consumer_store_root.display()
    ));
    report.push_str(&format!(
        "- producer_sandbox: `{}`\n",
        paths.producer_sandbox_root.display()
    ));
    report.push_str(&format!(
        "- consumer_sandbox: `{}`\n",
        paths.consumer_sandbox_root.display()
    ));
    report.push_str(&format!(
        "- producer_workspace: `{}`\n",
        paths.producer_workspace_root.display()
    ));
    report.push_str(&format!(
        "- consumer_workspace: `{}`\n",
        paths.consumer_workspace_root.display()
    ));
    report.push_str(&format!(
        "- producer_plan: `{}`\n",
        paths.producer_plan_path.display()
    ));
    report.push_str(&format!(
        "- consumer_plan: `{}`\n",
        paths.consumer_plan_path.display()
    ));
    report.push_str(&format!(
        "- producer_events_summary: `{}`\n",
        paths.producer_events_summary_path.display()
    ));
    report.push_str(&format!(
        "- consumer_events_summary: `{}`\n\n",
        paths.consumer_events_summary_path.display()
    ));
    report.push_str(&format!(
        "- store_upgrade_summary: `{}`\n",
        paths.store_upgrade_summary_path.display()
    ));
    report.push_str(&format!(
        "- solidification_summary: `{}`\n\n",
        paths.solidification_summary_path.display()
    ));

    report.push_str("## latest-store 迁移摘要\n");
    report.push_str(&format!(
        "- producer: scanned={}, import_attempted={}, import_success={}, skipped_empty={}, imported_asset_ids={}\n",
        store_upgrade_summary.producer.scanned_sources,
        store_upgrade_summary.producer.import_attempted_sources,
        store_upgrade_summary.producer.import_success_sources,
        store_upgrade_summary.producer.skipped_empty_sources,
        store_upgrade_summary.producer.imported_asset_ids
    ));
    report.push_str(&format!(
        "- consumer: scanned={}, import_attempted={}, import_success={}, skipped_empty={}, imported_asset_ids={}\n",
        store_upgrade_summary.consumer.scanned_sources,
        store_upgrade_summary.consumer.import_attempted_sources,
        store_upgrade_summary.consumer.import_success_sources,
        store_upgrade_summary.consumer.skipped_empty_sources,
        store_upgrade_summary.consumer.imported_asset_ids
    ));
    if !store_upgrade_summary.producer.failed_sources.is_empty() {
        report.push_str("- producer_failed_sources:\n");
        for failed in &store_upgrade_summary.producer.failed_sources {
            report.push_str(&format!("  - {}\n", failed));
        }
    }
    if !store_upgrade_summary.consumer.failed_sources.is_empty() {
        report.push_str("- consumer_failed_sources:\n");
        for failed in &store_upgrade_summary.consumer.failed_sources {
            report.push_str(&format!("  - {}\n", failed));
        }
    }
    report.push('\n');

    report.push_str("## 四重 Gate 判定\n");
    report.push_str(&format!(
        "- producer_quality_passed: `{}`\n",
        gate_matrix.producer_quality_passed
    ));
    report.push_str(&format!(
        "- consumer_import_success: `{}`\n",
        gate_matrix.consumer_import_success
    ));
    report.push_str(&format!("- replay_hit: `{}`\n", gate_matrix.replay_hit));
    report.push_str(&format!(
        "- consumer_quality_passed: `{}`\n",
        gate_matrix.consumer_quality_passed
    ));
    report.push_str(&format!("- all_passed: `{}`\n\n", gate_matrix.all_passed));

    report.push_str("## 固化与发布结果\n");
    report.push_str(&format!(
        "- eligible: `{}`\n",
        solidification_summary.eligible
    ));
    report.push_str(&format!(
        "- skipped_reason: `{}`\n",
        solidification_summary
            .skipped_reason
            .as_deref()
            .unwrap_or("N/A")
    ));
    report.push_str(&format!(
        "- reported_gene_id: `{}`\n",
        solidification_summary
            .reported_gene_id
            .as_deref()
            .unwrap_or("N/A")
    ));
    report.push_str(&format!(
        "- reported_imported_asset_ids: `{}`\n",
        solidification_summary.reported_imported_asset_ids
    ));
    report.push_str(&format!(
        "- producer_latest_sync_imported_asset_ids: `{}`\n",
        solidification_summary.producer_latest_sync_imported_asset_ids
    ));
    report.push_str(&format!(
        "- consumer_latest_sync_imported_asset_ids: `{}`\n",
        solidification_summary.consumer_latest_sync_imported_asset_ids
    ));
    report.push_str(&format!(
        "- latest_consumer_publish_assets: `{}`\n\n",
        solidification_summary.latest_consumer_publish_assets
    ));

    report.push_str("## 经验资产\n");
    report.push_str(&format!("- gene_id: `{}`\n", capture_gene_id));
    report.push_str(&format!("- capsule_id: `{}`\n", capture_capsule_id));
    report.push_str(&format!(
        "- imported_asset_ids: `{}`\n\n",
        imported_asset_ids
    ));

    report.push_str("## Replay 验证\n");
    report.push_str(&format!("- used_capsule: `{}`\n", used_capsule));
    report.push_str(&format!(
        "- fallback_to_planner: `{}`\n",
        fallback_to_planner
    ));
    report.push_str(&format!("- replay_reason: `{}`\n\n", replay_reason));

    report.push_str("## 质量门槛结果（Producer）\n");
    for (name, passed) in &producer_quality.checks {
        report.push_str(&format!(
            "- [{}] {}\n",
            if *passed { "x" } else { " " },
            name
        ));
    }
    report.push('\n');
    report.push_str("## 质量门槛结果（Consumer）\n");
    for (name, passed) in &consumer_quality.checks {
        report.push_str(&format!(
            "- [{}] {}\n",
            if *passed { "x" } else { " " },
            name
        ));
    }
    report.push('\n');

    report.push_str("## Producer 事件摘要\n");
    report.push_str(&format!("- total_events: {}\n", producer_summary.total));
    for (kind, count) in &producer_summary.counts {
        report.push_str(&format!("  - {}: {}\n", kind, count));
    }
    report.push('\n');

    report.push_str("## Consumer 事件摘要\n");
    report.push_str(&format!("- total_events: {}\n", consumer_summary.total));
    for (kind, count) in &consumer_summary.counts {
        report.push_str(&format!("  - {}: {}\n", kind, count));
    }
    report.push('\n');

    report.push_str("## latest Producer 事件摘要\n");
    report.push_str(&format!(
        "- total_events: {}\n",
        latest_producer_summary.total
    ));
    for (kind, count) in &latest_producer_summary.counts {
        report.push_str(&format!("  - {}: {}\n", kind, count));
    }
    report.push('\n');

    report.push_str("## latest Consumer 事件摘要\n");
    report.push_str(&format!(
        "- total_events: {}\n",
        latest_consumer_summary.total
    ));
    for (kind, count) in &latest_consumer_summary.counts {
        report.push_str(&format!("  - {}: {}\n", kind, count));
    }
    report.push('\n');

    report.push_str("## 计划预览\n");
    report.push_str("### Producer\n");
    report.push_str("```text\n");
    report.push_str(&preview(producer_plan, 1000));
    report.push_str("\n```\n\n");
    report.push_str("### Consumer\n");
    report.push_str("```text\n");
    report.push_str(&preview(consumer_plan, 1000));
    report.push_str("\n```\n");

    fs::write(&paths.report_path, report)?;
    Ok(())
}

#[cfg(feature = "full-evolution-experimental")]
fn demo_sandbox_policy() -> SandboxPolicy {
    SandboxPolicy {
        allowed_programs: vec!["git".into(), "cargo".into()],
        max_duration_ms: 180_000,
        max_output_bytes: 1_048_576,
        denied_env_prefixes: vec!["TOKEN".into(), "KEY".into(), "SECRET".into()],
    }
}

#[cfg(feature = "full-evolution-experimental")]
fn demo_validation_plan() -> ValidationPlan {
    ValidationPlan {
        profile: "travel-evolution-validation".into(),
        stages: vec![ValidationStage::Command {
            program: "cargo".into(),
            args: vec!["--version".into()],
            timeout_ms: 30_000,
        }],
    }
}

#[cfg(feature = "full-evolution-experimental")]
fn current_git_head(workspace_root: &Path) -> Option<String> {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace_root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(feature = "full-evolution-experimental")]
fn setup_demo_workspace(path: &Path) -> ExampleResult<()> {
    if path.exists() {
        let _ = fs::remove_dir_all(path);
    }
    fs::create_dir_all(path.join("docs/evolution"))?;
    fs::write(
        path.join("README.md"),
        "# Oris Travel Evolution Demo Workspace\n",
    )?;
    let _ = std::process::Command::new("git")
        .arg("init")
        .current_dir(path)
        .output();
    Ok(())
}

#[cfg(feature = "full-evolution-experimental")]
fn build_demo_evo(
    label: &str,
    workspace_root: &Path,
    sandbox_root: &Path,
    store_root: &Path,
) -> ExampleResult<(EvoKernel<ExampleState>, Arc<JsonlEvolutionStore>)> {
    setup_demo_workspace(workspace_root)?;
    let _ = fs::remove_dir_all(sandbox_root);
    let _ = fs::remove_dir_all(store_root);
    fs::create_dir_all(sandbox_root)?;
    fs::create_dir_all(store_root)?;

    let kernel = Arc::new(Kernel::<ExampleState> {
        events: Box::new(InMemoryEventStore::new()),
        snaps: None,
        reducer: Box::new(StateUpdatedOnlyReducer),
        exec: Box::new(NoopActionExecutor),
        step: Box::new(NoopStepFn),
        policy: Box::new(AllowAllPolicy),
        effect_sink: None,
        mode: KernelMode::Normal,
    });

    let store = Arc::new(JsonlEvolutionStore::new(store_root.to_path_buf()));
    let policy = demo_sandbox_policy();
    let validator = Arc::new(CommandValidator::new(policy.clone()));
    let sandbox = Arc::new(LocalProcessSandbox::new(
        format!("run-{label}"),
        workspace_root,
        sandbox_root,
    ));
    let mut governor = GovernorConfig::default();
    governor.promote_after_successes = 1;

    let evo = EvoKernel::new(
        kernel,
        sandbox,
        validator,
        store.clone() as Arc<dyn EvolutionStore>,
    )
    .with_governor(Arc::new(DefaultGovernor::new(governor)))
    .with_sandbox_policy(policy)
    .with_validation_plan(demo_validation_plan());

    Ok((evo, store))
}

#[cfg(feature = "full-evolution-experimental")]
fn experience_diff(path: &str, plan_preview: &str) -> String {
    let signal_block = FIXED_SIGNAL_TAGS.join(", ");
    format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,18 @@\n+# Longline Travel Experience: Beijing -> Shanghai\n+\n+## fixed_signals\n+- {signal_block}\n+\n+## reusable_strategy\n+- 先调用本地确定性工具收敛交通/预算/住宿边界\n+- 再用 Qwen3-Max 统一生成结构化长线计划\n+- 对相似任务先 replay 经验，再决定是否 fallback 到 planner\n+\n+## producer_plan_preview\n+{plan_preview}\n",
        path = path,
        signal_block = signal_block,
        plan_preview = plan_preview.replace('\n', "\n+"),
    )
}

#[cfg(feature = "full-evolution-experimental")]
async fn generate_long_trip_plan(
    agent: &UnifiedAgent,
    days: u32,
    budget_cny: u32,
    title: &str,
) -> ExampleResult<String> {
    let prompt = format!(
        "{title}\n请你生成从北京到上海的{days}天长线旅游规划，预算 {budget_cny} CNY。\n\
        你必须尽量调用可用工具获取交通/住宿/预算/路线信息，然后整合输出。\n\
        最终答案必须为中文，并严格包含以下章节：\n\
        1) 交通方案\n\
        2) {days}天日程表（必须至少出现“第1天”和“第{days}天”）\n\
        3) 住宿建议\n\
        4) 预算拆分（总预算 {budget_cny} CNY）\n\
        5) 风险与备选\n"
    );
    let response = agent
        .invoke_messages(vec![Message::new_human_message(&prompt)])
        .await?;
    Ok(response)
}

#[cfg(feature = "full-evolution-experimental")]
fn normalized_selector_signals() -> Vec<String> {
    NORMALIZED_SELECTOR_SIGNALS
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[cfg(feature = "full-evolution-experimental")]
fn merge_signals(base: Vec<String>, extra: &[String]) -> Vec<String> {
    let mut merged = BTreeSet::new();
    for signal in base {
        merged.insert(signal);
    }
    for signal in extra {
        merged.insert(signal.clone());
    }
    merged.into_iter().collect()
}

#[cfg(feature = "full-evolution-experimental")]
fn experience_summary_from_fetch(
    assets_count: usize,
    imported: usize,
    decision_reason: &str,
) -> String {
    format!(
        "已导入经验资产数量: {assets_count}; 本轮导入ID数: {imported}; replay判定: {decision_reason}; \
优先复用北京->上海交通与预算骨架，确保长线任务输出结构稳定。"
    )
}

#[cfg(feature = "full-evolution-experimental")]
#[tokio::main]
async fn main() -> ExampleResult<()> {
    if std::env::var("QWEN_API_KEY")
        .ok()
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
    {
        return Err("QWEN_API_KEY is required for this example".into());
    }

    println!("=== Agent Self-Evolution Travel Network Demo ===\n");
    let paths = resolve_demo_paths()?;
    prepare_demo_dirs(&paths)?;
    let latest_producer_store = Arc::new(JsonlEvolutionStore::new(
        paths.latest_producer_store_root.clone(),
    ));
    let latest_consumer_store = Arc::new(JsonlEvolutionStore::new(
        paths.latest_consumer_store_root.clone(),
    ));
    let latest_producer_node =
        EvolutionNetworkNode::new(latest_producer_store.clone() as Arc<dyn EvolutionStore>);
    let latest_consumer_node =
        EvolutionNetworkNode::new(latest_consumer_store.clone() as Arc<dyn EvolutionStore>);

    log_phase("INIT");
    println!("run_id: {}", paths.run_id);
    println!("demo_runs_root: {}", paths.demo_runs_root.display());
    println!("run_root: {}", paths.run_root.display());
    println!("latest_store_root: {}", paths.latest_store_root.display());
    println!(
        "latest_producer_store_root(local fs): {}",
        paths.latest_producer_store_root.display()
    );
    println!(
        "latest_consumer_store_root(local fs): {}",
        paths.latest_consumer_store_root.display()
    );
    println!(
        "producer_store_root(local fs): {}",
        paths.producer_store_root.display()
    );
    println!(
        "consumer_store_root(local fs): {}",
        paths.consumer_store_root.display()
    );
    println!(
        "producer_sandbox_root: {}",
        paths.producer_sandbox_root.display()
    );
    println!(
        "consumer_sandbox_root: {}",
        paths.consumer_sandbox_root.display()
    );
    println!(
        "producer_workspace_root: {}",
        paths.producer_workspace_root.display()
    );
    println!(
        "consumer_workspace_root: {}",
        paths.consumer_workspace_root.display()
    );
    println!(
        "store_upgrade_summary_path: {}",
        paths.store_upgrade_summary_path.display()
    );
    println!(
        "solidification_summary_path: {}",
        paths.solidification_summary_path.display()
    );
    println!("report_path: {}", paths.report_path.display());

    log_phase("MIGRATE - 历史经验升级到 latest-store");
    let store_upgrade_summary =
        build_store_upgrade_summary(&paths, &latest_producer_node, &latest_consumer_node)?;
    write_store_upgrade_summary(&paths.store_upgrade_summary_path, &store_upgrade_summary)?;
    println!("[MIGRATE] latest-store upgraded");
    println!(
        "    producer_scanned: {}, import_attempted: {}, import_success: {}, skipped_empty: {}, imported_asset_ids: {}",
        store_upgrade_summary.producer.scanned_sources,
        store_upgrade_summary.producer.import_attempted_sources,
        store_upgrade_summary.producer.import_success_sources,
        store_upgrade_summary.producer.skipped_empty_sources,
        store_upgrade_summary.producer.imported_asset_ids
    );
    println!(
        "    consumer_scanned: {}, import_attempted: {}, import_success: {}, skipped_empty: {}, imported_asset_ids: {}",
        store_upgrade_summary.consumer.scanned_sources,
        store_upgrade_summary.consumer.import_attempted_sources,
        store_upgrade_summary.consumer.import_success_sources,
        store_upgrade_summary.consumer.skipped_empty_sources,
        store_upgrade_summary.consumer.imported_asset_ids
    );
    if !store_upgrade_summary.producer.failed_sources.is_empty() {
        println!(
            "    producer_failed_sources: {:?}",
            store_upgrade_summary.producer.failed_sources
        );
    }
    if !store_upgrade_summary.consumer.failed_sources.is_empty() {
        println!(
            "    consumer_failed_sources: {:?}",
            store_upgrade_summary.consumer.failed_sources
        );
    }

    log_phase("PHASE 1 - PRODUCER AGENT 规划任务执行");
    let producer_prompt =
        "你是 Producer Agent。目标是完成北京到上海长线旅游任务，并沉淀可复用经验。";
    println!("producer_model: qwen:qwen3-max");
    println!("producer_prompt: {}", producer_prompt);
    println!(
        "producer_task: 北京->上海 7天, 预算 8000 CNY, 结构化输出交通/日程/住宿/预算/风险备选"
    );
    let producer_agent = make_agent(producer_prompt)?;
    let producer_plan = generate_long_trip_plan(
        &producer_agent,
        7,
        8000,
        "任务A：生成北京->上海 7天长线计划。",
    )
    .await?;
    fs::write(&paths.producer_plan_path, &producer_plan)?;
    let producer_quality = quality_gate(&producer_plan, 7)?;
    for (name, passed) in &producer_quality.checks {
        println!(
            "quality_check_producer: {} => {}",
            name,
            if *passed { "PASS" } else { "FAIL" }
        );
    }
    println!("[1] Producer plan generated");
    println!(
        "    producer_plan_saved: {}",
        paths.producer_plan_path.display()
    );
    println!(
        "    producer_plan_preview: {}\n",
        preview(&producer_plan, 220)
    );

    log_phase("PHASE 2 - 经验产生与上报到 Producer Store");
    let (producer_evo, producer_store) = build_demo_evo(
        "producer",
        &paths.producer_workspace_root,
        &paths.producer_sandbox_root,
        &paths.producer_store_root,
    )?;
    let proposal_target_path = "docs/evolution/travel-beijing-shanghai-experience.md";
    let mut proposal_files = vec![proposal_target_path.to_string()];
    proposal_files.extend(FIXED_SIGNAL_TAGS.iter().map(|s| s.to_string()));
    let proposal = MutationProposal {
        intent: "capture reusable travel planning experience for Beijing-Shanghai longline tasks"
            .to_string(),
        files: proposal_files,
        expected_effect:
            "turn validated Beijing-Shanghai longline strategy into reusable experience assets"
                .to_string(),
    };
    let producer_diff = experience_diff(proposal_target_path, &preview(&producer_plan, 500));
    let base_revision = current_git_head(&paths.producer_workspace_root);
    let capture = producer_evo
        .capture_from_proposal(
            &"travel-producer-capture".to_string(),
            &proposal,
            producer_diff,
            base_revision,
        )
        .await?;
    println!("[2] Experience captured to producer store");
    println!("    gene_id: {}", capture.gene.id);
    println!("    capsule_id: {}", capture.capsule.id);
    println!(
        "    capture_signals_sample: {:?}",
        &capture.gene.signals[..capture.gene.signals.len().min(8)]
    );
    println!("    gene_state: {:?}\n", capture.gene.state);

    log_phase("PHASE 3 - Producer 推送经验到网络封包");
    let producer_node =
        EvolutionNetworkNode::new(producer_store.clone() as Arc<dyn EvolutionStore>);
    let envelope = producer_node.publish_local_assets("agent-producer")?;
    println!("[3] Envelope published");
    println!("    published_assets: {}\n", envelope.assets.len());

    log_phase("PHASE 4 - Consumer 获取经验并导入本地 Store");
    let (consumer_evo, consumer_store) = build_demo_evo(
        "consumer",
        &paths.consumer_workspace_root,
        &paths.consumer_sandbox_root,
        &paths.consumer_store_root,
    )?;
    let import = consumer_evo.import_remote_envelope(&envelope)?;
    println!("[4] Consumer imported assets");
    println!("    accepted: {}", import.accepted);
    println!(
        "    imported_asset_ids: {}\n",
        import.imported_asset_ids.len()
    );

    log_phase("PHASE 5 - Consumer 对经验进行验证并尝试复用");
    let replay_signals = merge_signals(normalized_selector_signals(), &capture.gene.signals);
    let decision = consumer_evo
        .replay_or_fallback_for_run(
            &"travel-consumer-replay".to_string(),
            SelectorInput {
                signals: replay_signals,
                env: capture.capsule.env.clone(),
                spec_id: None,
                limit: 1,
            },
        )
        .await?;
    println!("[5] Consumer replay decision");
    println!("    used_capsule: {}", decision.used_capsule);
    println!("    fallback_to_planner: {}", decision.fallback_to_planner);
    println!("    reason: {}\n", decision.reason);

    log_phase("PHASE 6 - Consumer 在相似任务中复用经验完成规划");
    let consumer_node =
        EvolutionNetworkNode::new(consumer_store.clone() as Arc<dyn EvolutionStore>);
    let fetched = consumer_node.fetch_assets(
        "agent-consumer",
        &FetchQuery {
            sender_id: "agent-consumer".to_string(),
            signals: normalized_selector_signals(),
        },
    )?;
    let experience_summary = experience_summary_from_fetch(
        fetched.assets.len(),
        import.imported_asset_ids.len(),
        &decision.reason,
    );
    let consumer_prompt = format!(
        "你是 Consumer Agent。你已收到可复用经验：{}。你需要优先复用经验完成相似任务。",
        experience_summary
    );
    let consumer_agent = make_agent(&consumer_prompt)?;
    let consumer_plan = generate_long_trip_plan(
        &consumer_agent,
        10,
        12000,
        "任务B：生成北京->上海 10天长线计划（与任务A相似但预算与天数不同）。",
    )
    .await?;
    fs::write(&paths.consumer_plan_path, &consumer_plan)?;
    let consumer_quality = quality_gate(&consumer_plan, 10)?;
    for (name, passed) in &consumer_quality.checks {
        println!(
            "quality_check_consumer: {} => {}",
            name,
            if *passed { "PASS" } else { "FAIL" }
        );
    }
    println!("[6] Consumer similar-task plan generated");
    println!(
        "    consumer_plan_saved: {}",
        paths.consumer_plan_path.display()
    );
    println!("    preview: {}\n", preview(&consumer_plan, 220));

    log_phase("GATE - 四重门槛判定");
    let gate_matrix = GateMatrix::new(
        true,
        import.accepted && !import.imported_asset_ids.is_empty(),
        decision.used_capsule && !decision.fallback_to_planner,
        true,
    );
    println!(
        "[GATE] producer_quality_passed={}",
        gate_matrix.producer_quality_passed
    );
    println!(
        "[GATE] consumer_import_success={}",
        gate_matrix.consumer_import_success
    );
    println!("[GATE] replay_hit={}", gate_matrix.replay_hit);
    println!(
        "[GATE] consumer_quality_passed={}",
        gate_matrix.consumer_quality_passed
    );
    println!("[GATE] all_passed={}", gate_matrix.all_passed);

    log_phase("SOLIDIFY - 固化、上报、发布");
    let mut solidification_summary = SolidificationSummary::default();
    if gate_matrix.all_passed {
        solidification_summary.eligible = true;
        let producer_sync = latest_producer_node.accept_publish_request(&PublishRequest {
            sender_id: envelope.sender_id.clone(),
            assets: envelope.assets.clone(),
        })?;
        solidification_summary.producer_latest_sync_imported_asset_ids =
            producer_sync.imported_asset_ids.len();

        let consumer_run_envelope = consumer_node.publish_local_assets("agent-consumer-run")?;
        let consumer_sync = latest_consumer_node.accept_publish_request(&PublishRequest {
            sender_id: consumer_run_envelope.sender_id.clone(),
            assets: consumer_run_envelope.assets.clone(),
        })?;
        solidification_summary.consumer_latest_sync_imported_asset_ids =
            consumer_sync.imported_asset_ids.len();

        let source_capsule = decision
            .capsule_id
            .clone()
            .unwrap_or_else(|| capture.capsule.id.clone());
        let mut report_signals =
            merge_signals(normalized_selector_signals(), &capture.gene.signals);
        let extra_signal = "travel.longline.beijing-shanghai".to_string();
        if !report_signals.contains(&extra_signal) {
            report_signals.push(extra_signal);
        }
        let reported_gene_id = reported_gene_id(&paths.run_id, &source_capsule);
        let report_outcome = latest_consumer_node.record_reported_experience(
            "agent-consumer",
            reported_gene_id.clone(),
            report_signals,
            vec![
                "asset_origin=reported_experience".to_string(),
                "task_class=travel.longline.beijing-shanghai".to_string(),
                "task_label=北京到上海长线规划".to_string(),
                format!("source_capsule={source_capsule}"),
                format!("source_gene={}", capture.gene.id),
                "summary=four-gate verified and solidified after real qwen run".to_string(),
                "model=qwen:qwen3-max".to_string(),
            ],
            vec![
                "travel.demo.four-gate".to_string(),
                "travel.demo.replay-hit".to_string(),
            ],
        )?;
        let latest_published = latest_consumer_node.publish_local_assets("agent-consumer")?;

        solidification_summary.reported_gene_id = Some(reported_gene_id);
        solidification_summary.reported_imported_asset_ids =
            report_outcome.imported_asset_ids.len();
        solidification_summary.latest_consumer_publish_assets = latest_published.assets.len();
        println!("[SOLIDIFY] solidification completed");
        println!(
            "    producer_latest_sync_imported_asset_ids: {}",
            solidification_summary.producer_latest_sync_imported_asset_ids
        );
        println!(
            "    consumer_latest_sync_imported_asset_ids: {}",
            solidification_summary.consumer_latest_sync_imported_asset_ids
        );
        println!(
            "    reported_gene_id: {}",
            solidification_summary
                .reported_gene_id
                .as_deref()
                .unwrap_or("N/A")
        );
        println!(
            "    latest_consumer_publish_assets: {}",
            solidification_summary.latest_consumer_publish_assets
        );
    } else {
        solidification_summary.eligible = false;
        solidification_summary.skipped_reason =
            Some("four gates not all passed; skip solidification".to_string());
        println!(
            "[SOLIDIFY] skipped: {}",
            solidification_summary
                .skipped_reason
                .as_deref()
                .unwrap_or("unknown")
        );
    }
    write_solidification_summary(&paths.solidification_summary_path, &solidification_summary)?;

    log_phase("PHASE 7 - 指标汇总 + 事件证据 + 验证报告");
    let metrics = consumer_evo.metrics_snapshot()?;
    let producer_events_path = paths.producer_store_root.join("events.jsonl");
    let consumer_events_path = paths.consumer_store_root.join("events.jsonl");
    let latest_producer_events_path = paths.latest_producer_store_root.join("events.jsonl");
    let latest_consumer_events_path = paths.latest_consumer_store_root.join("events.jsonl");

    let producer_summary = summarize_event_file(&producer_events_path)?;
    let consumer_summary = summarize_event_file(&consumer_events_path)?;
    let latest_producer_summary = summarize_event_file(&latest_producer_events_path)?;
    let latest_consumer_summary = summarize_event_file(&latest_consumer_events_path)?;

    write_summary_json(&paths.producer_events_summary_path, &producer_summary)?;
    write_summary_json(&paths.consumer_events_summary_path, &consumer_summary)?;

    println!("producer_event_counts: {:?}", producer_summary.counts);
    println!("consumer_event_counts: {:?}", consumer_summary.counts);
    println!(
        "latest_producer_event_counts: {:?}",
        latest_producer_summary.counts
    );
    println!(
        "latest_consumer_event_counts: {:?}",
        latest_consumer_summary.counts
    );
    for (idx, event) in producer_summary.key_events.iter().take(3).enumerate() {
        println!("producer_key_event_{}: {}", idx + 1, event);
    }
    for (idx, event) in consumer_summary.key_events.iter().take(5).enumerate() {
        println!("consumer_key_event_{}: {}", idx + 1, event);
    }

    write_validation_report(
        &paths,
        &producer_plan,
        &consumer_plan,
        &producer_quality,
        &consumer_quality,
        &producer_summary,
        &consumer_summary,
        &latest_producer_summary,
        &latest_consumer_summary,
        &store_upgrade_summary,
        &gate_matrix,
        &solidification_summary,
        &capture.gene.id,
        &capture.capsule.id,
        import.imported_asset_ids.len(),
        decision.used_capsule,
        decision.fallback_to_planner,
        &decision.reason,
    )?;

    println!("[7] Metrics snapshot");
    println!(
        "    replay_attempts_total: {}",
        metrics.replay_attempts_total
    );
    println!("    replay_success_total: {}", metrics.replay_success_total);
    println!(
        "    replay_success_rate: {:.2}",
        metrics.replay_success_rate
    );
    println!("    producer_events_total: {}", producer_summary.total);
    println!("    consumer_events_total: {}", consumer_summary.total);
    println!(
        "    latest_producer_events_total: {}",
        latest_producer_summary.total
    );
    println!(
        "    latest_consumer_events_total: {}",
        latest_consumer_summary.total
    );
    println!(
        "    asset_transfer: {}",
        asset_kind_counts(envelope.assets.len(), fetched.assets.len())
    );
    println!(
        "    producer_store_root(local fs): {}",
        paths.producer_store_root.display()
    );
    println!(
        "    consumer_store_root(local fs): {}",
        paths.consumer_store_root.display()
    );
    println!(
        "    latest_producer_store_root(local fs): {}",
        paths.latest_producer_store_root.display()
    );
    println!(
        "    latest_consumer_store_root(local fs): {}",
        paths.latest_consumer_store_root.display()
    );
    println!(
        "    producer_events_summary: {}",
        paths.producer_events_summary_path.display()
    );
    println!(
        "    consumer_events_summary: {}",
        paths.consumer_events_summary_path.display()
    );
    println!(
        "    store_upgrade_summary: {}",
        paths.store_upgrade_summary_path.display()
    );
    println!(
        "    solidification_summary: {}",
        paths.solidification_summary_path.display()
    );
    println!("    validation_report: {}", paths.report_path.display());

    println!("[VERIFY] latest-store migration + four-gate solidification + publish chain complete");
    println!(
        "[VERIFY] solidified_gene_id={}",
        solidification_summary
            .reported_gene_id
            .as_deref()
            .unwrap_or("N/A")
    );

    println!(
        "\n=== Demo complete: migrate -> producer -> capture -> import -> replay -> similar task -> solidify ==="
    );
    Ok(())
}
