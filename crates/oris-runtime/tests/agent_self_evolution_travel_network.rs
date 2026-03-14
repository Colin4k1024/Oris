#![cfg(feature = "full-evolution-experimental")]

use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use oris_runtime::agent_contract::{
    AgentTask, HumanApproval, MutationProposal, SupervisedDeliveryReasonCode,
    SupervisedDeliveryStatus, SupervisedDevloopOutcome, SupervisedDevloopRequest,
    SupervisedDevloopStatus,
};
use oris_runtime::evolution::{
    CommandValidator, EvoAssetState, EvoEvolutionStore as EvolutionStore, EvoKernel,
    EvoSandboxPolicy as SandboxPolicy, EvoSelectorInput as SelectorInput, EvolutionNetworkNode,
    FetchQuery, JsonlEvolutionStore, LocalProcessSandbox, PublishRequest,
    ReplayRoiReleaseGateStatus, ReplayRoiReleaseGateThresholds, ValidationPlan,
};
use oris_runtime::governor::{DefaultGovernor, GovernorConfig};
use oris_runtime::kernel::{
    AllowAllPolicy, InMemoryEventStore, Kernel, KernelMode, KernelState, NoopActionExecutor,
    NoopStepFn, StateUpdatedOnlyReducer,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const FIXED_SIGNAL_TAGS: [&str; 5] = [
    "travel_planning",
    "route:beijing-shanghai",
    "trip:longline",
    "city:beijing",
    "city:shanghai",
];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct TestState;

impl KernelState for TestState {
    fn version(&self) -> u32 {
        1
    }
}

struct StubBrain;

impl StubBrain {
    fn generate_plan(days: u32, budget_cny: u32, experience_hint: &str) -> String {
        format!(
            "交通方案\n- 北京南站乘高铁至上海虹桥，必要时飞机备选\n\n\
{days}天日程表\n\
- 第1天：北京出发，抵达上海并外滩夜游\n\
- 第2天：南京路、人民广场、博物馆\n\
- 第3天：豫园、城隍庙、美食步行\n\
- 第4天：徐汇与武康路城市漫游\n\
- 第5天：浦东陆家嘴、滨江线\n\
- 第6天：迪士尼或郊野公园\n\
- 第7天：弹性安排与购物\n\
- 第{days}天：总结行程与返程准备\n\n\
住宿建议\n- 优先地铁沿线，控制通勤时间\n\n\
预算拆分\n- 总预算：{budget_cny} CNY\n- 交通/住宿/餐饮/景点/机动分层控制\n\n\
风险与备选\n- 高峰拥堵、天气变化、热门景点预约失败\n- 备选：错峰出发、室内替代景点、可取消酒店\n\n\
经验注入\n- {experience_hint}\n"
        )
    }
}

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

fn quality_gate_result(plan: &str, days: u32) -> Result<(), String> {
    let checks = [
        ("包含“北京”", plan.contains("北京")),
        ("包含“上海”", plan.contains("上海")),
        (
            "包含“交通方案”章节",
            plan.contains("交通方案") || plan.contains("交通"),
        ),
        (
            "包含“日程表”章节",
            plan.contains("日程表") || plan.contains("行程"),
        ),
        (
            "包含“住宿建议”章节",
            plan.contains("住宿建议") || plan.contains("住宿"),
        ),
        (
            "包含“预算拆分”章节",
            plan.contains("预算拆分") || plan.contains("预算"),
        ),
        (
            "包含“风险与备选”章节",
            plan.contains("风险") && plan.contains("备选"),
        ),
    ];
    let missing = checks
        .iter()
        .filter_map(|(name, passed)| {
            if *passed {
                None
            } else {
                Some((*name).to_string())
            }
        })
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "quality gate failed, missing sections: {:?}",
            missing
        ));
    }
    let first_day_ok = plan.contains("第1天") || plan.to_ascii_lowercase().contains("day 1");
    let last_day = format!("第{}天", days);
    let last_day_ok =
        plan.contains(&last_day) || plan.to_ascii_lowercase().contains(&format!("day {days}"));
    if !first_day_ok || !last_day_ok {
        return Err(format!(
            "quality gate failed, day markers missing: first_day_ok={first_day_ok}, last_day_ok={last_day_ok}"
        ));
    }
    Ok(())
}

fn quality_gate(plan: &str, days: u32) {
    if let Err(err) = quality_gate_result(plan, days) {
        panic!("{err}");
    }
}

fn inject_structural_failure(plan: &str, total_days: u32) -> String {
    let mut corrupted = plan.replace("风险", "注意").replace("备选", "替代");
    let last_day_marker = format!("第{}天", total_days);
    corrupted = corrupted.replace(&last_day_marker, "最后一天");
    corrupted
}

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

fn unique_path(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("oris-travel-network-test-{label}-{nanos}"))
}

fn create_audit_log_path(test_name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::current_dir()
        .unwrap()
        .join("target/test-audit/agent_self_evolution_travel_network");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join(format!("{test_name}-{nanos}.log"));
    std::fs::write(&path, format!("[INIT] test={test_name} nanos={nanos}\n")).unwrap();
    path
}

fn append_audit_log(path: &Path, message: impl AsRef<str>) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    file.write_all(message.as_ref().as_bytes()).unwrap();
    file.write_all(b"\n").unwrap();
}

fn create_realtime_log_paths(test_name: &str) -> (PathBuf, PathBuf) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::current_dir()
        .unwrap()
        .join("target/test-audit/agent_self_evolution_travel_network/realtime");
    std::fs::create_dir_all(&root).unwrap();
    (
        root.join(format!("{test_name}-{nanos}.log")),
        root.join(format!("{test_name}-{nanos}.jsonl")),
    )
}

fn append_realtime_event(
    log_path: &Path,
    jsonl_path: &Path,
    run_id: &str,
    phase: &str,
    event: &str,
    payload: Value,
) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .to_string();
    let record = json!({
        "ts": ts,
        "run_id": run_id,
        "agent_role": "stub-agent",
        "phase": phase,
        "event": event,
        "payload": payload,
    });
    let line = format!(
        "[{}] run_id={} phase={} event={} payload={}",
        ts, run_id, phase, event, record["payload"]
    );
    println!("{line}");
    let mut log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .unwrap();
    writeln!(log_file, "{line}").unwrap();
    let mut jsonl_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(jsonl_path)
        .unwrap();
    writeln!(jsonl_file, "{}", serde_json::to_string(&record).unwrap()).unwrap();
}

