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
#[cfg(not(feature = "full-evolution-experimental"))]
use chrono::SecondsFormat;
#[cfg(feature = "full-evolution-experimental")]
use chrono::{SecondsFormat, Utc};
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
    std::eprintln!(
        "[{}] This example requires feature `full-evolution-experimental`.",
        current_ts()
    );
    std::eprintln!(
        "[{}] {}",
        current_ts(),
        "Run: QWEN_API_KEY=... cargo run -p oris-runtime --example agent_self_evolution_travel_network --features \"full-evolution-experimental\""
    );
}

#[cfg(not(feature = "full-evolution-experimental"))]
fn current_ts() -> String {
    chrono::Local::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(feature = "full-evolution-experimental")]
type ExampleResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[cfg(feature = "full-evolution-experimental")]
fn current_ts() -> String {
    chrono::Local::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(feature = "full-evolution-experimental")]
macro_rules! ts_println {
    () => {
        std::println!("[{}]", crate::current_ts());
    };
    ($($arg:tt)*) => {
        {
            let rendered = format!($($arg)*);
            if rendered.is_empty() {
                std::println!("[{}]", crate::current_ts());
            } else {
                let has_trailing_newline = rendered.ends_with('\n');
                for line in rendered.lines() {
                    std::println!("[{}] {}", crate::current_ts(), line);
                }
                if has_trailing_newline {
                    std::println!("[{}]", crate::current_ts());
                }
            }
        }
    };
}

#[cfg(feature = "full-evolution-experimental")]
#[allow(unused_macros)]
macro_rules! ts_eprintln {
    () => {
        std::eprintln!("[{}]", crate::current_ts());
    };
    ($($arg:tt)*) => {
        {
            let rendered = format!($($arg)*);
            if rendered.is_empty() {
                std::eprintln!("[{}]", crate::current_ts());
            } else {
                let has_trailing_newline = rendered.ends_with('\n');
                for line in rendered.lines() {
                    std::eprintln!("[{}] {}", crate::current_ts(), line);
                }
                if has_trailing_newline {
                    std::eprintln!("[{}]", crate::current_ts());
                }
            }
        }
    };
}

#[cfg(feature = "full-evolution-experimental")]
macro_rules! println {
    () => {
        ts_println!();
    };
    ($($arg:tt)*) => {
        ts_println!($($arg)*);
    };
}

#[cfg(feature = "full-evolution-experimental")]
#[allow(unused_macros)]
macro_rules! eprintln {
    () => {
        ts_eprintln!();
    };
    ($($arg:tt)*) => {
        ts_eprintln!($($arg)*);
    };
}

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
const PRODUCER_MODEL_ID: &str = "qwen:qwen3-max";
#[cfg(feature = "full-evolution-experimental")]
const CONSUMER_MODEL_ID: &str = PRODUCER_MODEL_ID;

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
    producer_draft_v1_llm_output_path: PathBuf,
    producer_repair_v2_llm_output_path: PathBuf,
    producer_experience_doc_path: PathBuf,
    consumer_plan_path: PathBuf,
    consumer_llm_output_path: PathBuf,
    consumer_experience_doc_path: PathBuf,
    producer_events_summary_path: PathBuf,
    consumer_events_summary_path: PathBuf,
    store_upgrade_summary_path: PathBuf,
    solidification_summary_path: PathBuf,
    report_path: PathBuf,
    experience_assets_dir: PathBuf,
    gene_asset_path: PathBuf,
    capsule_asset_path: PathBuf,
    evolution_events_asset_path: PathBuf,
    mutation_asset_path: PathBuf,
    validation_report_asset_path: PathBuf,
    memory_graph_events_asset_path: PathBuf,
    reuse_verification_asset_path: PathBuf,
    asset_manifest_path: PathBuf,
    self_repair_trace_asset_path: PathBuf,
    producer_failed_plan_path: PathBuf,
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
#[derive(Debug, Clone)]
struct LlmPlanOutput {
    raw_response: String,
    normalized_plan: String,
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
#[derive(Debug, Clone, Serialize)]
struct ValidationReportAsset {
    mutation_id: String,
    success: bool,
    profile: String,
    duration_ms: u64,
    summary: String,
    source_event_kind: String,
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Debug, Clone, Serialize)]
struct ReuseVerificationAsset {
    import_accepted: bool,
    imported_asset_count: usize,
    used_capsule: bool,
    fallback_to_planner: bool,
    replay_reason: String,
    capsule_reused_event_detected: bool,
    reused_capsule_id: Option<String>,
    matches_captured_capsule: bool,
    final_reuse_verdict: bool,
    initial_failure_detected: bool,
    repair_success: bool,
    repair_reuse_verdict: bool,
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Debug, Clone, Serialize)]
struct SelfRepairTraceAsset {
    initial_failure_detected: bool,
    failed_checks: Vec<String>,
    failure_reason: String,
    repair_applied: bool,
    repair_success: bool,
    failed_plan_path: String,
    repaired_plan_path: String,
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Debug, Clone, Serialize)]
struct AssetManifestEntry {
    path: String,
    exists: bool,
    summary: String,
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Debug, Clone, Serialize)]
struct ExperienceAssetManifest {
    run_id: String,
    assets: BTreeMap<String, AssetManifestEntry>,
    missing_assets: Vec<String>,
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum MemoryEventKindAsset {
    Signal,
    GeneSelected,
    Outcome,
    CapsuleCreated,
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Debug, Clone, Serialize)]
struct MemoryGeneRefAsset {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<String>,
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Debug, Clone, Serialize)]
struct MemoryOutcomeRefAsset {
    status: String,
    score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Debug, Clone, Serialize)]
struct MemoryGraphEventAsset {
    #[serde(rename = "type")]
    event_type: String,
    kind: MemoryEventKindAsset,
    id: String,
    ts: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    signal: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gene: Option<MemoryGeneRefAsset>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<MemoryOutcomeRefAsset>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hypothesis: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<String>,
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
fn make_producer_agent(system_prompt: &str) -> ExampleResult<UnifiedAgent> {
    let tools = build_local_tools();
    let agent = create_agent(PRODUCER_MODEL_ID, &tools, Some(system_prompt), None)?
        .with_max_iterations(20)
        .with_break_if_error(true);
    Ok(agent)
}

#[cfg(feature = "full-evolution-experimental")]
fn make_consumer_agent(system_prompt: &str) -> ExampleResult<UnifiedAgent> {
    let tools = build_local_tools();
    let agent = create_agent(CONSUMER_MODEL_ID, &tools, Some(system_prompt), None)?
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
    let experience_assets_dir = run_root.join("experience_assets");

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
        producer_workspace_root: producer_workspace_root.clone(),
        consumer_workspace_root: consumer_workspace_root.clone(),
        producer_plan_path: run_root.join("producer_plan.md"),
        producer_draft_v1_llm_output_path: run_root.join("producer_llm_output_draft_v1.md"),
        producer_repair_v2_llm_output_path: run_root.join("producer_llm_output_repair_v2.md"),
        producer_experience_doc_path: producer_workspace_root
            .join("docs/evolution/travel-beijing-shanghai-experience.md"),
        producer_failed_plan_path: run_root.join("producer_plan_failed_v1.md"),
        consumer_plan_path: run_root.join("consumer_plan.md"),
        consumer_llm_output_path: run_root.join("consumer_llm_output_task_b.md"),
        consumer_experience_doc_path: consumer_workspace_root
            .join("docs/evolution/travel-beijing-shanghai-experience.md"),
        producer_events_summary_path: run_root.join("producer_events_summary.json"),
        consumer_events_summary_path: run_root.join("consumer_events_summary.json"),
        store_upgrade_summary_path: run_root.join("store_upgrade_summary.json"),
        solidification_summary_path: run_root.join("solidification_summary.json"),
        report_path: run_root.join("validation_report.md"),
        experience_assets_dir: experience_assets_dir.clone(),
        gene_asset_path: experience_assets_dir.join("gene.json"),
        capsule_asset_path: experience_assets_dir.join("capsule.json"),
        evolution_events_asset_path: experience_assets_dir.join("evolution_events.jsonl"),
        mutation_asset_path: experience_assets_dir.join("mutation.json"),
        validation_report_asset_path: experience_assets_dir.join("validation_report.json"),
        memory_graph_events_asset_path: experience_assets_dir.join("memory_graph_events.jsonl"),
        reuse_verification_asset_path: experience_assets_dir.join("reuse_verification.json"),
        asset_manifest_path: experience_assets_dir.join("asset_manifest.json"),
        self_repair_trace_asset_path: experience_assets_dir.join("self_repair_trace.json"),
    })
}

