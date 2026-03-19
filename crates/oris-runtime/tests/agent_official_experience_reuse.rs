#![cfg(feature = "full-evolution-experimental")]

use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use oris_runtime::agent::middleware::{Middleware, MiddlewareContext, MiddlewareError};
use oris_runtime::agent::{create_agent_from_llm, UnifiedAgent};
use oris_runtime::agent_contract::{
    ReplayFallbackNextAction, ReplayFallbackReasonCode, ReplayFeedback, ReplayPlannerDirective,
};
use oris_runtime::error::ToolError;
use oris_runtime::evolution::{
    evaluate_repair_quality_gate, CommandValidator, EvoEvolutionStore as EvolutionStore, EvoKernel,
    EvoSandboxPolicy as SandboxPolicy, EvoSelectorInput as SelectorInput, EvolutionNetworkNode,
    FetchQuery, JsonlEvolutionStore, LocalProcessSandbox, ValidationPlan,
};
use oris_runtime::governor::{DefaultGovernor, GovernorConfig};
use oris_runtime::kernel::{
    AllowAllPolicy, InMemoryEventStore, Kernel, KernelMode, KernelState, NoopActionExecutor,
    NoopStepFn, StateUpdatedOnlyReducer,
};
use oris_runtime::language_models::{options::CallOptions, GenerateResult};
use oris_runtime::llm::Qwen;
use oris_runtime::prompt::PromptArgs;
use oris_runtime::schemas::agent::{AgentAction, AgentEvent, AgentFinish};
use oris_runtime::schemas::messages::Message;
use oris_runtime::tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct TestState;

impl KernelState for TestState {
    fn version(&self) -> u32 {
        1
    }
}

struct IncidentTool;

#[async_trait]
impl Tool for IncidentTool {
    fn name(&self) -> String {
        "official_error_incidents".to_string()
    }

    fn description(&self) -> String {
        "Deterministic official-aligned incident snippets.".to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "case_id": {"type": "string"}
            },
            "required": ["case_id"]
        })
    }

    async fn run(&self, input: Value) -> Result<String, ToolError> {
        let case_id = input
            .get("case_id")
            .and_then(Value::as_str)
            .unwrap_or("case-a");
        let payload = if case_id == "case-b" {
            json!({
                "case_id": case_id,
                "error": "unknown command 'proccess'",
                "signals": ["error", "failed", "unstable", "windows_shell_incompatible"]
            })
        } else {
            json!({
                "case_id": case_id,
                "error": "unknown command 'process'",
                "signals": ["error", "failed", "unstable", "log_error"]
            })
        };
        serde_json::to_string_pretty(&payload).map_err(|e| ToolError::ExecutionError(e.to_string()))
    }
}

struct ChecklistTool;

#[async_trait]
impl Tool for ChecklistTool {
    fn name(&self) -> String {
        "repair_verification_checklist".to_string()
    }

    fn description(&self) -> String {
        "Deterministic verification/rollback checklist.".to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "scope": {"type": "string"}
            },
            "required": ["scope"]
        })
    }

    async fn run(&self, _input: Value) -> Result<String, ToolError> {
        let payload = json!({
            "verify": ["cargo --version", "cargo check -p oris-runtime"],
            "rollback": ["revert entrypoint", "disable command alias"]
        });
        serde_json::to_string_pretty(&payload).map_err(|e| ToolError::ExecutionError(e.to_string()))
    }
}

#[derive(Clone)]
struct RealtimeLogger {
    run_id: String,
    human_path: PathBuf,
    jsonl_path: PathBuf,
    human_file: Arc<Mutex<std::fs::File>>,
    jsonl_file: Arc<Mutex<std::fs::File>>,
}

impl RealtimeLogger {
    fn new(run_id: &str, run_root: &Path) -> Self {
        std::fs::create_dir_all(run_root).unwrap();
        let human_path = run_root.join("agent_realtime.log");
        let jsonl_path = run_root.join("agent_realtime.jsonl");
        let human_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&human_path)
            .unwrap();
        let jsonl_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&jsonl_path)
            .unwrap();
        Self {
            run_id: run_id.to_string(),
            human_path,
            jsonl_path,
            human_file: Arc::new(Mutex::new(human_file)),
            jsonl_file: Arc::new(Mutex::new(jsonl_file)),
        }
    }

    fn human_path(&self) -> &Path {
        &self.human_path
    }

    fn jsonl_path(&self) -> &Path {
        &self.jsonl_path
    }

    fn log_event(&self, role: &str, phase: &str, event: &str, payload: Value) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .to_string();
        let record = json!({
            "ts": ts,
            "run_id": self.run_id,
            "agent_role": role,
            "phase": phase,
            "event": event,
            "payload": payload,
        });
        let line = format!(
            "[{}] run_id={} role={} phase={} event={} payload={}",
            ts, self.run_id, role, phase, event, record["payload"]
        );
        {
            let mut human = self.human_file.lock().unwrap();
            writeln!(human, "{line}").unwrap();
        }
        {
            let mut jsonl = self.jsonl_file.lock().unwrap();
            writeln!(jsonl, "{}", serde_json::to_string(&record).unwrap()).unwrap();
        }
    }

    fn log_token_chunk(&self, role: &str, chunk: &str) {
        self.log_event(
            role,
            "llm_stream",
            "token_chunk",
            json!({
                "chunk_len": chunk.chars().count(),
                "chunk_preview": chunk.chars().take(120).collect::<String>(),
            }),
        );
    }
}