fn assert_stub_realtime_logs(jsonl_path: &Path) {
    let mut phase_count = 0usize;
    let mut tool_count = 0usize;
    let mut finish_count = 0usize;
    for line in std::fs::read_to_string(jsonl_path)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let value: Value = serde_json::from_str(line).unwrap();
        match value
            .get("event")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "phase_transition" => phase_count += 1,
            "tool_call_before" | "tool_call_after" => tool_count += 1,
            "finish" => finish_count += 1,
            _ => {}
        }
    }
    assert!(phase_count > 0, "missing phase_transition logs");
    assert!(tool_count > 0, "missing tool_call logs");
    assert!(finish_count > 0, "missing finish logs");
}

fn demo_policy() -> SandboxPolicy {
    SandboxPolicy {
        allowed_programs: vec!["git".into(), "cargo".into()],
        max_duration_ms: 30_000,
        max_output_bytes: 1_048_576,
        denied_env_prefixes: vec!["TOKEN".into(), "KEY".into(), "SECRET".into()],
    }
}

fn empty_validation_plan() -> ValidationPlan {
    ValidationPlan {
        profile: "test-empty-validation".into(),
        stages: vec![],
    }
}

fn build_test_evo(
    label: &str,
) -> (
    PathBuf,
    Arc<JsonlEvolutionStore>,
    EvoKernel<TestState>,
    PathBuf,
    PathBuf,
) {
    let workspace = std::env::current_dir().unwrap();
    let sandbox_root = unique_path(&format!("{label}-sandbox"));
    let store_root = unique_path(&format!("{label}-store"));
    std::fs::create_dir_all(&sandbox_root).unwrap();
    std::fs::create_dir_all(&store_root).unwrap();

    let store = Arc::new(JsonlEvolutionStore::new(store_root.clone()));
    let validator = Arc::new(CommandValidator::new(demo_policy()));
    let sandbox = Arc::new(LocalProcessSandbox::new(
        format!("run-{label}"),
        &workspace,
        &sandbox_root,
    ));
    let kernel = Arc::new(Kernel::<TestState> {
        events: Box::new(InMemoryEventStore::new()),
        snaps: None,
        reducer: Box::new(StateUpdatedOnlyReducer),
        exec: Box::new(NoopActionExecutor),
        step: Box::new(NoopStepFn),
        policy: Box::new(AllowAllPolicy),
        effect_sink: None,
        mode: KernelMode::Normal,
    });

    let evo = EvoKernel::new(
        kernel,
        sandbox,
        validator,
        store.clone() as Arc<dyn EvolutionStore>,
    )
    .with_governor(Arc::new(DefaultGovernor::new(GovernorConfig {
        promote_after_successes: 1,
        ..Default::default()
    })))
    .with_sandbox_policy(demo_policy())
    .with_validation_plan(empty_validation_plan());

    (workspace, store, evo, sandbox_root, store_root)
}

fn delivery_request(task_id: &str, file: &str, approved: bool) -> SupervisedDevloopRequest {
    SupervisedDevloopRequest {
        task: AgentTask {
            id: task_id.to_string(),
            description: format!("Prepare delivery artifacts for {file}"),
        },
        proposal: MutationProposal {
            intent: format!("Update {file}"),
            files: vec![file.to_string()],
            expected_effect: format!("Keep {file} in sync"),
        },
        approval: HumanApproval {
            approved,
            approver: approved.then(|| "maintainer".to_string()),
            note: Some("travel-network regression".to_string()),
        },
    }
}

fn delivery_diff(path: &str, title: &str) -> String {
    format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,3 @@\n+# {title}\n+\n+bounded delivery preparation\n"
    )
}

fn experience_diff(path: &str, plan_preview: &str) -> String {
    let signal_block = FIXED_SIGNAL_TAGS.join(", ");
    let mut content_lines = vec![
        "# Beijing Shanghai Longline Experience".to_string(),
        "".to_string(),
        format!("signals: {signal_block}"),
        "".to_string(),
        "strategy:".to_string(),
        "- call deterministic tools first".to_string(),
        "- then generate final integrated plan".to_string(),
        "".to_string(),
        "preview:".to_string(),
    ];
    content_lines.extend(plan_preview.lines().map(|line| line.to_string()));
    let hunk_line_count = content_lines.len();
    let patch_body = content_lines
        .iter()
        .map(|line| format!("+{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{hunk_line_count} @@\n{patch_body}\n",
        path = path,
        hunk_line_count = hunk_line_count,
        patch_body = patch_body,
    )
}

fn merge_signals(base: &[String], extra: &[String]) -> Vec<String> {
    let mut merged = BTreeSet::new();
    for signal in base {
        merged.insert(signal.clone());
    }
    for signal in extra {
        merged.insert(signal.clone());
    }
    merged.into_iter().collect()
}

fn strategy_value(strategy: &[String], key: &str) -> Option<String> {
    strategy.iter().find_map(|entry| {
        let (candidate_key, candidate_value) = entry.split_once('=')?;
        if candidate_key.trim().eq_ignore_ascii_case(key) {
            let value = candidate_value.trim();
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        } else {
            None
        }
    })
}

fn discover_historical_store_dirs(
    demo_runs_root: &Path,
    current_run_id: &str,
    side_store_name: &str,
) -> Vec<PathBuf> {
    if !demo_runs_root.exists() {
        return Vec::new();
    }
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(demo_runs_root).unwrap() {
        let entry = entry.unwrap();
        let run_path = entry.path();
        if !run_path.is_dir() {
            continue;
        }
        if run_path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name == current_run_id)
            .unwrap_or(false)
        {
            continue;
        }
        let candidate = run_path.join(side_store_name);
        if candidate.join("events.jsonl").exists() {
            dirs.push(candidate);
        }
    }
    dirs.sort();
    dirs
}

fn migrate_sources_to_latest(
    source_dirs: &[PathBuf],
    latest_node: &EvolutionNetworkNode,
    sender_prefix: &str,
) -> usize {
    let mut imported_asset_ids = 0;
    for source_dir in source_dirs {
        let source_store = Arc::new(JsonlEvolutionStore::new(source_dir.to_path_buf()));
        let source_node = EvolutionNetworkNode::new(source_store as Arc<dyn EvolutionStore>);
        let sender_id = format!(
            "{sender_prefix}-{}",
            source_dir
                .parent()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
                .unwrap_or("unknown-run")
        );
        let envelope = source_node.publish_local_assets(sender_id).unwrap();
        if envelope.assets.is_empty() {
            continue;
        }
        let import = latest_node
            .accept_publish_request(&PublishRequest {
                sender_id: envelope.sender_id.clone(),
                assets: envelope.assets.clone(),
                since_cursor: None,
                resume_token: None,
            })
            .unwrap();
        imported_asset_ids += import.imported_asset_ids.len();
    }
    imported_asset_ids
}