#[cfg(feature = "full-evolution-experimental")]
fn prepare_demo_dirs(paths: &DemoPaths) -> ExampleResult<()> {
    fs::create_dir_all(&paths.demo_runs_root)?;
    fs::create_dir_all(&paths.run_root)?;
    fs::create_dir_all(&paths.latest_store_root)?;
    fs::create_dir_all(&paths.experience_assets_dir)?;
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
    println!();
    println!("----- {title} -----");
    println!("phase_start_ts: {}", current_ts());
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
fn inject_structural_failure(plan: &str, total_days: u32) -> String {
    let mut corrupted = plan.replace("风险", "注意").replace("备选", "替代");
    let last_day_marker = format!("第{}天", total_days);
    corrupted = corrupted.replace(&last_day_marker, "最后一天");
    let day_upper = format!("Day {}", total_days);
    let day_lower = format!("day {}", total_days);
    corrupted = corrupted.replace(&day_upper, "Day final");
    corrupted = corrupted.replace(&day_lower, "day final");
    corrupted.push_str(
        "\n\n[InjectedFailure]\n- removed risk/fallback semantics\n- removed last-day marker\n",
    );
    corrupted
}

#[cfg(feature = "full-evolution-experimental")]
fn parse_failed_checks(failure_reason: &str) -> Vec<String> {
    if let Some(raw) = failure_reason.split("missing sections: ").nth(1) {
        if let Ok(checks) = serde_json::from_str::<Vec<String>>(raw.trim()) {
            return checks;
        }
    }
    if failure_reason.contains("day markers missing") {
        return vec!["包含“第1天/最后一天”标记".to_string()];
    }
    vec![failure_reason.to_string()]
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
fn load_jsonl_values(path: &Path) -> ExampleResult<Vec<Value>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for line in fs::read_to_string(path)?.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        out.push(serde_json::from_str(line)?);
    }
    Ok(out)
}

#[cfg(feature = "full-evolution-experimental")]
fn write_jsonl_values(path: &Path, values: &[Value]) -> ExampleResult<()> {
    let payload = values
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?
        .join("\n");
    let payload = if payload.is_empty() {
        payload
    } else {
        format!("{payload}\n")
    };
    fs::write(path, payload)?;
    Ok(())
}

#[cfg(feature = "full-evolution-experimental")]
fn event_kind(record: &Value) -> Option<&str> {
    record
        .get("event")
        .and_then(|event| event.get("kind"))
        .and_then(Value::as_str)
}

#[cfg(feature = "full-evolution-experimental")]
fn event_mutation_id(record: &Value) -> Option<String> {
    match event_kind(record)? {
        "mutation_declared" => record
            .get("event")
            .and_then(|event| event.get("mutation"))
            .and_then(|mutation| mutation.get("intent"))
            .and_then(|intent| intent.get("id"))
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        _ => record
            .get("event")
            .and_then(|event| event.get("mutation_id"))
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
    }
}

#[cfg(feature = "full-evolution-experimental")]
fn event_gene_id(record: &Value) -> Option<String> {
    match event_kind(record)? {
        "gene_projected" => record
            .get("event")
            .and_then(|event| event.get("gene"))
            .and_then(|gene| gene.get("id"))
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        "capsule_committed" => record
            .get("event")
            .and_then(|event| event.get("capsule"))
            .and_then(|capsule| capsule.get("gene_id"))
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        "capsule_reused" => record
            .get("event")
            .and_then(|event| event.get("gene_id"))
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        _ => None,
    }
}

#[cfg(feature = "full-evolution-experimental")]
fn event_capsule_id(record: &Value) -> Option<String> {
    match event_kind(record)? {
        "capsule_committed" => record
            .get("event")
            .and_then(|event| event.get("capsule"))
            .and_then(|capsule| capsule.get("id"))
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        "capsule_reused" => record
            .get("event")
            .and_then(|event| event.get("capsule_id"))
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        _ => None,
    }
}

#[cfg(feature = "full-evolution-experimental")]
fn extract_mutation_asset(records: &[Value], mutation_id: &str) -> Option<Value> {
    records.iter().find_map(|record| {
        if event_kind(record) == Some("mutation_declared")
            && event_mutation_id(record).as_deref() == Some(mutation_id)
        {
            record
                .get("event")
                .and_then(|event| event.get("mutation"))
                .cloned()
        } else {
            None
        }
    })
}

#[cfg(feature = "full-evolution-experimental")]
fn extract_validation_report_asset(
    records: &[Value],
    mutation_id: &str,
) -> Option<ValidationReportAsset> {
    records.iter().find_map(|record| {
        let kind = event_kind(record)?;
        if !matches!(kind, "validation_passed" | "validation_failed")
            || event_mutation_id(record).as_deref() != Some(mutation_id)
        {
            return None;
        }
        let report = record.get("event")?.get("report")?;
        Some(ValidationReportAsset {
            mutation_id: mutation_id.to_string(),
            success: report
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(kind == "validation_passed"),
            profile: report
                .get("profile")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            duration_ms: report
                .get("duration_ms")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            summary: report
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or("N/A")
                .to_string(),
            source_event_kind: kind.to_string(),
        })
    })
}

#[cfg(feature = "full-evolution-experimental")]
fn build_selected_evolution_events(
    sources: &[(&str, &[Value])],
    capture_gene_id: &str,
    capture_capsule_id: &str,
    capture_mutation_id: &str,
) -> Vec<Value> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();

    for (source_name, records) in sources {
        for record in *records {
            let kind = event_kind(record).unwrap_or_default();
            let include = match kind {
                "mutation_declared" | "validation_passed" | "validation_failed" => {
                    event_mutation_id(record).as_deref() == Some(capture_mutation_id)
                }
                "gene_projected" => event_gene_id(record).as_deref() == Some(capture_gene_id),
                "capsule_committed" | "capsule_reused" => {
                    event_capsule_id(record).as_deref() == Some(capture_capsule_id)
                }
                "remote_asset_imported" => record
                    .get("event")
                    .and_then(|event| event.get("asset_ids"))
                    .and_then(Value::as_array)
                    .map(|ids| {
                        ids.iter()
                            .filter_map(Value::as_str)
                            .any(|id| id == capture_gene_id || id == capture_capsule_id)
                    })
                    .unwrap_or(false),
                _ => false,
            };
            if !include {
                continue;
            }

            let wrapped = json!({
                "source_store": source_name,
                "record": record
            });
            let dedup_key = serde_json::to_string(&wrapped).unwrap_or_default();
            if seen.insert(dedup_key) {
                out.push(wrapped);
            }
        }
    }
    out
}

#[cfg(feature = "full-evolution-experimental")]
fn detect_capsule_reused(selected_events: &[Value]) -> (bool, Option<String>) {
    for entry in selected_events {
        let record = entry.get("record").unwrap_or(entry);
        if event_kind(record) == Some("capsule_reused") {
            let capsule_id = event_capsule_id(record);
            return (true, capsule_id);
        }
    }
    (false, None)
}