#[derive(Clone)]
struct RealtimeMiddleware {
    logger: RealtimeLogger,
    role: String,
}

impl RealtimeMiddleware {
    fn new(logger: RealtimeLogger, role: &str) -> Self {
        Self {
            logger,
            role: role.to_string(),
        }
    }

    fn should_log_once(context: &mut MiddlewareContext, key: String) -> bool {
        if context.get_custom_data(&key).is_some() {
            return false;
        }
        context.set_custom_data(key, json!(true));
        true
    }
}

#[async_trait]
impl Middleware for RealtimeMiddleware {
    async fn before_agent_plan(
        &self,
        input: &PromptArgs,
        steps: &[(AgentAction, String)],
        context: &mut MiddlewareContext,
    ) -> Result<Option<PromptArgs>, MiddlewareError> {
        if !Self::should_log_once(context, format!("rt-before-plan-{}", context.iteration)) {
            return Ok(None);
        }
        self.logger.log_event(
            &self.role,
            "agent_plan",
            "before_agent_plan",
            json!({
                "iteration": context.iteration,
                "steps": steps.len(),
                "input_keys": input.keys().collect::<Vec<_>>(),
            }),
        );
        Ok(None)
    }

    async fn after_agent_plan(
        &self,
        _input: &PromptArgs,
        event: &AgentEvent,
        context: &mut MiddlewareContext,
    ) -> Result<Option<AgentEvent>, MiddlewareError> {
        let dedupe_event_tag = match event {
            AgentEvent::Action(actions) => format!("action-{}", actions.len()),
            AgentEvent::Finish(_) => "finish".to_string(),
        };
        if !Self::should_log_once(
            context,
            format!("rt-after-plan-{}-{}", context.iteration, dedupe_event_tag),
        ) {
            return Ok(None);
        }
        let payload = match event {
            AgentEvent::Action(actions) => json!({
                "iteration": context.iteration,
                "tools": actions.iter().map(|action| action.tool.clone()).collect::<Vec<_>>(),
            }),
            AgentEvent::Finish(finish) => json!({
                "iteration": context.iteration,
                "finish_preview": finish.output.chars().take(180).collect::<String>(),
            }),
        };
        self.logger
            .log_event(&self.role, "agent_plan", "after_agent_plan", payload);
        Ok(None)
    }

    async fn before_tool_call(
        &self,
        action: &AgentAction,
        context: &mut MiddlewareContext,
    ) -> Result<Option<AgentAction>, MiddlewareError> {
        if !Self::should_log_once(
            context,
            format!(
                "rt-before-tool-{}-{}-{}",
                context.iteration, action.tool, action.tool_input
            ),
        ) {
            return Ok(None);
        }
        self.logger.log_event(
            &self.role,
            "tool",
            "tool_call_before",
            json!({
                "iteration": context.iteration,
                "tool": action.tool,
                "tool_input_summary": action.tool_input.chars().take(180).collect::<String>(),
            }),
        );
        Ok(None)
    }

    async fn after_tool_call(
        &self,
        action: &AgentAction,
        observation: &str,
        context: &mut MiddlewareContext,
    ) -> Result<Option<String>, MiddlewareError> {
        if !Self::should_log_once(
            context,
            format!(
                "rt-after-tool-{}-{}-{}",
                context.iteration,
                action.tool,
                observation.len()
            ),
        ) {
            return Ok(None);
        }
        self.logger.log_event(
            &self.role,
            "tool",
            "tool_call_after",
            json!({
                "iteration": context.iteration,
                "tool": action.tool,
                "observation_len": observation.chars().count(),
            }),
        );
        Ok(None)
    }

    async fn after_finish(
        &self,
        finish: &AgentFinish,
        result: &GenerateResult,
        context: &mut MiddlewareContext,
    ) -> Result<(), MiddlewareError> {
        if !Self::should_log_once(
            context,
            format!(
                "rt-after-finish-{}-{}",
                context.iteration,
                finish.output.len()
            ),
        ) {
            return Ok(());
        }
        self.logger.log_event(
            &self.role,
            "finish",
            "finish",
            json!({
                "iteration": context.iteration,
                "output_preview": finish.output.chars().take(180).collect::<String>(),
                "result_preview": result.generation.chars().take(180).collect::<String>(),
            }),
        );
        Ok(())
    }
}