fn build_gate_matrix(
    producer_quality_passed: bool,
    consumer_import_success: bool,
    replay_hit: bool,
    consumer_quality_passed: bool,
) -> bool {
    producer_quality_passed && consumer_import_success && replay_hit && consumer_quality_passed
}

fn solidify_if_gate_passed(
    latest_consumer_node: &EvolutionNetworkNode,
    gate_all_passed: bool,
    run_id: &str,
    source_capsule_id: &str,
    source_gene_id: &str,
) -> Option<String> {
    if !gate_all_passed {
        return None;
    }
    let reported_gene_id = format!("reported-travel-{run_id}-{source_capsule_id}");
    latest_consumer_node
        .record_reported_experience(
            "agent-consumer",
            reported_gene_id.clone(),
            vec![
                "travel planning".to_string(),
                "route beijing shanghai".to_string(),
                "trip longline".to_string(),
                "travel.longline.beijing-shanghai".to_string(),
            ],
            vec![
                "asset_origin=reported_experience".to_string(),
                "task_class=travel.longline.beijing-shanghai".to_string(),
                "task_label=北京到上海长线规划".to_string(),
                format!("source_capsule={source_capsule_id}"),
                format!("source_gene={source_gene_id}"),
                "summary=four-gate verified and solidified".to_string(),
            ],
            vec!["travel.demo.four-gate".to_string()],
        )
        .unwrap();
    Some(reported_gene_id)
}

fn count_reported_promoted_genes(store: &JsonlEvolutionStore) -> usize {
    let projection = store.rebuild_projection().unwrap();
    projection
        .genes
        .iter()
        .filter(|gene| {
            gene.state == EvoAssetState::Promoted
                && strategy_value(&gene.strategy, "asset_origin").as_deref()
                    == Some("reported_experience")
        })
        .count()
}

fn load_jsonl_values(path: &Path) -> Vec<Value> {
    if !path.exists() {
        return Vec::new();
    }
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                None
            } else {
                serde_json::from_str::<Value>(line).ok()
            }
        })
        .collect()
}

fn write_jsonl_values(path: &Path, values: &[Value]) {
    let mut payload = String::new();
    for value in values {
        payload.push_str(&serde_json::to_string(value).unwrap());
        payload.push('\n');
    }
    std::fs::write(path, payload).unwrap();
}