#[cfg(feature = "full-evolution-experimental")]
fn build_reuse_verification(
    import_accepted: bool,
    imported_asset_count: usize,
    used_capsule: bool,
    fallback_to_planner: bool,
    replay_reason: &str,
    selected_events: &[Value],
    capture_capsule_id: &str,
    initial_failure_detected: bool,
    repair_success: bool,
) -> ReuseVerificationAsset {
    let (capsule_reused_event_detected, reused_capsule_id) = detect_capsule_reused(selected_events);
    let matches_captured_capsule = reused_capsule_id
        .as_deref()
        .map(|id| id == capture_capsule_id)
        .unwrap_or(false);
    let final_reuse_verdict = import_accepted
        && imported_asset_count > 0
        && used_capsule
        && !fallback_to_planner
        && capsule_reused_event_detected;
    let repair_reuse_verdict = initial_failure_detected
        && repair_success
        && import_accepted
        && imported_asset_count > 0
        && used_capsule
        && !fallback_to_planner
        && capsule_reused_event_detected;
    ReuseVerificationAsset {
        import_accepted,
        imported_asset_count,
        used_capsule,
        fallback_to_planner,
        replay_reason: replay_reason.to_string(),
        capsule_reused_event_detected,
        reused_capsule_id,
        matches_captured_capsule,
        final_reuse_verdict,
        initial_failure_detected,
        repair_success,
        repair_reuse_verdict,
    }
}

#[cfg(feature = "full-evolution-experimental")]
fn build_memory_graph_events(
    run_id: &str,
    replay_signals: &[String],
    capture_gene_id: &str,
    capture_capsule_id: &str,
    reuse_verification: &ReuseVerificationAsset,
) -> Vec<Value> {
    let replay_succeeded =
        reuse_verification.used_capsule && !reuse_verification.fallback_to_planner;
    let ts = Utc::now().to_rfc3339();
    let signal_id = format!("signal-{run_id}");
    let gene_id = format!("gene-selected-{run_id}");
    let outcome_id = format!("outcome-{run_id}");
    let capsule_event_id = format!("capsule-{run_id}");

    let events = vec![
        MemoryGraphEventAsset {
            event_type: "MemoryGraphEvent".to_string(),
            kind: MemoryEventKindAsset::Signal,
            id: signal_id.clone(),
            ts: ts.clone(),
            signal: Some(json!({
                "signals": replay_signals,
                "task_class": "travel.longline.beijing-shanghai"
            })),
            gene: None,
            outcome: None,
            hypothesis: None,
            parent: None,
        },
        MemoryGraphEventAsset {
            event_type: "MemoryGraphEvent".to_string(),
            kind: MemoryEventKindAsset::GeneSelected,
            id: gene_id.clone(),
            ts: ts.clone(),
            signal: None,
            gene: Some(MemoryGeneRefAsset {
                id: capture_gene_id.to_string(),
                category: Some("travel.longline.beijing-shanghai".to_string()),
            }),
            outcome: None,
            hypothesis: None,
            parent: Some(signal_id),
        },
        MemoryGraphEventAsset {
            event_type: "MemoryGraphEvent".to_string(),
            kind: MemoryEventKindAsset::Outcome,
            id: outcome_id.clone(),
            ts: ts.clone(),
            signal: None,
            gene: None,
            outcome: Some(MemoryOutcomeRefAsset {
                status: if replay_succeeded {
                    "success".to_string()
                } else {
                    "failed".to_string()
                },
                score: if replay_succeeded { 1.0 } else { 0.0 },
                note: Some(reuse_verification.replay_reason.clone()),
            }),
            hypothesis: None,
            parent: Some(gene_id),
        },
        MemoryGraphEventAsset {
            event_type: "MemoryGraphEvent".to_string(),
            kind: MemoryEventKindAsset::CapsuleCreated,
            id: capsule_event_id,
            ts,
            signal: Some(json!({
                "capsule_id": capture_capsule_id
            })),
            gene: None,
            outcome: None,
            hypothesis: None,
            parent: Some(outcome_id),
        },
    ];
    events
        .into_iter()
        .filter_map(|event| serde_json::to_value(event).ok())
        .collect()
}

#[cfg(feature = "full-evolution-experimental")]
fn push_manifest_entry(
    assets: &mut BTreeMap<String, AssetManifestEntry>,
    key: &str,
    path: &Path,
    summary: String,
) {
    assets.insert(
        key.to_string(),
        AssetManifestEntry {
            path: path.display().to_string(),
            exists: path.exists(),
            summary,
        },
    );
}

#[cfg(feature = "full-evolution-experimental")]
fn export_experience_assets(
    paths: &DemoPaths,
    capture_gene: &Value,
    capture_capsule: &Value,
    capture_gene_id: &str,
    capture_capsule_id: &str,
    capture_mutation_id: &str,
    import_accepted: bool,
    imported_asset_count: usize,
    used_capsule: bool,
    fallback_to_planner: bool,
    replay_reason: &str,
    replay_signals: &[String],
    producer_events: &[Value],
    consumer_events: &[Value],
    latest_producer_events: &[Value],
    latest_consumer_events: &[Value],
    self_repair_trace: &SelfRepairTraceAsset,
) -> ExampleResult<(ExperienceAssetManifest, ReuseVerificationAsset)> {
    fs::create_dir_all(&paths.experience_assets_dir)?;

    fs::write(
        &paths.gene_asset_path,
        serde_json::to_string_pretty(capture_gene)?,
    )?;
    fs::write(
        &paths.capsule_asset_path,
        serde_json::to_string_pretty(capture_capsule)?,
    )?;

    let mut all_records = Vec::new();
    all_records.extend_from_slice(producer_events);
    all_records.extend_from_slice(consumer_events);
    all_records.extend_from_slice(latest_producer_events);
    all_records.extend_from_slice(latest_consumer_events);

    let mutation_asset = extract_mutation_asset(&all_records, capture_mutation_id);
    let validation_asset = extract_validation_report_asset(&all_records, capture_mutation_id);

    let mut missing_assets = Vec::new();
    if let Some(mutation) = &mutation_asset {
        fs::write(
            &paths.mutation_asset_path,
            serde_json::to_string_pretty(mutation)?,
        )?;
    } else {
        missing_assets.push("mutation".to_string());
        fs::write(
            &paths.mutation_asset_path,
            serde_json::to_string_pretty(&json!({
                "missing": true,
                "mutation_id": capture_mutation_id,
                "reason": "mutation_declared not found in event stream"
            }))?,
        )?;
    }

    if let Some(validation) = &validation_asset {
        fs::write(
            &paths.validation_report_asset_path,
            serde_json::to_string_pretty(validation)?,
        )?;
    } else {
        missing_assets.push("validation_report".to_string());
        fs::write(
            &paths.validation_report_asset_path,
            serde_json::to_string_pretty(&json!({
                "missing": true,
                "mutation_id": capture_mutation_id,
                "reason": "validation_passed/failed snapshot not found"
            }))?,
        )?;
    }

    let selected_events = build_selected_evolution_events(
        &[
            ("producer", producer_events),
            ("consumer", consumer_events),
            ("latest_producer", latest_producer_events),
            ("latest_consumer", latest_consumer_events),
        ],
        capture_gene_id,
        capture_capsule_id,
        capture_mutation_id,
    );
    if selected_events.is_empty() {
        missing_assets.push("evolution_events".to_string());
    }
    write_jsonl_values(&paths.evolution_events_asset_path, &selected_events)?;

    let reuse_verification = build_reuse_verification(
        import_accepted,
        imported_asset_count,
        used_capsule,
        fallback_to_planner,
        replay_reason,
        &selected_events,
        capture_capsule_id,
        self_repair_trace.initial_failure_detected,
        self_repair_trace.repair_success,
    );
    fs::write(
        &paths.reuse_verification_asset_path,
        serde_json::to_string_pretty(&reuse_verification)?,
    )?;
    fs::write(
        &paths.self_repair_trace_asset_path,
        serde_json::to_string_pretty(self_repair_trace)?,
    )?;

    let memory_graph_events = build_memory_graph_events(
        &paths.run_id,
        replay_signals,
        capture_gene_id,
        capture_capsule_id,
        &reuse_verification,
    );
    write_jsonl_values(&paths.memory_graph_events_asset_path, &memory_graph_events)?;

    let mut assets = BTreeMap::new();
    push_manifest_entry(
        &mut assets,
        "gene",
        &paths.gene_asset_path,
        format!("gene_id={capture_gene_id}"),
    );
    push_manifest_entry(
        &mut assets,
        "capsule",
        &paths.capsule_asset_path,
        format!("capsule_id={capture_capsule_id}"),
    );
    push_manifest_entry(
        &mut assets,
        "evolution_events",
        &paths.evolution_events_asset_path,
        format!("selected_events={}", selected_events.len()),
    );
    push_manifest_entry(
        &mut assets,
        "mutation",
        &paths.mutation_asset_path,
        mutation_asset
            .as_ref()
            .map(|_| format!("mutation_id={capture_mutation_id}"))
            .unwrap_or_else(|| "missing mutation_declared".to_string()),
    );
    push_manifest_entry(
        &mut assets,
        "validation_report",
        &paths.validation_report_asset_path,
        validation_asset
            .as_ref()
            .map(|asset| format!("success={} profile={}", asset.success, asset.profile))
            .unwrap_or_else(|| "missing validation snapshot".to_string()),
    );
    push_manifest_entry(
        &mut assets,
        "memory_graph_events",
        &paths.memory_graph_events_asset_path,
        format!("events={}", memory_graph_events.len()),
    );
    push_manifest_entry(
        &mut assets,
        "reuse_verification",
        &paths.reuse_verification_asset_path,
        format!(
            "final_reuse_verdict={} repair_reuse_verdict={}",
            reuse_verification.final_reuse_verdict, reuse_verification.repair_reuse_verdict
        ),
    );
    push_manifest_entry(
        &mut assets,
        "self_repair_trace",
        &paths.self_repair_trace_asset_path,
        format!(
            "initial_failure_detected={} repair_success={}",
            self_repair_trace.initial_failure_detected, self_repair_trace.repair_success
        ),
    );

    missing_assets.sort();
    missing_assets.dedup();
    let manifest = ExperienceAssetManifest {
        run_id: paths.run_id.clone(),
        assets,
        missing_assets,
    };
    fs::write(
        &paths.asset_manifest_path,
        serde_json::to_string_pretty(&manifest)?,
    )?;
    Ok((manifest, reuse_verification))
}