fn make_agent(prompt: &str, logger: RealtimeLogger) -> UnifiedAgent {
    let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(IncidentTool), Arc::new(ChecklistTool)];
    let stream_logger = logger.clone();
    let callback = move |chunk: String| {
        let stream_logger = stream_logger.clone();
        async move {
            stream_logger.log_token_chunk("worker-agent", &chunk);
            Ok(())
        }
    };
    let llm = Qwen::new()
        .with_api_key(std::env::var("QWEN_API_KEY").unwrap_or_default())
        .with_model("qwen3-max")
        .with_options(
            CallOptions::default()
                .with_temperature(0.4)
                .with_max_tokens(1600)
                .with_streaming_func(callback),
        );
    create_agent_from_llm(llm, &tools, Some(prompt))
        .expect("create qwen agent from llm")
        .with_middleware(vec![Arc::new(RealtimeMiddleware::new(
            logger,
            "worker-agent",
        ))])
        .with_max_iterations(12)
        .with_break_if_error(true)
}

fn unique_path(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("oris-official-reuse-test-{label}-{nanos}"))
}

fn create_audit_log_path(test_name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::current_dir()
        .unwrap()
        .join("target/test-audit/agent_official_experience_reuse");
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

fn create_fallback_snapshot_path(case_id: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::current_dir()
        .unwrap()
        .join("target/test-audit/agent_official_experience_reuse/fallback-negative-controls");
    std::fs::create_dir_all(&root).unwrap();
    root.join(format!("{case_id}-{nanos}.json"))
}

fn write_fallback_snapshot(case_id: &str, payload: &Value) -> PathBuf {
    let path = create_fallback_snapshot_path(case_id);
    std::fs::write(&path, serde_json::to_string_pretty(payload).unwrap()).unwrap();
    path
}

fn setup_workspace(path: &Path) {
    if path.exists() {
        let _ = std::fs::remove_dir_all(path);
    }
    std::fs::create_dir_all(path.join("docs/evolution")).unwrap();
    std::fs::write(path.join("README.md"), "# Test workspace\n").unwrap();
    let _ = std::process::Command::new("git")
        .arg("init")
        .current_dir(path)
        .output();
}

fn policy() -> SandboxPolicy {
    SandboxPolicy {
        allowed_programs: vec!["cargo".into(), "git".into()],
        max_duration_ms: 30_000,
        max_output_bytes: 1_048_576,
        denied_env_prefixes: vec!["TOKEN".into(), "KEY".into(), "SECRET".into()],
        max_memory_bytes: None,
        max_cpu_secs: None,
        use_process_group: false,
    }
}

fn validation_plan() -> ValidationPlan {
    ValidationPlan {
        profile: "official-reuse-test".into(),
        stages: vec![],
    }
}

fn build_evo(
    label: &str,
    workspace_root: &Path,
    sandbox_root: &Path,
    store_root: &Path,
) -> (EvoKernel<TestState>, Arc<JsonlEvolutionStore>) {
    setup_workspace(workspace_root);
    let _ = std::fs::remove_dir_all(sandbox_root);
    let _ = std::fs::remove_dir_all(store_root);
    std::fs::create_dir_all(sandbox_root).unwrap();
    std::fs::create_dir_all(store_root).unwrap();

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

    let store = Arc::new(JsonlEvolutionStore::new(store_root.to_path_buf()));
    let evo = EvoKernel::new(
        kernel,
        Arc::new(LocalProcessSandbox::new(
            format!("run-{label}"),
            workspace_root,
            sandbox_root,
        )),
        Arc::new(CommandValidator::new(policy())),
        store.clone() as Arc<dyn EvolutionStore>,
    )
    .with_governor(Arc::new(DefaultGovernor::new(GovernorConfig {
        promote_after_successes: 1,
        ..Default::default()
    })))
    .with_sandbox_policy(policy())
    .with_validation_plan(validation_plan());

    (evo, store)
}

fn quality_gate(plan: &str) {
    let report = evaluate_repair_quality_gate(plan);
    let preview = plan.chars().take(240).collect::<String>();

    assert!(
        report.incident_anchor,
        "quality_gate missing incident anchor; preview={preview}"
    );
    assert!(
        report.structure_score >= 3,
        "quality_gate structure too weak (score={}); root={} fix={} verification={} rollback={}; preview={preview}",
        report.structure_score,
        report.root_cause,
        report.fix,
        report.verification,
        report.rollback
    );
    assert!(
        report.has_actionable_command || report.verification,
        "quality_gate missing actionable verification command; preview={preview}"
    );
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DirectiveExecutionRoute {
    ReuseWithoutPlanner,
    PlanFromScratch,
    ValidateSignalsThenPlan,
    RebuildCapsule,
    RegenerateMutationPayload,
    RebasePatchAndRetry,
    RepairAndRevalidate,
    UnsupportedDirective,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AgentDirectiveExecution {
    route: DirectiveExecutionRoute,
    repair_hint: Option<String>,
    verification_hint: String,
    fallback_classification: Option<String>,
}

fn consume_replay_directive(feedback: &ReplayFeedback) -> AgentDirectiveExecution {
    match feedback.planner_directive {
        ReplayPlannerDirective::SkipPlanner => AgentDirectiveExecution {
            route: DirectiveExecutionRoute::ReuseWithoutPlanner,
            repair_hint: None,
            verification_hint: "Run checklist tool to verify reused capsule impact.".to_string(),
            fallback_classification: None,
        },
        ReplayPlannerDirective::PlanFallback => match feedback.next_action {
            Some(ReplayFallbackNextAction::PlanFromScratch) => AgentDirectiveExecution {
                route: DirectiveExecutionRoute::PlanFromScratch,
                repair_hint: feedback.repair_hint.clone(),
                verification_hint: "Generate a minimal patch and run verification checklist."
                    .to_string(),
                fallback_classification: None,
            },
            Some(ReplayFallbackNextAction::ValidateSignalsThenPlan) => AgentDirectiveExecution {
                route: DirectiveExecutionRoute::ValidateSignalsThenPlan,
                repair_hint: feedback.repair_hint.clone(),
                verification_hint: "Validate incident signals before generating a patch."
                    .to_string(),
                fallback_classification: None,
            },
            Some(ReplayFallbackNextAction::RebuildCapsule) => AgentDirectiveExecution {
                route: DirectiveExecutionRoute::RebuildCapsule,
                repair_hint: feedback.repair_hint.clone(),
                verification_hint: "Rebuild capsule payload and validate replay boundary."
                    .to_string(),
                fallback_classification: None,
            },
            Some(ReplayFallbackNextAction::RegenerateMutationPayload) => AgentDirectiveExecution {
                route: DirectiveExecutionRoute::RegenerateMutationPayload,
                repair_hint: feedback.repair_hint.clone(),
                verification_hint: "Regenerate mutation payload then validate with checklist tool."
                    .to_string(),
                fallback_classification: None,
            },
            Some(ReplayFallbackNextAction::RebasePatchAndRetry) => AgentDirectiveExecution {
                route: DirectiveExecutionRoute::RebasePatchAndRetry,
                repair_hint: feedback.repair_hint.clone(),
                verification_hint: "Rebase patch, retry replay, and validate regression checks."
                    .to_string(),
                fallback_classification: None,
            },
            Some(ReplayFallbackNextAction::RepairAndRevalidate) => AgentDirectiveExecution {
                route: DirectiveExecutionRoute::RepairAndRevalidate,
                repair_hint: feedback.repair_hint.clone(),
                verification_hint:
                    "Produce repair mutation and run validation checklist before release."
                        .to_string(),
                fallback_classification: None,
            },
            Some(ReplayFallbackNextAction::EscalateFailClosed) | None => AgentDirectiveExecution {
                route: DirectiveExecutionRoute::UnsupportedDirective,
                repair_hint: feedback.repair_hint.clone(),
                verification_hint:
                    "Directive is not executable; force fail-closed fallback planning.".to_string(),
                fallback_classification: Some(
                    "directive_unexecutable_missing_or_escalated_next_action".to_string(),
                ),
            },
        },
    }
}

fn directive_guided_prompt(case_text: &str, execution: &AgentDirectiveExecution) -> String {
    let mut prompt = format!("{}。", case_text.trim());
    if let Some(repair_hint) = execution.repair_hint.as_deref() {
        prompt.push_str(&format!(" 优先执行 repair_hint: `{repair_hint}`。"));
    }
    prompt.push_str(&format!(
        " verification_hint: `{}`。",
        execution.verification_hint
    ));
    if let Some(classification) = execution.fallback_classification.as_deref() {
        prompt.push_str(&format!(
            " 当前指令不可执行，按 `{classification}` 走 fail-closed 回退。"
        ));
    }
    prompt
}

#[test]
fn quality_gate_accepts_semantic_variants() {
    let plan = r#"
根本原因：脚本中拼写错误导致 unknown command 'process'。
修复建议：将 `proccess` 更正为 `process`，并统一命令入口。
验证方式：执行 `cargo check -p oris-runtime` 与回归测试。
恢复方案：若新入口异常，立即回滚到旧命令映射。
"#;
    quality_gate(plan);
}

#[test]
#[should_panic(expected = "quality_gate missing incident anchor")]
fn quality_gate_rejects_missing_incident_context() {
    let plan = r#"
原因分析：逻辑分支覆盖不足。
修复方案：补充分支与日志。
验证命令：cargo check -p oris-runtime
回滚方案：git revert HEAD
"#;
    quality_gate(plan);
}

#[test]
fn directive_consumer_maps_fallback_actions_to_execution_routes() {
    let feedback = ReplayFeedback {
        used_capsule: false,
        capsule_id: None,
        planner_directive: ReplayPlannerDirective::PlanFallback,
        reasoning_steps_avoided: 0,
        fallback_reason: Some("replay validation failed".to_string()),
        reason_code: Some(ReplayFallbackReasonCode::ValidationFailed),
        repair_hint: Some("produce a repair mutation and rerun validation".to_string()),
        next_action: Some(ReplayFallbackNextAction::RepairAndRevalidate),
        confidence: Some(64),
        task_class_id: "build.fix".to_string(),
        task_label: "Build fix".to_string(),
        summary: "fallback".to_string(),
    };
    let execution = consume_replay_directive(&feedback);
    assert_eq!(
        execution.route,
        DirectiveExecutionRoute::RepairAndRevalidate
    );
    assert!(execution.fallback_classification.is_none());
    assert!(execution.verification_hint.contains("validation"));
}

#[test]
fn directive_consumer_marks_unexecutable_directive_with_fail_closed_classification() {
    let feedback = ReplayFeedback {
        used_capsule: false,
        capsule_id: None,
        planner_directive: ReplayPlannerDirective::PlanFallback,
        reasoning_steps_avoided: 0,
        fallback_reason: Some("unmapped replay fallback reason".to_string()),
        reason_code: Some(ReplayFallbackReasonCode::UnmappedFallbackReason),
        repair_hint: Some("manual intervention required".to_string()),
        next_action: None,
        confidence: Some(0),
        task_class_id: "unknown".to_string(),
        task_label: "Unknown".to_string(),
        summary: "fallback".to_string(),
    };
    let execution = consume_replay_directive(&feedback);
    assert_eq!(
        execution.route,
        DirectiveExecutionRoute::UnsupportedDirective
    );
    assert_eq!(
        execution.fallback_classification.as_deref(),
        Some("directive_unexecutable_missing_or_escalated_next_action")
    );
    let prompt = directive_guided_prompt("处理未知回退", &execution);
    assert!(prompt.contains("fail-closed"));
}

fn assert_negative_control_case(
    case_id: &str,
    reason_code: ReplayFallbackReasonCode,
    next_action: Option<ReplayFallbackNextAction>,
    expected_route: DirectiveExecutionRoute,
    expected_classification: Option<&str>,
) {
    let feedback = ReplayFeedback {
        used_capsule: false,
        capsule_id: None,
        planner_directive: ReplayPlannerDirective::PlanFallback,
        reasoning_steps_avoided: 0,
        fallback_reason: Some(format!("negative-control-{case_id}")),
        reason_code: Some(reason_code.clone()),
        repair_hint: Some("negative control repair hint".to_string()),
        next_action: next_action.clone(),
        confidence: Some(0),
        task_class_id: format!("negative.{case_id}"),
        task_label: format!("Negative {case_id}"),
        summary: "negative-control".to_string(),
    };
    let execution = consume_replay_directive(&feedback);
    assert_eq!(execution.route, expected_route);
    assert_eq!(
        execution.fallback_classification.as_deref(),
        expected_classification
    );

    let expected_next_action = next_action.map(|action| format!("{:?}", action));
    let audit_payload = json!({
        "case_id": case_id,
        "reason_code": format!("{:?}", reason_code),
        "planner_directive": format!("{:?}", feedback.planner_directive),
        "next_action": expected_next_action,
        "directive_route": format!("{:?}", execution.route),
        "fallback_classification": execution.fallback_classification,
    });
    let snapshot_path = write_fallback_snapshot(case_id, &audit_payload);
    assert!(snapshot_path.exists());

    let loaded: Value = serde_json::from_str(&std::fs::read_to_string(&snapshot_path).unwrap())
        .expect("snapshot json");
    assert_eq!(loaded["reason_code"], format!("{:?}", reason_code));
    assert_eq!(loaded["planner_directive"], "PlanFallback");
    assert_eq!(loaded["directive_route"], format!("{:?}", expected_route));
}

#[test]
fn directive_negative_control_replay_miss_keeps_reason_code_directive_and_audit_consistent() {
    assert_negative_control_case(
        "replay-miss",
        ReplayFallbackReasonCode::NoCandidateAfterSelect,
        Some(ReplayFallbackNextAction::ValidateSignalsThenPlan),
        DirectiveExecutionRoute::ValidateSignalsThenPlan,
        None,
    );
}

#[test]
fn directive_negative_control_replay_failure_keeps_reason_code_directive_and_audit_consistent() {
    assert_negative_control_case(
        "replay-failure",
        ReplayFallbackReasonCode::ValidationFailed,
        Some(ReplayFallbackNextAction::RepairAndRevalidate),
        DirectiveExecutionRoute::RepairAndRevalidate,
        None,
    );
}

#[test]
fn directive_negative_control_unknown_reason_keeps_reason_code_directive_and_audit_consistent() {
    assert_negative_control_case(
        "unknown-reason",
        ReplayFallbackReasonCode::UnmappedFallbackReason,
        None,
        DirectiveExecutionRoute::UnsupportedDirective,
        Some("directive_unexecutable_missing_or_escalated_next_action"),
    );
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

fn summarize(path: &Path) -> Value {
    let mut counts = BTreeMap::<String, usize>::new();
    let mut key_events = Vec::new();
    for (idx, line) in std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .enumerate()
    {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line).unwrap();
        let kind = value
            .get("event")
            .and_then(|event| event.get("kind"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        *counts.entry(kind.clone()).or_insert(0) += 1;
        if matches!(
            kind.as_str(),
            "remote_asset_imported"
                | "mutation_declared"
                | "capsule_reused"
                | "capsule_released"
                | "promotion_evaluated"
        ) {
            key_events.push(format!("seq_line={} {}", idx + 1, line));
        }
    }

    json!({
        "counts": counts,
        "key_events": key_events,
    })
}

fn assert_realtime_events(path: &Path) {
    let mut phase_transition = 0usize;
    let mut tool_events = 0usize;
    let mut token_events = 0usize;
    let mut finish_events = 0usize;

    for line in std::fs::read_to_string(path)
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
            "phase_transition" => phase_transition += 1,
            "tool_call_before" | "tool_call_after" => tool_events += 1,
            "token_chunk" => token_events += 1,
            "finish" => finish_events += 1,
            _ => {}
        }
    }

    assert!(phase_transition > 0, "missing phase_transition event");
    assert!(tool_events > 0, "missing tool_call_* events");
    assert!(token_events > 0, "missing token_chunk events");
    assert!(finish_events > 0, "missing finish events");
}

#[tokio::test]
async fn official_experience_reuse_with_real_qwen() {
    let audit_log = create_audit_log_path("official_experience_reuse_with_real_qwen");
    let _key = match std::env::var("QWEN_API_KEY") {
        Ok(raw) if !raw.trim().is_empty() => raw,
        _ => {
            append_audit_log(
                &audit_log,
                "[SKIP] official_experience_reuse_with_real_qwen skipped: missing QWEN_API_KEY",
            );
            return;
        }
    };
    append_audit_log(&audit_log, "[STEP] QWEN_API_KEY detected");

    let run_root = unique_path("run");
    let official_store_root = run_root.join("official-store");
    let worker_store_root = run_root.join("worker-store");
    let official_workspace_root = run_root.join("official-workspace");
    let worker_workspace_root = run_root.join("worker-workspace");
    let official_sandbox_root = run_root.join("official-sandbox");
    let worker_sandbox_root = run_root.join("worker-sandbox");
    let realtime_logger = RealtimeLogger::new("official-reuse-test", &run_root);
    realtime_logger.log_event(
        "system",
        "bootstrap",
        "phase_transition",
        json!({"stage": "bootstrap"}),
    );

    let (official_evo, official_store) = build_evo(
        "official",
        &official_workspace_root,
        &official_sandbox_root,
        &official_store_root,
    );
    let (worker_evo, _worker_store) = build_evo(
        "worker",
        &worker_workspace_root,
        &worker_sandbox_root,
        &worker_store_root,
    );
    let official_node =
        EvolutionNetworkNode::new(official_store.clone() as Arc<dyn EvolutionStore>);
    append_audit_log(
        &audit_log,
        format!(
            "[STEP] stores ready run_root={} official_store={} worker_store={}",
            run_root.display(),
            official_store_root.display(),
            worker_store_root.display()
        ),
    );
    append_audit_log(
        &audit_log,
        format!(
            "[STEP] realtime logs human={} jsonl={}",
            realtime_logger.human_path().display(),
            realtime_logger.jsonl_path().display()
        ),
    );

    let ensure = official_node
        .ensure_builtin_experience_assets("runtime-bootstrap")
        .unwrap();
    assert!(ensure.accepted);
    realtime_logger.log_event(
        "official-node",
        "stage-0",
        "phase_transition",
        json!({
            "stage": "[0] Official builtin assets ensured",
            "imported_asset_ids": ensure.imported_asset_ids.len(),
        }),
    );
    append_audit_log(
        &audit_log,
        format!(
            "[STEP] ensure_builtin_experience_assets accepted={} imported={}",
            ensure.accepted,
            ensure.imported_asset_ids.len()
        ),
    );

    let fetch = official_node
        .fetch_assets(
            "agent-official",
            &FetchQuery {
                sender_id: "agent-official".to_string(),
                signals: vec![
                    "error".to_string(),
                    "failed".to_string(),
                    "unstable".to_string(),
                    "log_error".to_string(),
                ],
                since_cursor: None,
                resume_token: None,
            },
        )
        .unwrap();
    assert!(!fetch.assets.is_empty());
    realtime_logger.log_event(
        "official-node",
        "stage-1",
        "phase_transition",
        json!({
            "stage": "[1] Official assets fetched",
            "matched_assets": fetch.assets.len(),
        }),
    );
    append_audit_log(
        &audit_log,
        format!("[STEP] fetch_assets matched={}", fetch.assets.len()),
    );

    let official_projection = official_store.rebuild_projection().unwrap();
    let official_gene = official_projection
        .genes
        .iter()
        .find(|gene| {
            strategy_value(&gene.strategy, "asset_origin").as_deref() == Some("builtin_evomap")
                && gene
                    .signals
                    .iter()
                    .any(|signal| signal.eq_ignore_ascii_case("error"))
        })
        .unwrap()
        .clone();
    let official_capsule = official_projection
        .capsules
        .iter()
        .find(|capsule| capsule.gene_id == official_gene.id)
        .unwrap()
        .clone();
    append_audit_log(
        &audit_log,
        format!(
            "[STEP] selected assets gene_id={} capsule_id={} mutation_id={}",
            official_gene.id, official_capsule.id, official_capsule.mutation_id
        ),
    );

    let envelope = official_node
        .publish_local_assets("agent-official")
        .unwrap();
    assert!(!envelope.assets.is_empty());
    realtime_logger.log_event(
        "official-node",
        "stage-2",
        "phase_transition",
        json!({
            "stage": "[2] Official envelope published",
            "published_assets": envelope.assets.len(),
        }),
    );
    append_audit_log(
        &audit_log,
        format!("[STEP] publish_local_assets size={}", envelope.assets.len()),
    );

    let import = worker_evo.import_remote_envelope(&envelope).unwrap();
    assert!(import.accepted);
    assert!(!import.imported_asset_ids.is_empty());
    realtime_logger.log_event(
        "worker-node",
        "stage-3",
        "phase_transition",
        json!({
            "stage": "[3] Worker imported official assets",
            "imported_ids": import.imported_asset_ids.len(),
        }),
    );
    append_audit_log(
        &audit_log,
        format!(
            "[STEP] import_remote_envelope accepted={} imported={}",
            import.accepted,
            import.imported_asset_ids.len()
        ),
    );

    let merged_signals = {
        let mut merged = BTreeSet::new();
        for signal in [
            "error",
            "failed",
            "unstable",
            "log_error",
            "windows_shell_incompatible",
            "unknown command process",
        ] {
            merged.insert(signal.to_string());
        }
        for signal in &official_gene.signals {
            merged.insert(signal.clone());
        }
        merged.into_iter().collect::<Vec<_>>()
    };

    let decision = worker_evo
        .replay_or_fallback_for_run(
            &"test-replay-run".to_string(),
            SelectorInput {
                signals: merged_signals.clone(),
                env: official_capsule.env.clone(),
                spec_id: None,
                limit: 1,
            },
        )
        .await
        .unwrap();
    let replay_feedback =
        EvoKernel::<TestState>::replay_feedback_for_agent(&merged_signals, &decision);
    let directive_execution = consume_replay_directive(&replay_feedback);
    assert!(decision.used_capsule);
    assert!(!decision.fallback_to_planner);
    assert_eq!(
        directive_execution.route,
        DirectiveExecutionRoute::ReuseWithoutPlanner
    );
    assert!(directive_execution.fallback_classification.is_none());
    realtime_logger.log_event(
        "worker-node",
        "stage-4",
        "phase_transition",
        json!({
            "stage": "[4] Worker replay decision",
            "used_capsule": decision.used_capsule,
            "fallback_to_planner": decision.fallback_to_planner,
            "planner_directive": replay_feedback.planner_directive,
            "next_action": replay_feedback.next_action,
            "repair_hint": replay_feedback.repair_hint,
        }),
    );
    realtime_logger.log_event(
        "worker-node",
        "stage-4",
        "directive_consumed",
        json!({
            "route": format!("{:?}", directive_execution.route),
            "verification_hint": &directive_execution.verification_hint,
            "fallback_classification": &directive_execution.fallback_classification,
        }),
    );
    append_audit_log(
        &audit_log,
        format!(
            "[STEP] replay used_capsule={} fallback={} reason={} directive={:?} next_action={:?}",
            decision.used_capsule,
            decision.fallback_to_planner,
            decision.reason,
            replay_feedback.planner_directive,
            replay_feedback.next_action
        ),
    );

    let agent = make_agent(
        "你是官方经验复用 Agent。输出必须包含根因分析、修复步骤、验证命令、回滚方案，并且必须调用工具。",
        realtime_logger.clone(),
    );
    let plan_a = agent
        .invoke_messages(vec![Message::new_human_message(directive_guided_prompt(
            "处理 case-a：unknown command 'process'。请调用工具并输出四节结构化修复方案。",
            &directive_execution,
        ))])
        .await
        .unwrap();
    quality_gate(&plan_a);
    realtime_logger.log_event(
        "worker-agent",
        "stage-5",
        "phase_transition",
        json!({
            "stage": "[5] Qwen repair plan generated",
            "len": plan_a.len(),
        }),
    );
    append_audit_log(
        &audit_log,
        format!("[STEP] primary plan quality passed len={}", plan_a.len()),
    );

    let plan_b = agent
        .invoke_messages(vec![Message::new_human_message(directive_guided_prompt(
            "处理 case-b：unknown command 'proccess'。请调用工具并输出四节结构化修复方案。",
            &directive_execution,
        ))])
        .await
        .unwrap();
    quality_gate(&plan_b);
    realtime_logger.log_event(
        "worker-agent",
        "stage-6",
        "phase_transition",
        json!({
            "stage": "[6] Qwen similar-task repair generated",
            "len": plan_b.len(),
        }),
    );
    append_audit_log(
        &audit_log,
        format!("[STEP] similar plan quality passed len={}", plan_b.len()),
    );

    let report_path = run_root.join("verification_report.md");
    let events_summary_path = run_root.join("events_summary.json");
    let replay_evidence_path = run_root.join("replay_evidence.json");
    let realtime_log_path = run_root.join("agent_realtime.log");
    let realtime_jsonl_path = run_root.join("agent_realtime.jsonl");

    let official_events = summarize(&official_store_root.join("events.jsonl"));
    let worker_events = summarize(&worker_store_root.join("events.jsonl"));
    std::fs::write(
        &events_summary_path,
        serde_json::to_string_pretty(&json!({
            "official": official_events,
            "worker": worker_events,
        }))
        .unwrap(),
    )
    .unwrap();

    std::fs::write(
        &replay_evidence_path,
        serde_json::to_string_pretty(&json!({
            "used_capsule": decision.used_capsule,
            "fallback_to_planner": decision.fallback_to_planner,
            "reason": decision.reason,
            "planner_directive": replay_feedback.planner_directive,
            "next_action": replay_feedback.next_action,
            "repair_hint": replay_feedback.repair_hint,
            "official_gene_id": official_gene.id,
            "official_capsule_id": official_capsule.id,
            "mutation_id": official_capsule.mutation_id,
            "asset_origin": strategy_value(&official_gene.strategy, "asset_origin").unwrap_or_default(),
            "imported_asset_ids": import.imported_asset_ids.len(),
            "realtime_log_path": realtime_log_path.display().to_string(),
            "realtime_jsonl_path": realtime_jsonl_path.display().to_string(),
        }))
        .unwrap(),
    )
    .unwrap();

    std::fs::write(
        &report_path,
        format!(
            "# Official Experience Reuse Test Report\n\n- used_capsule: `{}`\n- fallback_to_planner: `{}`\n- official_gene_id: `{}`\n- official_capsule_id: `{}`\n- mutation_id: `{}`\n- plan_a_len: {}\n- plan_b_len: {}\n",
            decision.used_capsule,
            decision.fallback_to_planner,
            official_gene.id,
            official_capsule.id,
            official_capsule.mutation_id,
            plan_a.len(),
            plan_b.len(),
        ),
    )
    .unwrap();

    assert!(report_path.exists());
    assert!(events_summary_path.exists());
    assert!(replay_evidence_path.exists());
    assert!(realtime_log_path.exists());
    assert!(realtime_jsonl_path.exists());
    assert_realtime_events(&realtime_jsonl_path);
    realtime_logger.log_event(
        "system",
        "stage-7",
        "phase_transition",
        json!({
            "stage": "[7] Verification report",
            "report": report_path.display().to_string(),
            "realtime_jsonl": realtime_jsonl_path.display().to_string(),
        }),
    );
    append_audit_log(
        &audit_log,
        format!(
            "[STEP] artifacts written report={} events_summary={} replay_evidence={} realtime_log={} realtime_jsonl={}",
            report_path.display(),
            events_summary_path.display(),
            replay_evidence_path.display(),
            realtime_log_path.display(),
            realtime_jsonl_path.display()
        ),
    );

    let replay_evidence: Value =
        serde_json::from_str(&std::fs::read_to_string(&replay_evidence_path).unwrap()).unwrap();
    assert_eq!(
        replay_evidence
            .get("used_capsule")
            .and_then(Value::as_bool)
            .unwrap(),
        true
    );
    assert_eq!(
        replay_evidence
            .get("fallback_to_planner")
            .and_then(Value::as_bool)
            .unwrap(),
        false
    );
    assert_eq!(
        replay_evidence
            .get("asset_origin")
            .and_then(Value::as_str)
            .unwrap(),
        "builtin_evomap"
    );

    let report = std::fs::read_to_string(&report_path).unwrap();
    assert!(report.contains("official_gene_id"));
    assert!(report.contains("mutation_id"));
    append_audit_log(&audit_log, "[STEP] report content assertions passed");

    let _ = official_evo.metrics_snapshot().unwrap();
    append_audit_log(
        &audit_log,
        "[PASS] official_experience_reuse_with_real_qwen",
    );
}