fn write_json_value(path: &Path, value: &Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

fn event_kind(record: &Value) -> Option<&str> {
    record
        .get("event")
        .and_then(|event| event.get("kind"))
        .and_then(Value::as_str)
}

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

fn extract_mutation_snapshot(records: &[Value], mutation_id: &str) -> Option<Value> {
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

fn extract_validation_report_snapshot(records: &[Value], mutation_id: &str) -> Option<Value> {
    records.iter().find_map(|record| {
        let kind = event_kind(record)?;
        if !matches!(kind, "validation_passed" | "validation_failed")
            || event_mutation_id(record).as_deref() != Some(mutation_id)
        {
            return None;
        }
        record
            .get("event")
            .and_then(|event| event.get("report"))
            .cloned()
    })
}

fn detect_capsule_reused_event(records: &[Value], capsule_id: &str) -> bool {
    records.iter().any(|record| {
        event_kind(record) == Some("capsule_reused")
            && event_capsule_id(record).as_deref() == Some(capsule_id)
    })
}

fn collect_promotion_reason_codes(records: &[Value]) -> BTreeSet<String> {
    records
        .iter()
        .filter(|record| event_kind(record) == Some("promotion_evaluated"))
        .filter_map(|record| {
            record
                .get("event")
                .and_then(|event| event.get("reason_code"))
                .and_then(Value::as_str)
                .map(|value| value.to_string())
        })
        .collect()
}

fn final_reuse_verdict(
    import_accepted: bool,
    imported_asset_count: usize,
    used_capsule: bool,
    fallback_to_planner: bool,
    capsule_reused_event_detected: bool,
) -> bool {
    import_accepted
        && imported_asset_count > 0
        && used_capsule
        && !fallback_to_planner
        && capsule_reused_event_detected
}

fn repair_reuse_verdict(
    initial_failure_detected: bool,
    repair_success: bool,
    import_accepted: bool,
    imported_asset_count: usize,
    used_capsule: bool,
    fallback_to_planner: bool,
    capsule_reused_event_detected: bool,
) -> bool {
    initial_failure_detected
        && repair_success
        && import_accepted
        && imported_asset_count > 0
        && used_capsule
        && !fallback_to_planner
        && capsule_reused_event_detected
}

fn build_memory_graph_events(
    run_id: &str,
    replay_signals: &[String],
    gene_id: &str,
    capsule_id: &str,
    replay_success: bool,
    reason: &str,
) -> Vec<Value> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .to_string();
    let signal_event_id = format!("signal-{run_id}");
    let gene_event_id = format!("gene-selected-{run_id}");
    let outcome_event_id = format!("outcome-{run_id}");
    let capsule_event_id = format!("capsule-{run_id}");
    vec![
        json!({
            "type": "MemoryGraphEvent",
            "kind": "signal",
            "id": signal_event_id,
            "ts": ts,
            "signal": {"signals": replay_signals},
        }),
        json!({
            "type": "MemoryGraphEvent",
            "kind": "gene_selected",
            "id": gene_event_id,
            "ts": ts,
            "gene": {"id": gene_id},
            "parent": signal_event_id,
        }),
        json!({
            "type": "MemoryGraphEvent",
            "kind": "outcome",
            "id": outcome_event_id,
            "ts": ts,
            "outcome": {
                "status": if replay_success { "success" } else { "failed" },
                "score": if replay_success { 1.0 } else { 0.0 },
                "note": reason,
            },
            "parent": gene_event_id,
        }),
        json!({
            "type": "MemoryGraphEvent",
            "kind": "capsule_created",
            "id": capsule_event_id,
            "ts": ts,
            "signal": {"capsule_id": capsule_id},
            "parent": outcome_event_id,
        }),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestAssetManifestEntry {
    path: String,
    exists: bool,
    summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestAssetManifest {
    run_id: String,
    assets: BTreeMap<String, TestAssetManifestEntry>,
    missing_assets: Vec<String>,
}

fn build_test_asset_manifest(run_id: &str, root: &Path) -> TestAssetManifest {
    let specs = [
        ("gene", "gene.json"),
        ("capsule", "capsule.json"),
        ("evolution_events", "evolution_events.jsonl"),
        ("mutation", "mutation.json"),
        ("validation_report", "validation_report.json"),
        ("memory_graph_events", "memory_graph_events.jsonl"),
        ("self_repair_trace", "self_repair_trace.json"),
    ];
    let mut assets = BTreeMap::new();
    let mut missing_assets = Vec::new();
    for (asset_name, file_name) in specs {
        let path = root.join(file_name);
        let exists = path.exists();
        if !exists {
            missing_assets.push(asset_name.to_string());
        }
        assets.insert(
            asset_name.to_string(),
            TestAssetManifestEntry {
                path: path.display().to_string(),
                exists,
                summary: if exists {
                    "present".to_string()
                } else {
                    "missing".to_string()
                },
            },
        );
    }
    missing_assets.sort();
    TestAssetManifest {
        run_id: run_id.to_string(),
        assets,
        missing_assets,
    }
}

#[tokio::test]
async fn travel_network_demo_flow_captures_publishes_imports_and_replays() {
    let audit_log =
        create_audit_log_path("travel_network_demo_flow_captures_publishes_imports_and_replays");
    let (realtime_log_path, realtime_jsonl_path) = create_realtime_log_paths(
        "travel_network_demo_flow_captures_publishes_imports_and_replays",
    );
    let realtime_run_id = "stub-travel-reuse";
    append_realtime_event(
        &realtime_log_path,
        &realtime_jsonl_path,
        realtime_run_id,
        "bootstrap",
        "phase_transition",
        json!({"stage": "bootstrap"}),
    );
    let producer_plan_base = StubBrain::generate_plan(7, 8000, "none");
    let draft_v1 = inject_structural_failure(&producer_plan_base, 7);
    let draft_failure_reason = quality_gate_result(&draft_v1, 7)
        .expect_err("draft_v1 should fail quality gate after deterministic injection");
    let initial_failure_detected = true;
    let failed_checks = parse_failed_checks(&draft_failure_reason);
    assert!(
        !failed_checks.is_empty(),
        "failed checks should be extracted from quality gate error"
    );
    append_audit_log(
        &audit_log,
        format!(
            "[STEP] draft_v1 failed as expected reason={} failed_checks={:?}",
            draft_failure_reason, failed_checks
        ),
    );

    let producer_plan =
        StubBrain::generate_plan(7, 8000, &format!("repair:{draft_failure_reason}"));
    quality_gate(&producer_plan, 7);
    let repair_success = true;
    append_audit_log(&audit_log, "[STEP] repair_v2 quality gate passed");
    append_realtime_event(
        &realtime_log_path,
        &realtime_jsonl_path,
        realtime_run_id,
        "producer",
        "phase_transition",
        json!({"stage": "[1] producer plan generated"}),
    );

    let (_workspace, producer_store, producer_evo, _producer_sandbox, _producer_store_root) =
        build_test_evo("producer");
    let target_path = "docs/evolution/travel-beijing-shanghai-experience.md";
    let mut proposal_files = vec![target_path.to_string()];
    proposal_files.extend(FIXED_SIGNAL_TAGS.iter().map(|s| s.to_string()));
    let proposal = MutationProposal {
        intent: "capture reusable travel planning experience for Beijing-Shanghai longline tasks"
            .into(),
        files: proposal_files,
        expected_effect: "promote reusable experience for similar cross-agent travel tasks".into(),
    };
    let capture = producer_evo
        .capture_from_proposal(
            &"travel-producer-capture".to_string(),
            &proposal,
            experience_diff(target_path, &preview(&producer_plan, 320)),
            None,
        )
        .await
        .unwrap();
    append_realtime_event(
        &realtime_log_path,
        &realtime_jsonl_path,
        realtime_run_id,
        "producer-tool",
        "tool_call_before",
        json!({"tool": "capture_from_proposal"}),
    );
    append_realtime_event(
        &realtime_log_path,
        &realtime_jsonl_path,
        realtime_run_id,
        "producer-tool",
        "tool_call_after",
        json!({"tool": "capture_from_proposal", "gene_id": &capture.gene.id, "capsule_id": &capture.capsule.id}),
    );
    append_audit_log(
        &audit_log,
        format!(
            "[STEP] capture_from_proposal gene_id={} capsule_id={}",
            capture.gene.id, capture.capsule.id
        ),
    );

    let producer_projection = producer_store.rebuild_projection().unwrap();
    assert_eq!(capture.gene.state, EvoAssetState::Promoted);
    assert_eq!(capture.capsule.state, EvoAssetState::Promoted);
    assert!(producer_projection
        .genes
        .iter()
        .any(|gene| gene.id == capture.gene.id && gene.state == EvoAssetState::Promoted));
    assert!(producer_projection.capsules.iter().any(
        |capsule| capsule.id == capture.capsule.id && capsule.state == EvoAssetState::Promoted
    ));

    let producer_node =
        EvolutionNetworkNode::new(producer_store.clone() as Arc<dyn EvolutionStore>);
    let envelope = producer_node
        .publish_local_assets("agent-producer")
        .unwrap();
    assert!(!envelope.assets.is_empty());
    append_audit_log(
        &audit_log,
        format!("[STEP] producer publish assets={}", envelope.assets.len()),
    );

    let (_workspace, consumer_store, consumer_evo, _consumer_sandbox, _consumer_store_root) =
        build_test_evo("consumer");
    let import = consumer_evo.import_remote_envelope(&envelope).unwrap();
    assert!(import.accepted);
    assert!(!import.imported_asset_ids.is_empty());
    append_audit_log(
        &audit_log,
        format!(
            "[STEP] consumer import accepted={} imported_ids={}",
            import.accepted,
            import.imported_asset_ids.len()
        ),
    );

    let fixed_signals = vec![
        "travel planning".to_string(),
        "route beijing shanghai".to_string(),
        "trip longline".to_string(),
        "city beijing".to_string(),
        "city shanghai".to_string(),
    ];
    let first_decision = consumer_evo
        .replay_or_fallback_for_run(
            &"travel-consumer-replay".to_string(),
            SelectorInput {
                signals: merge_signals(&fixed_signals, &capture.gene.signals),
                env: capture.capsule.env.clone(),
                spec_id: None,
                limit: 1,
            },
        )
        .await
        .unwrap();
    append_realtime_event(
        &realtime_log_path,
        &realtime_jsonl_path,
        realtime_run_id,
        "consumer",
        "phase_transition",
        json!({
            "stage": "[5] replay-shadow",
            "used_capsule": first_decision.used_capsule,
            "fallback": first_decision.fallback_to_planner
        }),
    );

    assert!(first_decision.used_capsule);
    assert!(!first_decision.fallback_to_planner);
    append_audit_log(
        &audit_log,
        format!(
            "[STEP] replay-shadow used_capsule={} fallback={} reason={}",
            first_decision.used_capsule, first_decision.fallback_to_planner, first_decision.reason
        ),
    );

    let second_decision = consumer_evo
        .replay_or_fallback_for_run(
            &"travel-consumer-replay-2".to_string(),
            SelectorInput {
                signals: merge_signals(&fixed_signals, &capture.gene.signals),
                env: capture.capsule.env.clone(),
                spec_id: None,
                limit: 1,
            },
        )
        .await
        .unwrap();
    append_realtime_event(
        &realtime_log_path,
        &realtime_jsonl_path,
        realtime_run_id,
        "consumer",
        "phase_transition",
        json!({
            "stage": "[6] replay-promote",
            "used_capsule": second_decision.used_capsule,
            "fallback": second_decision.fallback_to_planner
        }),
    );
    assert!(second_decision.used_capsule);
    assert!(!second_decision.fallback_to_planner);
    let replay_feedback = EvoKernel::<TestState>::replay_feedback_for_agent(
        &merge_signals(&fixed_signals, &capture.gene.signals),
        &second_decision,
    );
    assert_eq!(
        replay_feedback.task_class_id,
        second_decision.detect_evidence.task_class_id
    );
    assert_eq!(
        replay_feedback.task_label,
        second_decision.detect_evidence.task_label
    );
    append_audit_log(
        &audit_log,
        format!(
            "[STEP] replay-promote used_capsule={} fallback={} reason={}",
            second_decision.used_capsule,
            second_decision.fallback_to_planner,
            second_decision.reason
        ),
    );

    let producer_events = load_jsonl_values(&producer_store.root_dir().join("events.jsonl"));
    let consumer_events = load_jsonl_values(&consumer_store.root_dir().join("events.jsonl"));
    let mut all_events = Vec::new();
    all_events.extend(producer_events.clone());
    all_events.extend(consumer_events.clone());

    let mutation_snapshot = extract_mutation_snapshot(&all_events, &capture.capsule.mutation_id)
        .expect("expected mutation_declared snapshot for the captured mutation_id");
    assert_eq!(
        mutation_snapshot
            .get("intent")
            .and_then(|intent| intent.get("id"))
            .and_then(Value::as_str),
        Some(capture.capsule.mutation_id.as_str())
    );
    let mutation_snapshot_text = serde_json::to_string(&mutation_snapshot).unwrap();
    assert!(
        !mutation_snapshot_text.contains("InjectedFailure"),
        "capture payload should come from repaired plan, not injected draft_v1"
    );

    let validation_snapshot =
        extract_validation_report_snapshot(&all_events, &capture.capsule.mutation_id)
            .expect("expected validation_passed/failed report snapshot for captured mutation_id");
    assert!(
        validation_snapshot
            .get("profile")
            .and_then(Value::as_str)
            .is_some(),
        "validation snapshot should include profile"
    );

    let capsule_reused_event_detected =
        detect_capsule_reused_event(&consumer_events, &capture.capsule.id);
    let promotion_reason_codes = collect_promotion_reason_codes(&consumer_events);
    let imported_sender_ids = consumer_events
        .iter()
        .filter(|record| event_kind(record) == Some("remote_asset_imported"))
        .filter_map(|record| {
            record
                .get("event")
                .and_then(|event| event.get("sender_id"))
                .and_then(Value::as_str)
                .map(|value| value.to_string())
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(capsule_reused_event_detected, second_decision.used_capsule);
    assert!(promotion_reason_codes.contains("downgrade_remote_requires_local_validation"));
    assert!(promotion_reason_codes.contains("promotion_shadow_validation_passed"));
    assert!(promotion_reason_codes.contains("promotion_remote_replay_validated"));
    assert!(imported_sender_ids.contains(&envelope.sender_id));
    assert_eq!(
        first_decision
            .economics_evidence
            .source_sender_id
            .as_deref(),
        Some(envelope.sender_id.as_str())
    );
    assert_eq!(
        second_decision
            .economics_evidence
            .source_sender_id
            .as_deref(),
        Some(envelope.sender_id.as_str())
    );
    append_audit_log(
        &audit_log,
        format!("[STEP] promotion reason_codes={:?}", promotion_reason_codes),
    );

    let success_reuse_verdict = final_reuse_verdict(
        import.accepted,
        import.imported_asset_ids.len(),
        second_decision.used_capsule,
        second_decision.fallback_to_planner,
        capsule_reused_event_detected,
    );
    assert!(success_reuse_verdict);
    let success_repair_reuse_verdict = repair_reuse_verdict(
        initial_failure_detected,
        repair_success,
        import.accepted,
        import.imported_asset_ids.len(),
        second_decision.used_capsule,
        second_decision.fallback_to_planner,
        capsule_reused_event_detected,
    );
    assert!(success_repair_reuse_verdict);

    let consumer_projection = consumer_store.rebuild_projection().unwrap();
    assert!(consumer_projection
        .genes
        .iter()
        .any(|gene| gene.id == capture.gene.id && gene.state == EvoAssetState::Promoted));
    assert!(consumer_projection.capsules.iter().any(
        |capsule| capsule.id == capture.capsule.id && capsule.state == EvoAssetState::Promoted
    ));

    let similar_plan = StubBrain::generate_plan(10, 12000, "reused Beijing-Shanghai experience");
    quality_gate(&similar_plan, 10);
    append_audit_log(&audit_log, "[STEP] similar stub plan quality passed");
    append_realtime_event(
        &realtime_log_path,
        &realtime_jsonl_path,
        realtime_run_id,
        "consumer",
        "finish",
        json!({"stage": "consumer similar task generated", "plan_len": similar_plan.len()}),
    );

    let metrics = consumer_evo.metrics_snapshot().unwrap();
    assert!(metrics.replay_attempts_total >= 1);
    assert!(metrics.replay_success_total >= 1);
    assert!(metrics.reasoning_avoided_tokens_total >= 1);
    let roi_summary = consumer_evo
        .replay_roi_release_gate_summary(24 * 60 * 60)
        .unwrap();
    assert!(roi_summary.replay_attempts_total >= 2);
    assert!(roi_summary.replay_success_total >= 1);
    assert!(roi_summary.reasoning_avoided_tokens_total >= 1);
    let release_gate_contract = consumer_evo
        .replay_roi_release_gate_contract(24 * 60 * 60, ReplayRoiReleaseGateThresholds::default())
        .unwrap();
    assert_eq!(
        release_gate_contract.output.status,
        ReplayRoiReleaseGateStatus::FailClosed
    );
    assert!(
        !release_gate_contract.output.failed_checks.is_empty(),
        "release gate should expose failed checks in demo flow"
    );
    assert_eq!(
        release_gate_contract.input.replay_attempts_total,
        roi_summary.replay_attempts_total
    );
    assert_eq!(
        release_gate_contract.input.replay_success_total,
        roi_summary.replay_success_total
    );
    assert_eq!(
        release_gate_contract.input.replay_failure_total,
        roi_summary.replay_failure_total
    );
    assert_eq!(
        release_gate_contract.input.reasoning_avoided_tokens,
        roi_summary.reasoning_avoided_tokens_total
    );
    assert_eq!(
        release_gate_contract.input.replay_fallback_cost_total,
        roi_summary.replay_fallback_cost_total
    );
    assert_eq!(
        release_gate_contract.input.replay_roi,
        roi_summary.replay_roi
    );
    append_audit_log(
        &audit_log,
        format!(
            "[PASS] metrics replay_attempts_total={} replay_success_total={} replay_roi={:.3}",
            metrics.replay_attempts_total, metrics.replay_success_total, metrics.replay_roi
        ),
    );
    append_audit_log(
        &audit_log,
        format!(
            "[PASS] replay_roi_release_gate_summary attempts={} success={} failure={} avoided_tokens={} fallback_cost={} roi={:.3}",
            roi_summary.replay_attempts_total,
            roi_summary.replay_success_total,
            roi_summary.replay_failure_total,
            roi_summary.reasoning_avoided_tokens_total,
            roi_summary.replay_fallback_cost_total,
            roi_summary.replay_roi
        ),
    );
    append_audit_log(
        &audit_log,
        format!(
            "[STEP] release_gate status={:?} failed_checks={:?}",
            release_gate_contract.output.status, release_gate_contract.output.failed_checks
        ),
    );
    let release_gate_evidence = json!({
        "gate": "self-evolution-release-gate",
        "status": release_gate_contract.output.status,
        "summary": release_gate_contract.output.summary,
        "failed_checks": release_gate_contract.output.failed_checks,
        "evidence_refs": release_gate_contract.output.evidence_refs,
        "window_seconds": release_gate_contract.input.window_seconds,
        "generated_at": release_gate_contract.input.generated_at,
        "thresholds": release_gate_contract.input.thresholds,
        "metrics": {
            "replay_attempts_total": release_gate_contract.input.replay_attempts_total,
            "replay_success_total": release_gate_contract.input.replay_success_total,
            "replay_failure_total": release_gate_contract.input.replay_failure_total,
            "replay_hit_rate": release_gate_contract.input.replay_hit_rate,
            "false_replay_rate": release_gate_contract.input.false_replay_rate,
            "reasoning_avoided_tokens": release_gate_contract.input.reasoning_avoided_tokens,
            "replay_fallback_cost_total": release_gate_contract.input.replay_fallback_cost_total,
            "replay_roi": release_gate_contract.input.replay_roi,
            "replay_safety": release_gate_contract.input.replay_safety
        }
    });
    if let Ok(path) = std::env::var("ORIS_RELEASE_GATE_EVIDENCE_OUT") {
        let out_path = PathBuf::from(path);
        write_json_value(&out_path, &release_gate_evidence);
        append_audit_log(
            &audit_log,
            format!(
                "[PASS] release gate evidence exported path={}",
                out_path.display()
            ),
        );
    }

    let spoofed_revoke =
        consumer_evo.revoke_assets(&oris_runtime::evolution::evolution_network::RevokeNotice {
            sender_id: "agent-spoof".to_string(),
            asset_ids: vec![capture.capsule.id.clone()],
            reason: "spoofed remote revoke".to_string(),
        });
    assert!(spoofed_revoke.is_err());
    let spoofed_revoke_error = spoofed_revoke.unwrap_err().to_string();
    assert!(
        spoofed_revoke_error.contains("owned"),
        "expected fail-closed revoke ownership error, got {spoofed_revoke_error}"
    );
    append_audit_log(
        &audit_log,
        format!("[STEP] spoofed revoke rejected sender=agent-spoof error={spoofed_revoke_error}"),
    );

    let revoke = consumer_evo
        .revoke_assets(&oris_runtime::evolution::evolution_network::RevokeNotice {
            sender_id: envelope.sender_id.clone(),
            asset_ids: vec![capture.capsule.id.clone()],
            reason: "producer revoked remote travel asset".to_string(),
        })
        .unwrap();
    assert_eq!(revoke.sender_id, envelope.sender_id);
    assert!(revoke.asset_ids.contains(&capture.gene.id));
    assert!(revoke.asset_ids.contains(&capture.capsule.id));

    let consumer_projection_after_revoke = consumer_store.rebuild_projection().unwrap();
    assert!(consumer_projection_after_revoke
        .genes
        .iter()
        .any(|gene| gene.id == capture.gene.id && gene.state == EvoAssetState::Revoked));
    assert!(consumer_projection_after_revoke
        .capsules
        .iter()
        .any(|capsule| capsule.id == capture.capsule.id
            && capsule.state == EvoAssetState::Quarantined));

    let consumer_events_after_revoke =
        load_jsonl_values(&consumer_store.root_dir().join("events.jsonl"));
    assert!(consumer_events_after_revoke.iter().any(|record| {
        event_kind(record) == Some("gene_revoked")
            && record
                .get("event")
                .and_then(|event| event.get("gene_id"))
                .and_then(Value::as_str)
                == Some(capture.gene.id.as_str())
            && record
                .get("event")
                .and_then(|event| event.get("reason"))
                .and_then(Value::as_str)
                == Some("producer revoked remote travel asset")
    }));
    append_audit_log(
        &audit_log,
        format!(
            "[PASS] remote revoke sender={} affected_ids={:?}",
            revoke.sender_id, revoke.asset_ids
        ),
    );

    assert!(realtime_log_path.exists());
    assert!(realtime_jsonl_path.exists());
    assert_stub_realtime_logs(&realtime_jsonl_path);
    append_audit_log(
        &audit_log,
        format!(
            "[STEP] realtime logs validated log={} jsonl={}",
            realtime_log_path.display(),
            realtime_jsonl_path.display()
        ),
    );

    let replay_signals = merge_signals(&fixed_signals, &capture.gene.signals);
    let memory_graph_events = build_memory_graph_events(
        realtime_run_id,
        &replay_signals,
        &capture.gene.id,
        &capture.capsule.id,
        second_decision.used_capsule && !second_decision.fallback_to_planner,
        &second_decision.reason,
    );
    let memory_graph_path = unique_path("memory-graph-events").join("memory_graph_events.jsonl");
    std::fs::create_dir_all(memory_graph_path.parent().unwrap()).unwrap();
    write_jsonl_values(&memory_graph_path, &memory_graph_events);
    let loaded_memory_graph_events = load_jsonl_values(&memory_graph_path);
    assert_eq!(loaded_memory_graph_events.len(), 4);
    assert_eq!(
        loaded_memory_graph_events[0]
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "signal"
    );
    assert_eq!(
        loaded_memory_graph_events[1]
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "gene_selected"
    );
    assert_eq!(
        loaded_memory_graph_events[2]
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "outcome"
    );
    assert_eq!(
        loaded_memory_graph_events[3]
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "capsule_created"
    );
    assert_eq!(
        loaded_memory_graph_events[1]
            .get("parent")
            .and_then(Value::as_str),
        loaded_memory_graph_events[0]
            .get("id")
            .and_then(Value::as_str)
    );
    assert_eq!(
        loaded_memory_graph_events[2]
            .get("parent")
            .and_then(Value::as_str),
        loaded_memory_graph_events[1]
            .get("id")
            .and_then(Value::as_str)
    );
    assert_eq!(
        loaded_memory_graph_events[3]
            .get("parent")
            .and_then(Value::as_str),
        loaded_memory_graph_events[2]
            .get("id")
            .and_then(Value::as_str)
    );

    let experience_assets_root = unique_path("experience-assets-repair-flow");
    std::fs::create_dir_all(&experience_assets_root).unwrap();
    std::fs::write(experience_assets_root.join("gene.json"), "{}").unwrap();
    std::fs::write(experience_assets_root.join("capsule.json"), "{}").unwrap();
    std::fs::write(
        experience_assets_root.join("evolution_events.jsonl"),
        "{}\n",
    )
    .unwrap();
    std::fs::write(experience_assets_root.join("mutation.json"), "{}").unwrap();
    std::fs::write(experience_assets_root.join("validation_report.json"), "{}").unwrap();
    std::fs::write(
        experience_assets_root.join("memory_graph_events.jsonl"),
        "{}\n",
    )
    .unwrap();
    std::fs::write(
        experience_assets_root.join("self_repair_trace.json"),
        serde_json::to_string_pretty(&json!({
            "initial_failure_detected": initial_failure_detected,
            "failed_checks": failed_checks,
            "failure_reason": draft_failure_reason,
            "repair_applied": true,
            "repair_success": repair_success,
            "failed_plan_path": "/tmp/failed-plan.md",
            "repaired_plan_path": "/tmp/repaired-plan.md"
        }))
        .unwrap(),
    )
    .unwrap();
    let manifest = build_test_asset_manifest("run-repair-flow", &experience_assets_root);
    assert!(manifest
        .assets
        .get("self_repair_trace")
        .map(|entry| entry.exists)
        .unwrap_or(false));
    assert!(!manifest
        .missing_assets
        .contains(&"self_repair_trace".to_string()));
}

#[tokio::test]
async fn travel_network_delivery_prepares_branch_and_pr_summary() {
    let (_workspace, _store, evo, _sandbox_root, _store_root) =
        build_test_evo("travel-network-delivery-success");
    let request = delivery_request(
        "travel-network-delivery-task",
        "docs/evolution/travel-delivery.md",
        true,
    );
    let outcome = evo
        .run_supervised_devloop(
            &"travel-network-delivery-run".to_string(),
            &request,
            delivery_diff("docs/evolution/travel-delivery.md", "Travel Delivery"),
            None,
        )
        .await
        .unwrap();

    let delivery = evo.prepare_supervised_delivery(&request, &outcome).unwrap();

    assert_eq!(delivery.delivery_status, SupervisedDeliveryStatus::Prepared);
    assert!(delivery.branch_name.is_some());
    assert!(delivery.pr_title.is_some());
    assert!(delivery.pr_summary.is_some());
}

#[tokio::test]
async fn travel_network_delivery_denied_negative_control_is_fail_closed() {
    let (_workspace, _store, evo, _sandbox_root, _store_root) =
        build_test_evo("travel-network-delivery-denied");
    let request = delivery_request(
        "travel-network-delivery-denied-task",
        "docs/evolution/travel-delivery-denied.md",
        true,
    );
    let outcome = SupervisedDevloopOutcome {
        task_id: request.task.id.clone(),
        task_class: None,
        status: SupervisedDevloopStatus::Executed,
        execution_feedback: None,
        failure_contract: None,
        summary: "missing delivery evidence".to_string(),
    };

    let delivery = evo.prepare_supervised_delivery(&request, &outcome).unwrap();

    assert_eq!(delivery.delivery_status, SupervisedDeliveryStatus::Denied);
    assert_eq!(
        delivery.reason_code,
        SupervisedDeliveryReasonCode::UnsupportedTaskScope
    );
    assert!(delivery.fail_closed);
}

#[tokio::test]
async fn latest_store_upgrade_migrates_history_and_is_idempotent() {
    let audit_log =
        create_audit_log_path("latest_store_upgrade_migrates_history_and_is_idempotent");
    let demo_runs_root = unique_path("demo-runs-root");
    let current_run_id = "run-current";
    std::fs::create_dir_all(demo_runs_root.join(current_run_id)).unwrap();
    std::fs::create_dir_all(demo_runs_root.join("run-empty/producer-store")).unwrap();
    std::fs::create_dir_all(demo_runs_root.join("run-empty/consumer-store")).unwrap();

    let run_a_producer_store = demo_runs_root.join("run-a/producer-store");
    let run_b_producer_store = demo_runs_root.join("run-b/producer-store");
    std::fs::create_dir_all(&run_a_producer_store).unwrap();
    std::fs::create_dir_all(&run_b_producer_store).unwrap();

    let (_workspace_a, producer_store_a, producer_evo_a, _sandbox_a, _store_root_a) =
        build_test_evo("migrate-a");
    let (_workspace_b, producer_store_b, producer_evo_b, _sandbox_b, _store_root_b) =
        build_test_evo("migrate-b");

    let target_path = "docs/evolution/travel-beijing-shanghai-experience.md";
    let proposal = MutationProposal {
        intent: "capture reusable travel planning experience".into(),
        files: vec![
            target_path.to_string(),
            "travel_planning".to_string(),
            "route:beijing-shanghai".to_string(),
        ],
        expected_effect: "promote reusable experience".into(),
    };
    let history_plan = StubBrain::generate_plan(7, 8000, "history");
    append_audit_log(
        &audit_log,
        format!(
            "[STEP] prepare history stores root={} current_run={}",
            demo_runs_root.display(),
            current_run_id
        ),
    );
    producer_evo_a
        .capture_from_proposal(
            &"travel-migrate-a".to_string(),
            &proposal,
            experience_diff(target_path, &preview(&history_plan, 220)),
            None,
        )
        .await
        .unwrap();
    producer_evo_b
        .capture_from_proposal(
            &"travel-migrate-b".to_string(),
            &proposal,
            experience_diff(target_path, &preview(&history_plan, 180)),
            None,
        )
        .await
        .unwrap();
    append_audit_log(&audit_log, "[STEP] produced two history captures");

    std::fs::copy(
        producer_store_a.root_dir().join("events.jsonl"),
        run_a_producer_store.join("events.jsonl"),
    )
    .unwrap();
    std::fs::copy(
        producer_store_b.root_dir().join("events.jsonl"),
        run_b_producer_store.join("events.jsonl"),
    )
    .unwrap();

    let latest_store = Arc::new(JsonlEvolutionStore::new(unique_path("latest-producer")));
    let latest_node = EvolutionNetworkNode::new(latest_store.clone() as Arc<dyn EvolutionStore>);
    let discovered =
        discover_historical_store_dirs(&demo_runs_root, current_run_id, "producer-store");
    assert_eq!(discovered.len(), 2);
    append_audit_log(
        &audit_log,
        format!("[STEP] discovered historical stores={}", discovered.len()),
    );

    let first_imported = migrate_sources_to_latest(&discovered, &latest_node, "migrate");
    assert!(first_imported > 0);
    let second_imported = migrate_sources_to_latest(&discovered, &latest_node, "migrate");
    assert_eq!(second_imported, 0);
    append_audit_log(
        &audit_log,
        format!(
            "[PASS] migrate idempotent first_imported={} second_imported={}",
            first_imported, second_imported
        ),
    );
}

#[test]
fn four_gate_solidification_only_runs_when_all_conditions_pass() {
    let audit_log =
        create_audit_log_path("four_gate_solidification_only_runs_when_all_conditions_pass");
    let latest_consumer_store = Arc::new(JsonlEvolutionStore::new(unique_path("latest-consumer")));
    let latest_consumer_node =
        EvolutionNetworkNode::new(latest_consumer_store.clone() as Arc<dyn EvolutionStore>);

    let gate_failed = build_gate_matrix(true, true, false, true);
    assert!(!gate_failed);
    let before_failed = count_reported_promoted_genes(&latest_consumer_store);
    let failed_gene = solidify_if_gate_passed(
        &latest_consumer_node,
        gate_failed,
        "run-failed",
        "capsule-failed",
        "gene-failed",
    );
    assert!(failed_gene.is_none());
    let after_failed = count_reported_promoted_genes(&latest_consumer_store);
    assert_eq!(before_failed, after_failed);
    append_audit_log(
        &audit_log,
        format!(
            "[STEP] failed gate no solidify before={} after={}",
            before_failed, after_failed
        ),
    );

    let gate_passed = build_gate_matrix(true, true, true, true);
    assert!(gate_passed);
    let before_passed = count_reported_promoted_genes(&latest_consumer_store);
    let passed_gene = solidify_if_gate_passed(
        &latest_consumer_node,
        gate_passed,
        "run-passed",
        "capsule-passed",
        "gene-passed",
    );
    assert!(passed_gene.is_some());
    let after_passed = count_reported_promoted_genes(&latest_consumer_store);
    assert_eq!(after_passed, before_passed + 1);
    append_audit_log(
        &audit_log,
        format!(
            "[STEP] passed gate solidified before={} after={} gene={}",
            before_passed,
            after_passed,
            passed_gene.as_deref().unwrap_or("N/A")
        ),
    );

    let fetch = latest_consumer_node
        .fetch_assets(
            "agent-consumer",
            &FetchQuery {
                sender_id: "agent-consumer".to_string(),
                signals: vec!["travel.longline.beijing-shanghai".to_string()],
                since_cursor: None,
                resume_token: None,
            },
        )
        .unwrap();
    assert!(!fetch.assets.is_empty());

    let published = latest_consumer_node
        .publish_local_assets("agent-consumer")
        .unwrap();
    assert!(!published.assets.is_empty());
    append_audit_log(
        &audit_log,
        format!(
            "[PASS] fetch_assets={} published_assets={}",
            fetch.assets.len(),
            published.assets.len()
        ),
    );
}

#[test]
fn final_reuse_verdict_is_false_for_fallback_path() {
    let verdict = final_reuse_verdict(true, 2, false, true, false);
    assert!(!verdict);
    let repair_verdict = repair_reuse_verdict(true, true, true, 2, false, true, false);
    assert!(!repair_verdict);
}

#[test]
fn asset_manifest_marks_missing_assets_without_panicking() {
    let asset_root = unique_path("experience-assets-manifest");
    std::fs::create_dir_all(&asset_root).unwrap();
    std::fs::write(asset_root.join("gene.json"), "{}").unwrap();
    std::fs::write(asset_root.join("capsule.json"), "{}").unwrap();
    std::fs::write(asset_root.join("evolution_events.jsonl"), "{}\n").unwrap();
    std::fs::write(asset_root.join("memory_graph_events.jsonl"), "{}\n").unwrap();
    // Keep mutation/validation_report absent on purpose to validate missing_assets behavior.

    let manifest = build_test_asset_manifest("run-test", &asset_root);
    assert_eq!(manifest.assets.len(), 7);
    assert!(manifest
        .assets
        .get("gene")
        .map(|entry| entry.exists)
        .unwrap_or(false));
    assert!(!manifest
        .assets
        .get("mutation")
        .map(|entry| entry.exists)
        .unwrap_or(true));
    assert!(manifest.missing_assets.contains(&"mutation".to_string()));
    assert!(manifest
        .missing_assets
        .contains(&"validation_report".to_string()));
    assert!(manifest
        .missing_assets
        .contains(&"self_repair_trace".to_string()));

    let manifest_path = asset_root.join("asset_manifest.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let round_trip: TestAssetManifest =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    assert_eq!(round_trip.assets.len(), 7);
    assert_eq!(round_trip.missing_assets, manifest.missing_assets);
}