#[cfg(feature = "full-evolution-experimental")]
fn print_asset_section(
    paths: &DemoPaths,
    manifest: &ExperienceAssetManifest,
    reuse_verification: &ReuseVerificationAsset,
    self_repair_trace: &SelfRepairTraceAsset,
) {
    println!("[ASSET] 六大核心资产导出");
    println!("    root: {}", paths.experience_assets_dir.display());
    for (name, entry) in &manifest.assets {
        println!(
            "    - {}: exists={} path={} summary={}",
            name, entry.exists, entry.path, entry.summary
        );
    }
    println!(
        "    - asset_manifest: {}",
        paths.asset_manifest_path.display()
    );
    println!("    - missing_assets: {:?}", manifest.missing_assets);
    println!(
        "    - final_reuse_verdict: {}",
        reuse_verification.final_reuse_verdict
    );
    println!(
        "    - repair_reuse_verdict: {}",
        reuse_verification.repair_reuse_verdict
    );
    println!("[ASSET][REPAIR] 错误与修复经验资产证据");
    println!(
        "    - self_repair_trace_path: {}",
        paths.self_repair_trace_asset_path.display()
    );
    println!(
        "    - initial_failure_detected: {}",
        self_repair_trace.initial_failure_detected
    );
    println!("    - failed_checks: {:?}", self_repair_trace.failed_checks);
    println!("    - failure_reason: {}", self_repair_trace.failure_reason);
    println!("    - repair_applied: {}", self_repair_trace.repair_applied);
    println!("    - repair_success: {}", self_repair_trace.repair_success);
    println!(
        "    - failed_plan_path: {}",
        self_repair_trace.failed_plan_path
    );
    println!(
        "    - repaired_plan_path: {}",
        self_repair_trace.repaired_plan_path
    );
    if let Some(entry) = manifest.assets.get("self_repair_trace") {
        println!(
            "    - manifest.self_repair_trace: exists={} summary={}",
            entry.exists, entry.summary
        );
    }
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
    asset_manifest: &ExperienceAssetManifest,
    reuse_verification: &ReuseVerificationAsset,
    self_repair_trace: &SelfRepairTraceAsset,
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
        "- producer_llm_output_draft_v1: `{}`\n",
        paths.producer_draft_v1_llm_output_path.display()
    ));
    report.push_str(&format!(
        "- producer_llm_output_repair_v2: `{}`\n",
        paths.producer_repair_v2_llm_output_path.display()
    ));
    report.push_str(&format!(
        "- producer_experience_doc: `{}`\n",
        paths.producer_experience_doc_path.display()
    ));
    report.push_str(&format!(
        "- consumer_experience_doc: `{}`\n",
        paths.consumer_experience_doc_path.display()
    ));
    report.push_str(&format!(
        "- consumer_plan: `{}`\n",
        paths.consumer_plan_path.display()
    ));
    report.push_str(&format!(
        "- consumer_llm_output_task_b: `{}`\n",
        paths.consumer_llm_output_path.display()
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
        "- experience_assets_root: `{}`\n",
        paths.experience_assets_dir.display()
    ));
    report.push_str(&format!(
        "- asset_manifest: `{}`\n",
        paths.asset_manifest_path.display()
    ));
    report.push_str(&format!(
        "- reuse_verification: `{}`\n\n",
        paths.reuse_verification_asset_path.display()
    ));
    report.push_str(&format!(
        "- self_repair_trace: `{}`\n\n",
        paths.self_repair_trace_asset_path.display()
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

    report.push_str("## 固化结果（Consumer 经验仅本地持久化，不上报）\n");
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
    report.push_str("- consumer_experience_reporting: `disabled(local_only_persistence)`\n\n");

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
    report.push_str(&format!("- replay_reason: `{}`\n", replay_reason));
    report.push_str(&format!(
        "- final_reuse_verdict: `{}`\n\n",
        reuse_verification.final_reuse_verdict
    ));

    report.push_str("## 错误-自修复-跨智能体复用闭环验证\n");
    report.push_str(&format!(
        "- initial_failure_detected: `{}`\n",
        self_repair_trace.initial_failure_detected
    ));
    report.push_str(&format!(
        "- failed_checks: `{:?}`\n",
        self_repair_trace.failed_checks
    ));
    report.push_str(&format!(
        "- failure_reason: `{}`\n",
        self_repair_trace.failure_reason
    ));
    report.push_str(&format!(
        "- repair_applied: `{}`\n",
        self_repair_trace.repair_applied
    ));
    report.push_str(&format!(
        "- repair_success: `{}`\n",
        self_repair_trace.repair_success
    ));
    report.push_str(&format!(
        "- failed_plan_path: `{}`\n",
        self_repair_trace.failed_plan_path
    ));
    report.push_str(&format!(
        "- repaired_plan_path: `{}`\n",
        self_repair_trace.repaired_plan_path
    ));
    report.push_str(&format!(
        "- import_accepted: `{}` imported_asset_count: `{}`\n",
        reuse_verification.import_accepted, reuse_verification.imported_asset_count
    ));
    report.push_str(&format!(
        "- used_capsule: `{}` fallback_to_planner: `{}`\n",
        reuse_verification.used_capsule, reuse_verification.fallback_to_planner
    ));
    report.push_str(&format!(
        "- capsule_reused_event_detected: `{}` reused_capsule_id: `{}`\n",
        reuse_verification.capsule_reused_event_detected,
        reuse_verification
            .reused_capsule_id
            .as_deref()
            .unwrap_or("N/A")
    ));
    report.push_str(&format!(
        "- repair_reuse_verdict: `{}`\n\n",
        reuse_verification.repair_reuse_verdict
    ));

    report.push_str("## 六大核心资产验证\n");
    for key in [
        "gene",
        "capsule",
        "evolution_events",
        "mutation",
        "validation_report",
        "memory_graph_events",
    ] {
        match asset_manifest.assets.get(key) {
            Some(entry) => {
                report.push_str(&format!(
                    "- {}: exists=`{}` path=`{}` summary=`{}`\n",
                    key, entry.exists, entry.path, entry.summary
                ));
            }
            None => {
                report.push_str(&format!(
                    "- {}: exists=`false` path=`N/A` summary=`missing manifest entry`\n",
                    key
                ));
            }
        }
    }
    if let Some(entry) = asset_manifest.assets.get("reuse_verification") {
        report.push_str(&format!(
            "- reuse_verification: exists=`{}` path=`{}` summary=`{}`\n",
            entry.exists, entry.path, entry.summary
        ));
    }
    if let Some(entry) = asset_manifest.assets.get("self_repair_trace") {
        report.push_str(&format!(
            "- self_repair_trace: exists=`{}` path=`{}` summary=`{}`\n",
            entry.exists, entry.path, entry.summary
        ));
    }
    report.push_str(&format!(
        "- asset_manifest: exists=`{}` path=`{}`\n",
        paths.asset_manifest_path.exists(),
        paths.asset_manifest_path.display()
    ));
    report.push_str(&format!(
        "- missing_assets: {:?}\n\n",
        asset_manifest.missing_assets
    ));

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

    report.push_str("## LLM 完整输出\n");
    report.push_str("### Producer (repair_v2)\n");
    report.push_str("```text\n");
    report.push_str(producer_plan);
    report.push_str("\n```\n\n");
    report.push_str("### Consumer (task_b)\n");
    report.push_str("```text\n");
    report.push_str(consumer_plan);
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
fn experience_doc_seed() -> String {
    let signal_block = FIXED_SIGNAL_TAGS.join(", ");
    [
        "# Longline Travel Experience: Beijing -> Shanghai".to_string(),
        "".to_string(),
        "## fixed_signals".to_string(),
        format!("- {signal_block}"),
        "".to_string(),
        "## reusable_strategy".to_string(),
        "- 先调用本地确定性工具收敛交通/预算/住宿边界".to_string(),
        "- 再用 Qwen3-Max 统一生成结构化长线计划".to_string(),
        "- 对相似任务先 replay 经验，再决定是否 fallback 到 planner".to_string(),
        "".to_string(),
        "## producer_llm_output_full".to_string(),
        "- seed content".to_string(),
        "".to_string(),
    ]
    .join("\n")
}

#[cfg(feature = "full-evolution-experimental")]
fn materialize_experience_doc_seed(path: &Path) -> ExampleResult<String> {
    let seed = experience_doc_seed();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, &seed)?;
    Ok(seed)
}

#[cfg(feature = "full-evolution-experimental")]
fn experience_doc_content(plan_full_output: &str) -> String {
    let signal_block = FIXED_SIGNAL_TAGS.join(", ");
    let mut out = vec![
        "# Longline Travel Experience: Beijing -> Shanghai".to_string(),
        "".to_string(),
        "## fixed_signals".to_string(),
        format!("- {signal_block}"),
        "".to_string(),
        "## reusable_strategy".to_string(),
        "- 先调用本地确定性工具收敛交通/预算/住宿边界".to_string(),
        "- 再用 Qwen3-Max 统一生成结构化长线计划".to_string(),
        "- 对相似任务先 replay 经验，再决定是否 fallback 到 planner".to_string(),
        "".to_string(),
        "## producer_llm_output_full".to_string(),
    ];
    out.extend(plan_full_output.lines().map(|line| line.to_string()));
    out.join("\n")
}

#[cfg(feature = "full-evolution-experimental")]
fn consumer_experience_doc_content(
    plan_full_output: &str,
    llm_call_status: &str,
    llm_raw_trace: &str,
    replay_reason: &str,
    used_capsule: bool,
    fallback_to_planner: bool,
) -> String {
    let signal_block = FIXED_SIGNAL_TAGS.join(", ");
    let mut out = vec![
        "# Longline Travel Experience: Beijing -> Shanghai (Consumer)".to_string(),
        "".to_string(),
        "## fixed_signals".to_string(),
        format!("- {signal_block}"),
        "".to_string(),
        "## replay_context".to_string(),
        format!("- replay_reason: {replay_reason}"),
        format!("- used_capsule: {used_capsule}"),
        format!("- fallback_to_planner: {fallback_to_planner}"),
        "- persistence_scope: local_only_not_reported".to_string(),
        "".to_string(),
        "## consumer_llm_call_status".to_string(),
        format!("- {llm_call_status}"),
        "".to_string(),
        "## consumer_llm_raw_response_trace".to_string(),
    ];
    out.extend(llm_raw_trace.lines().map(|line| line.to_string()));
    out.extend(["".to_string(), "## consumer_llm_output_full".to_string()]);
    out.extend(plan_full_output.lines().map(|line| line.to_string()));
    out.join("\n")
}

#[cfg(feature = "full-evolution-experimental")]
fn experience_diff(path: &str, old_content: &str, new_content: &str) -> String {
    let old_lines = old_content.lines().collect::<Vec<_>>();
    let new_lines = new_content.lines().collect::<Vec<_>>();
    let old_count = old_lines.len();
    let new_count = new_lines.len();
    let old_block = old_lines
        .iter()
        .map(|line| format!("-{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let new_block = new_lines
        .iter()
        .map(|line| format!("+{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "diff --git a/{path} b/{path}\nindex 1111111..2222222 100644\n--- a/{path}\n+++ b/{path}\n@@ -1,{old_count} +1,{new_count} @@\n{old_block}\n{new_block}\n",
        path = path,
        old_count = old_count.max(1),
        new_count = new_count.max(1),
        old_block = old_block,
        new_block = new_block,
    )
}

#[cfg(feature = "full-evolution-experimental")]
fn long_trip_task_prompt(title: &str, days: u32, budget_cny: u32) -> String {
    format!(
        r#"{}
请你生成从北京到上海的{}天长线旅游规划，预算 {} CNY。
你必须尽量调用可用工具获取交通/住宿/预算/路线信息，然后整合输出。
最终答案必须为中文，并严格包含以下章节：
1) 交通方案
2) {}天日程表（必须至少出现"第1天"和"第{}天"）
3) 住宿建议
4) 预算拆分（总预算 {} CNY）
5) 风险与备选
"#,
        title, days, budget_cny, days, days, budget_cny
    )
}

