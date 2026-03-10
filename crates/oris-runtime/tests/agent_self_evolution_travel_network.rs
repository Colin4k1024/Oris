#![cfg(feature = "full-evolution-experimental")]

use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use oris_runtime::agent_contract::MutationProposal;
use oris_runtime::evolution::{
    CommandValidator, EvoAssetState, EvoEvolutionStore as EvolutionStore, EvoKernel,
    EvoSandboxPolicy as SandboxPolicy, EvoSelectorInput as SelectorInput, EvolutionNetworkNode,
    FetchQuery, JsonlEvolutionStore, LocalProcessSandbox, PublishRequest, ValidationPlan,
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

fn quality_gate(plan: &str, days: u32) {
    assert!(plan.contains("北京"));
    assert!(plan.contains("上海"));
    assert!(plan.contains("交通方案") || plan.contains("交通"));
    assert!(plan.contains("日程表") || plan.contains("行程"));
    assert!(plan.contains("住宿建议") || plan.contains("住宿"));
    assert!(plan.contains("预算拆分") || plan.contains("预算"));
    assert!(plan.contains("风险") && plan.contains("备选"));
    assert!(plan.contains("第1天") || plan.to_ascii_lowercase().contains("day 1"));
    let last_day = format!("第{}天", days);
    assert!(plan.contains(&last_day) || plan.to_ascii_lowercase().contains(&format!("day {days}")));
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
        match value.get("event").and_then(Value::as_str).unwrap_or_default() {
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

fn experience_diff(path: &str, plan_preview: &str) -> String {
    let signal_block = FIXED_SIGNAL_TAGS.join(", ");
    format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,12 @@\n+# Beijing Shanghai Longline Experience\n+\n+signals: {signal_block}\n+\n+strategy:\n+- call deterministic tools first\n+- then generate final integrated plan\n+\n+preview:\n+{plan_preview}\n",
        path = path,
        signal_block = signal_block,
        plan_preview = plan_preview.replace('\n', "\n+"),
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

#[tokio::test]
async fn travel_network_demo_flow_captures_publishes_imports_and_replays() {
    let audit_log =
        create_audit_log_path("travel_network_demo_flow_captures_publishes_imports_and_replays");
    let (realtime_log_path, realtime_jsonl_path) =
        create_realtime_log_paths("travel_network_demo_flow_captures_publishes_imports_and_replays");
    let realtime_run_id = "stub-travel-reuse";
    append_realtime_event(
        &realtime_log_path,
        &realtime_jsonl_path,
        realtime_run_id,
        "bootstrap",
        "phase_transition",
        json!({"stage": "bootstrap"}),
    );
    let producer_plan = StubBrain::generate_plan(7, 8000, "none");
    quality_gate(&producer_plan, 7);
    append_audit_log(&audit_log, "[STEP] producer stub plan quality passed");
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
    let decision = consumer_evo
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
            "stage": "[5] replay",
            "used_capsule": decision.used_capsule,
            "fallback": decision.fallback_to_planner
        }),
    );

    assert!(decision.used_capsule);
    assert!(!decision.fallback_to_planner);
    append_audit_log(
        &audit_log,
        format!(
            "[STEP] replay used_capsule={} fallback={} reason={}",
            decision.used_capsule, decision.fallback_to_planner, decision.reason
        ),
    );

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
    append_audit_log(
        &audit_log,
        format!(
            "[PASS] metrics replay_attempts_total={} replay_success_total={}",
            metrics.replay_attempts_total, metrics.replay_success_total
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