#[cfg(feature = "full-evolution-experimental")]
fn unwrap_markdown_code_fence(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if !trimmed.starts_with("```") {
        return None;
    }
    let lines = trimmed.lines().collect::<Vec<_>>();
    if lines.len() < 3 {
        return None;
    }
    if lines.last().map(|line| line.trim()) != Some("```") {
        return None;
    }
    Some(lines[1..lines.len() - 1].join("\n"))
}

#[cfg(feature = "full-evolution-experimental")]
fn extract_action_input(raw: &str) -> Option<String> {
    fn extract_action_input_loose(candidate: &str) -> Option<String> {
        let key = "\"action_input\"";
        let key_pos = candidate.find(key)?;
        let after_key = &candidate[key_pos + key.len()..];
        let colon_pos = after_key.find(':')?;
        let mut rest = after_key[colon_pos + 1..].trim_start();
        if !rest.starts_with('"') {
            return None;
        }
        rest = &rest[1..];
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(ch) = chars.next() {
            match ch {
                '\\' => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('u') => {
                        let mut hex = String::new();
                        for _ in 0..4 {
                            if let Some(h) = chars.next() {
                                hex.push(h);
                            } else {
                                break;
                            }
                        }
                        if hex.len() == 4 {
                            if let Ok(code) = u16::from_str_radix(&hex, 16) {
                                if let Some(unicode) = char::from_u32(code as u32) {
                                    out.push(unicode);
                                }
                            }
                        }
                    }
                    Some(other) => out.push(other),
                    None => break,
                },
                '"' => return Some(out),
                _ => out.push(ch),
            }
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    let mut candidates = vec![raw.trim().to_string()];
    if let Some(unwrapped) = unwrap_markdown_code_fence(raw) {
        if unwrapped.trim() != raw.trim() {
            candidates.push(unwrapped);
        }
    }
    for candidate in candidates {
        if let Ok(value) = serde_json::from_str::<Value>(&candidate) {
            if let Some(action_input) = value.get("action_input").and_then(Value::as_str) {
                return Some(action_input.to_string());
            }
            if let Some(content) = value.get("content").and_then(Value::as_str) {
                return Some(content.to_string());
            }
        }
        if let Some(action_input) = extract_action_input_loose(&candidate) {
            return Some(action_input);
        }
    }
    None
}

#[cfg(feature = "full-evolution-experimental")]
fn normalize_numbered_sections_markdown(text: &str) -> String {
    fn parse_heading(line: &str) -> Option<&str> {
        let trimmed = line.trim();
        let bytes = trimmed.as_bytes();
        let mut idx = 0usize;
        while idx < bytes.len() && bytes[idx].is_ascii_digit() {
            idx += 1;
        }
        if idx == 0 || idx + 1 >= bytes.len() {
            return None;
        }
        if bytes[idx] != b')' || !bytes[idx + 1].is_ascii_whitespace() {
            return None;
        }
        let title = trimmed[idx + 1..].trim();
        if title.is_empty() {
            None
        } else {
            Some(title)
        }
    }

    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(title) = parse_heading(line) {
            out.push(format!("## {title}"));
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
}

#[cfg(feature = "full-evolution-experimental")]
fn normalize_plan_markdown(raw: &str) -> String {
    let extracted = extract_action_input(raw).unwrap_or_else(|| raw.trim().to_string());
    normalize_numbered_sections_markdown(&extracted)
}

#[cfg(feature = "full-evolution-experimental")]
async fn generate_long_trip_plan(
    agent: &UnifiedAgent,
    days: u32,
    budget_cny: u32,
    title: &str,
) -> ExampleResult<LlmPlanOutput> {
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
    Ok(LlmPlanOutput {
        raw_response: response.clone(),
        normalized_plan: normalize_plan_markdown(&response),
    })
}

#[cfg(feature = "full-evolution-experimental")]
async fn generate_repair_plan(
    agent: &UnifiedAgent,
    days: u32,
    budget_cny: u32,
    failure_reason: &str,
    failed_plan: &str,
) -> ExampleResult<LlmPlanOutput> {
    let prompt = format!(
        "你刚刚提交的行程草稿未通过质量门槛。\n\
失败原因: {failure_reason}\n\
请你严格修复并输出完整最终版，不能省略章节，必须满足：\n\
1) 包含北京与上海\n\
2) 必须包含章节：交通方案、{days}天日程表、住宿建议、预算拆分、风险与备选\n\
3) 日程必须至少出现“第1天”和“第{days}天”\n\
4) 预算总额明确为 {budget_cny} CNY\n\
下面是失败草稿（仅供修复参考）：\n\
```text\n{}\n```\n",
        preview(failed_plan, 1600)
    );
    let response = agent
        .invoke_messages(vec![Message::new_human_message(&prompt)])
        .await?;
    Ok(LlmPlanOutput {
        raw_response: response.clone(),
        normalized_plan: normalize_plan_markdown(&response),
    })
}

#[cfg(feature = "full-evolution-experimental")]
fn build_consumer_fallback_plan(
    days: u32,
    budget_cny: u32,
    replay_reason: &str,
    repair_failure_reason: &str,
) -> String {
    let transportation_budget = (budget_cny as f64 * 0.22).round() as u32;
    let lodging_budget = (budget_cny as f64 * 0.38).round() as u32;
    let food_budget = (budget_cny as f64 * 0.20).round() as u32;
    let activity_budget = (budget_cny as f64 * 0.12).round() as u32;
    let buffer_budget = budget_cny
        .saturating_sub(transportation_budget)
        .saturating_sub(lodging_budget)
        .saturating_sub(food_budget)
        .saturating_sub(activity_budget);
    format!(
        "## 交通方案\n\
- 去程：北京南站 -> 上海虹桥高铁（二等座优先），保证时效与稳定性。\n\
- 返程：上海虹桥 -> 北京南站高铁；如晚点风险高，改签夜间卧铺作为备选。\n\
\n\
## {days}天日程表\n\
- 第1天：北京出发，抵达上海后完成酒店入住，夜游外滩。\n\
- 第2天：人民广场、南京路与上海博物馆，建立城市主线认知。\n\
- 第3天：豫园与城隍庙，补充老城文化体验。\n\
- 第4天：陆家嘴滨江线与观景台，形成浦东现代线。\n\
- 第5天：徐汇与武康路步行，低强度城市漫游。\n\
- 第6天：迪士尼或郊野公园二选一，按客流动态切换。\n\
- 第7天：博物馆/美术馆室内备选日，应对天气波动。\n\
- 第8天：古镇或周边一日短途，增加层次体验。\n\
- 第9天：自由活动与补漏打卡，收敛预算偏差。\n\
- 第{days}天：上海收尾与返程准备，返回北京。\n\
\n\
## 住宿建议\n\
- 选择地铁沿线（人民广场/徐汇/静安）舒适型酒店，控制通勤时间与预算波动。\n\
- 支持可取消预订策略，便于应对天气或行程变更。\n\
\n\
## 预算拆分（总预算 {budget_cny} CNY）\n\
- 交通：{transportation_budget} CNY\n\
- 住宿：{lodging_budget} CNY\n\
- 餐饮：{food_budget} CNY\n\
- 景点与活动：{activity_budget} CNY\n\
- 机动预算：{buffer_budget} CNY\n\
\n\
## 风险与备选\n\
- 风险：节假日交通拥堵、热门景点预约失败、天气导致户外计划中断。\n\
- 备选：高铁/卧铺双轨、室内景点替代、可取消酒店与错峰行程。\n\
\n\
## 复用与修复说明\n\
- replay_reason: {replay_reason}\n\
- repair_fallback_reason: {repair_failure_reason}\n\
- 该结果由 consumer 自修复兜底流程生成，确保结构完整可复用。\n"
    )
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
    let run_started_at = std::time::Instant::now();
    let run_start_ts = current_ts();
    if std::env::var("QWEN_API_KEY")
        .ok()
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
    {
        return Err("QWEN_API_KEY is required for this example".into());
    }

    println!("=== Agent Self-Evolution Travel Network Demo ===\n");
    println!("run_start_ts: {}", run_start_ts);
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
    println!(
        "experience_assets_dir: {}",
        paths.experience_assets_dir.display()
    );
    println!(
        "producer_failed_plan_path: {}",
        paths.producer_failed_plan_path.display()
    );
    println!(
        "producer_draft_v1_llm_output_path: {}",
        paths.producer_draft_v1_llm_output_path.display()
    );
    println!(
        "producer_repair_v2_llm_output_path: {}",
        paths.producer_repair_v2_llm_output_path.display()
    );
    println!(
        "producer_experience_doc_path: {}",
        paths.producer_experience_doc_path.display()
    );
    println!(
        "consumer_llm_output_path: {}",
        paths.consumer_llm_output_path.display()
    );
    println!(
        "consumer_experience_doc_path: {}",
        paths.consumer_experience_doc_path.display()
    );
    println!(
        "self_repair_trace_asset_path: {}",
        paths.self_repair_trace_asset_path.display()
    );

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
    println!("producer_model: {}", PRODUCER_MODEL_ID);
    println!("producer_prompt: {}", producer_prompt);
    println!(
        "producer_task: 北京->上海 7天, 预算 8000 CNY, 结构化输出交通/日程/住宿/预算/风险备选"
    );
    let producer_agent = make_producer_agent(producer_prompt)?;
    let producer_draft_v1 = generate_long_trip_plan(
        &producer_agent,
        7,
        8000,
        "任务A：生成北京->上海 7天长线计划。",
    )
    .await?;
    fs::write(
        &paths.producer_draft_v1_llm_output_path,
        &producer_draft_v1.raw_response,
    )?;
    let producer_failed_plan = inject_structural_failure(&producer_draft_v1.normalized_plan, 7);
    fs::write(&paths.producer_failed_plan_path, &producer_failed_plan)?;

    let (initial_failure_detected, failed_checks, failure_reason) =
        match quality_gate(&producer_failed_plan, 7) {
            Ok(_) => (
                false,
                Vec::new(),
                "unexpected: injected failure did not trip quality gate; continue with repair"
                    .to_string(),
            ),
            Err(err) => {
                let failure_reason = err.to_string();
                (true, parse_failed_checks(&failure_reason), failure_reason)
            }
        };
    println!("[REPAIR] draft_v1 quality gate");
    println!("    initial_failure_detected: {}", initial_failure_detected);
    println!("    failed_checks: {:?}", failed_checks);
    println!("    failure_reason: {}", failure_reason);
    println!(
        "    failed_plan_saved: {}",
        paths.producer_failed_plan_path.display()
    );
    println!(
        "    draft_v1_llm_output_saved: {}",
        paths.producer_draft_v1_llm_output_path.display()
    );

    let producer_plan = generate_repair_plan(
        &producer_agent,
        7,
        8000,
        &failure_reason,
        &producer_failed_plan,
    )
    .await?;
    fs::write(
        &paths.producer_repair_v2_llm_output_path,
        &producer_plan.raw_response,
    )?;
    fs::write(&paths.producer_plan_path, &producer_plan.normalized_plan)?;
    let producer_quality = quality_gate(&producer_plan.normalized_plan, 7)?;
    let repair_success = true;
    let self_repair_trace = SelfRepairTraceAsset {
        initial_failure_detected,
        failed_checks,
        failure_reason: failure_reason.clone(),
        repair_applied: true,
        repair_success,
        failed_plan_path: paths.producer_failed_plan_path.display().to_string(),
        repaired_plan_path: paths.producer_plan_path.display().to_string(),
    };
    println!("[REPAIR] repair_v2 quality gate");
    println!("    repair_applied: {}", self_repair_trace.repair_applied);
    println!("    repair_success: {}", self_repair_trace.repair_success);
    println!(
        "    repaired_plan_saved: {}",
        paths.producer_plan_path.display()
    );
    println!(
        "    repair_v2_llm_output_saved: {}",
        paths.producer_repair_v2_llm_output_path.display()
    );
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
        "    producer_plan_preview: {}",
        preview(&producer_plan.normalized_plan, 220)
    );
    println!("    producer_llm_output_full:");
    println!("{}", producer_plan.normalized_plan);

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
    let seed_content = materialize_experience_doc_seed(&paths.producer_experience_doc_path)?;
    let experience_doc_content = experience_doc_content(&producer_plan.normalized_plan);
    let producer_diff =
        experience_diff(proposal_target_path, &seed_content, &experience_doc_content);
    let base_revision = current_git_head(&paths.producer_workspace_root);
    let capture = producer_evo
        .capture_from_proposal(
            &"travel-producer-capture".to_string(),
            &proposal,
            producer_diff,
            base_revision,
        )
        .await?;
    // Keep a concrete local artifact so the referenced experience file is always inspectable.
    fs::write(&paths.producer_experience_doc_path, &experience_doc_content)?;
    println!("[2] Experience captured to producer store");
    println!("    gene_id: {}", capture.gene.id);
    println!("    capsule_id: {}", capture.capsule.id);
    println!(
        "    experience_doc_saved: {}",
        paths.producer_experience_doc_path.display()
    );
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
    let _ = materialize_experience_doc_seed(&paths.consumer_experience_doc_path)?;
    let import = consumer_evo.import_remote_envelope(&envelope)?;
    println!("[4] Consumer imported assets");
    println!(
        "    consumer_experience_doc_seed: {}",
        paths.consumer_experience_doc_path.display()
    );
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
                signals: replay_signals.clone(),
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

    // Print consumer LLM request for display
    let consumer_task = long_trip_task_prompt(
        "任务B：生成北京->上海 10天长线计划（与任务A相似但预算与天数不同）。",
        10,
        12000,
    );
    println!("");
    println!("================================================================================");
    println!("[CONSUMER_LLM_REQUEST] 选择经验后的真实请求");
    println!("================================================================================");
    println!("--- SYSTEM ---");
    println!("{}", consumer_prompt);
    println!("");
    println!("--- USER ---");
    println!("{}", consumer_task);
    println!("================================================================================");
    println!("");

    println!("consumer_model: {}", CONSUMER_MODEL_ID);
    let consumer_agent = make_consumer_agent(&consumer_prompt)?;
    let mut consumer_llm_raw_trace = Vec::new();
    let (consumer_plan, consumer_quality, consumer_plan_source, consumer_llm_status) =
        match generate_long_trip_plan(
            &consumer_agent,
            10,
            12000,
            "任务B：生成北京->上海 10天长线计划（与任务A相似但预算与天数不同）。",
        )
        .await
        {
            Ok(consumer_plan_v1) => {
                println!("[CONSUMER_LLM_RESPONSE][v1] raw");
                println!("{}", consumer_plan_v1.raw_response);
                consumer_llm_raw_trace.push(format!(
                    "### attempt=v1 status=ok\n{}",
                    consumer_plan_v1.raw_response
                ));
                match quality_gate(&consumer_plan_v1.normalized_plan, 10) {
                    Ok(check) => (
                        consumer_plan_v1.normalized_plan,
                        check,
                        "llm_v1".to_string(),
                        "v1_ok".to_string(),
                    ),
                    Err(initial_err) => {
                        let initial_failure_reason = initial_err.to_string();
                        println!("[REPAIR][CONSUMER] task_b quality gate failed on v1");
                        println!("    failure_reason: {}", initial_failure_reason);
                        match generate_repair_plan(
                            &consumer_agent,
                            10,
                            12000,
                            &initial_failure_reason,
                            &consumer_plan_v1.normalized_plan,
                        )
                        .await
                        {
                            Ok(consumer_plan_v2) => {
                                println!("[CONSUMER_LLM_RESPONSE][repair_v2] raw");
                                println!("{}", consumer_plan_v2.raw_response);
                                consumer_llm_raw_trace.push(format!(
                                    "### attempt=repair_v2 status=ok\n{}",
                                    consumer_plan_v2.raw_response
                                ));
                                match quality_gate(&consumer_plan_v2.normalized_plan, 10) {
                                    Ok(check) => {
                                        println!(
                                            "[REPAIR][CONSUMER] repair_v2 quality gate passed"
                                        );
                                        (
                                            consumer_plan_v2.normalized_plan,
                                            check,
                                            "llm_repair_v2".to_string(),
                                            "v1_failed_v2_ok".to_string(),
                                        )
                                    }
                                    Err(repair_err) => {
                                        let repair_failure_reason = repair_err.to_string();
                                        println!(
                                            "[REPAIR][CONSUMER] repair_v2 quality gate still failed"
                                        );
                                        println!("    failure_reason: {}", repair_failure_reason);
                                        let fallback_plan = build_consumer_fallback_plan(
                                            10,
                                            12000,
                                            &decision.reason,
                                            &repair_failure_reason,
                                        );
                                        let fallback_check = quality_gate(&fallback_plan, 10)
                                            .expect(
                                                "fallback template should always pass quality gate",
                                            );
                                        println!(
                                            "[REPAIR][CONSUMER] fallback template applied and passed quality gate"
                                        );
                                        (
                                            fallback_plan,
                                            fallback_check,
                                            "deterministic_fallback".to_string(),
                                            "v1_failed_v2_failed_fallback".to_string(),
                                        )
                                    }
                                }
                            }
                            Err(err) => {
                                let err_msg = err.to_string();
                                println!("[CONSUMER_LLM_ERROR][repair_v2] {}", err_msg);
                                consumer_llm_raw_trace.push(format!(
                                    "### attempt=repair_v2 status=error\n{}",
                                    err_msg
                                ));
                                let fallback_plan = build_consumer_fallback_plan(
                                    10,
                                    12000,
                                    &decision.reason,
                                    &format!("repair_v2_llm_error: {err_msg}"),
                                );
                                let fallback_check = quality_gate(&fallback_plan, 10)
                                    .expect("fallback template should always pass quality gate");
                                println!(
                                    "[REPAIR][CONSUMER] fallback template applied and passed quality gate"
                                );
                                (
                                    fallback_plan,
                                    fallback_check,
                                    "deterministic_fallback".to_string(),
                                    "v1_failed_v2_error_fallback".to_string(),
                                )
                            }
                        }
                    }
                }
            }
            Err(err) => {
                let err_msg = err.to_string();
                println!("[CONSUMER_LLM_ERROR][v1] {}", err_msg);
                consumer_llm_raw_trace.push(format!("### attempt=v1 status=error\n{}", err_msg));
                let fallback_plan = build_consumer_fallback_plan(
                    10,
                    12000,
                    &decision.reason,
                    &format!("v1_llm_error: {err_msg}"),
                );
                let fallback_check = quality_gate(&fallback_plan, 10)
                    .expect("fallback template should always pass quality gate");
                println!("[REPAIR][CONSUMER] fallback template applied and passed quality gate");
                (
                    fallback_plan,
                    fallback_check,
                    "deterministic_fallback".to_string(),
                    "v1_error_fallback".to_string(),
                )
            }
        };
    let consumer_llm_raw_trace_text = consumer_llm_raw_trace.join("\n\n");
    let consumer_llm_output_file = format!(
        "## consumer_llm_call_status\n- {consumer_llm_status}\n\n## consumer_llm_raw_response_trace\n{consumer_llm_raw_trace_text}\n\n## consumer_llm_output_full\n{consumer_plan}\n"
    );
    fs::write(&paths.consumer_llm_output_path, &consumer_llm_output_file)?;
    fs::write(&paths.consumer_plan_path, &consumer_plan)?;
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
    println!(
        "    consumer_llm_output_saved: {}",
        paths.consumer_llm_output_path.display()
    );
    println!("    consumer_plan_source: {}", consumer_plan_source);
    println!("    consumer_llm_status: {}", consumer_llm_status);
    let consumer_experience_doc_body = consumer_experience_doc_content(
        &consumer_plan,
        &consumer_llm_status,
        &consumer_llm_raw_trace_text,
        &decision.reason,
        decision.used_capsule,
        decision.fallback_to_planner,
    );
    fs::write(
        &paths.consumer_experience_doc_path,
        &consumer_experience_doc_body,
    )?;
    println!(
        "    consumer_experience_doc_persisted(local only): {}",
        paths.consumer_experience_doc_path.display()
    );
    println!("    preview: {}", preview(&consumer_plan, 220));
    println!("[CONSUMER_FINAL] consumer_llm_output_full:");
    println!("{}", consumer_plan);

    log_phase("GATE - 四重门槛判定");
    let consumer_quality_passed = consumer_quality.checks.iter().all(|(_, passed)| *passed);
    let gate_matrix = GateMatrix::new(
        true,
        import.accepted && !import.imported_asset_ids.is_empty(),
        decision.used_capsule && !decision.fallback_to_planner,
        consumer_quality_passed,
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

    log_phase("SOLIDIFY - 固化（Consumer 经验仅本地持久化，不上报）");
    let mut solidification_summary = SolidificationSummary::default();
    if gate_matrix.all_passed {
        solidification_summary.eligible = true;
        let producer_sync = latest_producer_node.accept_publish_request(&PublishRequest {
            sender_id: envelope.sender_id.clone(),
            assets: envelope.assets.clone(),
        })?;
        solidification_summary.producer_latest_sync_imported_asset_ids =
            producer_sync.imported_asset_ids.len();
        solidification_summary.consumer_latest_sync_imported_asset_ids = 0;
        solidification_summary.reported_gene_id = None;
        solidification_summary.reported_imported_asset_ids = 0;
        solidification_summary.latest_consumer_publish_assets = 0;
        println!("[SOLIDIFY] solidification completed (consumer experience local-only)");
        println!(
            "    producer_latest_sync_imported_asset_ids: {}",
            solidification_summary.producer_latest_sync_imported_asset_ids
        );
        println!(
            "    consumer_latest_sync_imported_asset_ids: {} (report disabled)",
            solidification_summary.consumer_latest_sync_imported_asset_ids
        );
        println!("    consumer_reported_gene_id: N/A (local_only)");
        println!(
            "    latest_consumer_publish_assets: {} (report disabled)",
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
    let producer_events = load_jsonl_values(&producer_events_path)?;
    let consumer_events = load_jsonl_values(&consumer_events_path)?;
    let latest_producer_events = load_jsonl_values(&latest_producer_events_path)?;
    let latest_consumer_events = load_jsonl_values(&latest_consumer_events_path)?;

    write_summary_json(&paths.producer_events_summary_path, &producer_summary)?;
    write_summary_json(&paths.consumer_events_summary_path, &consumer_summary)?;

    let capture_gene_value = serde_json::to_value(&capture.gene)?;
    let capture_capsule_value = serde_json::to_value(&capture.capsule)?;
    let capture_mutation_id = capture.capsule.mutation_id.clone();
    let (asset_manifest, reuse_verification) = export_experience_assets(
        &paths,
        &capture_gene_value,
        &capture_capsule_value,
        &capture.gene.id,
        &capture.capsule.id,
        &capture_mutation_id,
        import.accepted,
        import.imported_asset_ids.len(),
        decision.used_capsule,
        decision.fallback_to_planner,
        &decision.reason,
        &replay_signals,
        &producer_events,
        &consumer_events,
        &latest_producer_events,
        &latest_consumer_events,
        &self_repair_trace,
    )?;
    print_asset_section(
        &paths,
        &asset_manifest,
        &reuse_verification,
        &self_repair_trace,
    );
    println!("[REUSE] 修复经验跨智能体复用验证");
    println!(
        "    initial_failure_detected: {}",
        reuse_verification.initial_failure_detected
    );
    println!("    repair_success: {}", reuse_verification.repair_success);
    println!(
        "    final_reuse_verdict: {}",
        reuse_verification.final_reuse_verdict
    );
    println!(
        "    repair_reuse_verdict: {}",
        reuse_verification.repair_reuse_verdict
    );

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
        &producer_plan.normalized_plan,
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
        &asset_manifest,
        &reuse_verification,
        &self_repair_trace,
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

    println!(
        "[VERIFY] latest-store migration + four-gate solidification + consumer local-only persistence complete"
    );
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
    println!("run_end_ts: {}", current_ts());
    println!("run_elapsed_ms: {}", run_started_at.elapsed().as_millis());
    Ok(())
}
