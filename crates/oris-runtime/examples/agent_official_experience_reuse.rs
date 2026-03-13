//! Official EvoMap experience reuse demo with enforced real Qwen calls.
//!
//! Run:
//! `QWEN_API_KEY=... cargo run -p oris-runtime --example agent_official_experience_reuse --features "full-evolution-experimental"`

#[cfg(feature = "full-evolution-experimental")]
use std::collections::{BTreeMap, BTreeSet};
#[cfg(feature = "full-evolution-experimental")]
use std::fs;
#[cfg(feature = "full-evolution-experimental")]
use std::io::{Error as IoError, ErrorKind as IoErrorKind, Write};
#[cfg(feature = "full-evolution-experimental")]
use std::path::{Path, PathBuf};
#[cfg(feature = "full-evolution-experimental")]
use std::sync::{Arc, Mutex};

#[cfg(feature = "full-evolution-experimental")]
use async_trait::async_trait;
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::agent::middleware::{Middleware, MiddlewareContext, MiddlewareError};
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::agent::{create_agent_from_llm, UnifiedAgent};
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::agent_contract::{
    ReplayFallbackNextAction, ReplayFeedback, ReplayPlannerDirective,
};
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::error::ToolError;
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::evolution::{
    evaluate_repair_quality_gate, CommandValidator, EvoAssetState,
    EvoEvolutionStore as EvolutionStore, EvoKernel, EvoSandboxPolicy as SandboxPolicy,
    EvoSelectorInput as SelectorInput, EvolutionNetworkNode, FetchQuery, JsonlEvolutionStore,
    LocalProcessSandbox, ValidationPlan, ValidationStage,
};
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::governor::{DefaultGovernor, GovernorConfig};
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::kernel::{
    AllowAllPolicy, InMemoryEventStore, Kernel, KernelMode, KernelState, NoopActionExecutor,
    NoopStepFn, StateUpdatedOnlyReducer,
};
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::language_models::{options::CallOptions, GenerateResult};
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::llm::Qwen;
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::prompt::PromptArgs;
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::schemas::agent::{AgentAction, AgentEvent, AgentFinish};
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
        "Run: QWEN_API_KEY=... cargo run -p oris-runtime --example agent_official_experience_reuse --features \"full-evolution-experimental\""
    );
}

#[cfg(feature = "full-evolution-experimental")]
type ExampleResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[cfg(feature = "full-evolution-experimental")]
const OFFICIAL_FETCH_SIGNALS: [&str; 4] = ["error", "failed", "unstable", "log_error"];

#[cfg(feature = "full-evolution-experimental")]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct DemoState;

#[cfg(feature = "full-evolution-experimental")]
impl KernelState for DemoState {
    fn version(&self) -> u32 {
        1
    }
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Debug)]
struct DemoPaths {
    run_id: String,
    runs_root: PathBuf,
    run_root: PathBuf,
    official_store_root: PathBuf,
    worker_store_root: PathBuf,
    official_workspace_root: PathBuf,
    worker_workspace_root: PathBuf,
    official_sandbox_root: PathBuf,
    worker_sandbox_root: PathBuf,
    primary_plan_path: PathBuf,
    similar_plan_path: PathBuf,
    report_path: PathBuf,
    events_summary_path: PathBuf,
    replay_evidence_path: PathBuf,
    realtime_log_path: PathBuf,
    realtime_jsonl_path: PathBuf,
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
#[derive(Debug, Default)]
struct RealtimeSummary {
    token_chunk_count: usize,
    tool_call_event_count: usize,
    phase_transition_count: usize,
    finish_count: usize,
    excerpts: Vec<String>,
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Clone)]
struct RealtimeLogger {
    run_id: String,
    human_path: PathBuf,
    jsonl_path: PathBuf,
    human_file: Arc<Mutex<fs::File>>,
    jsonl_file: Arc<Mutex<fs::File>>,
}

#[cfg(feature = "full-evolution-experimental")]
impl RealtimeLogger {
    fn new(run_id: &str, human_path: PathBuf, jsonl_path: PathBuf) -> ExampleResult<Self> {
        let human_file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&human_path)?;
        let jsonl_file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&jsonl_path)?;
        Ok(Self {
            run_id: run_id.to_string(),
            human_path,
            jsonl_path,
            human_file: Arc::new(Mutex::new(human_file)),
            jsonl_file: Arc::new(Mutex::new(jsonl_file)),
        })
    }

    fn human_path(&self) -> &Path {
        &self.human_path
    }

    fn jsonl_path(&self) -> &Path {
        &self.jsonl_path
    }

    fn log_event(&self, agent_role: &str, phase: &str, event: &str, payload: Value) {
        self.write_event(agent_role, phase, event, payload, true);
    }

    fn log_token_chunk(&self, agent_role: &str, phase: &str, chunk: &str) {
        print!("{chunk}");
        let _ = std::io::stdout().flush();
        self.write_event(
            agent_role,
            phase,
            "token_chunk",
            json!({
                "chunk_len": chunk.chars().count(),
                "chunk_preview": preview(chunk, 120),
            }),
            false,
        );
    }

    fn write_event(
        &self,
        agent_role: &str,
        phase: &str,
        event: &str,
        payload: Value,
        emit_stdout: bool,
    ) {
        let ts = now_millis().to_string();
        let record = json!({
            "ts": ts,
            "run_id": self.run_id,
            "agent_role": agent_role,
            "phase": phase,
            "event": event,
            "payload": payload,
        });
        let human_line = format!(
            "[{}] run_id={} role={} phase={} event={} payload={}",
            ts,
            self.run_id,
            agent_role,
            phase,
            event,
            record.get("payload").cloned().unwrap_or_else(|| json!({}))
        );
        if emit_stdout {
            println!("{human_line}");
        }
        if let Err(err) = self.append_record(&human_line, &record) {
            self.write_error_fallback(format!("append_record_failed: {}", err));
        }
    }

    fn append_record(&self, human_line: &str, record: &Value) -> std::io::Result<()> {
        let mut human = self
            .human_file
            .lock()
            .map_err(|_| IoError::new(IoErrorKind::Other, "realtime human log lock poisoned"))?;
        writeln!(human, "{human_line}")?;
        drop(human);

        let mut jsonl = self
            .jsonl_file
            .lock()
            .map_err(|_| IoError::new(IoErrorKind::Other, "realtime jsonl log lock poisoned"))?;
        writeln!(jsonl, "{}", serde_json::to_string(record)?)?;
        Ok(())
    }

    fn write_error_fallback(&self, message: String) {
        let fallback = json!({
            "ts": now_millis().to_string(),
            "run_id": self.run_id,
            "agent_role": "logger",
            "phase": "realtime",
            "event": "error",
            "payload": {"message": message},
        });
        let line = serde_json::to_string(&fallback).unwrap_or_else(|_| "{}".to_string());
        let _ = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.jsonl_path)
            .and_then(|mut file| writeln!(file, "{line}"));
        eprintln!("[realtime-logger-error] {}", line);
    }
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Clone)]
struct RealtimeMiddleware {
    logger: RealtimeLogger,
    agent_role: String,
}

#[cfg(feature = "full-evolution-experimental")]
impl RealtimeMiddleware {
    fn new(logger: RealtimeLogger, agent_role: &str) -> Self {
        Self {
            logger,
            agent_role: agent_role.to_string(),
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

#[cfg(feature = "full-evolution-experimental")]
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
        let mut input_keys = input.keys().cloned().collect::<Vec<_>>();
        input_keys.sort();
        self.logger.log_event(
            &self.agent_role,
            "agent_plan",
            "before_agent_plan",
            json!({
                "iteration": context.iteration,
                "steps": steps.len(),
                "tool_call_count": context.tool_call_count,
                "input_keys": input_keys,
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
                "planned_actions": actions.len(),
                "tools": actions.iter().map(|action| action.tool.clone()).collect::<Vec<_>>(),
            }),
            AgentEvent::Finish(finish) => json!({
                "iteration": context.iteration,
                "finish_preview": preview(&finish.output, 180),
            }),
        };
        self.logger
            .log_event(&self.agent_role, "agent_plan", "after_agent_plan", payload);
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
            &self.agent_role,
            "tool",
            "tool_call_before",
            json!({
                "iteration": context.iteration,
                "tool": action.tool,
                "tool_input_summary": preview(&action.tool_input, 220),
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
            &self.agent_role,
            "tool",
            "tool_call_after",
            json!({
                "iteration": context.iteration,
                "tool": action.tool,
                "observation_len": observation.chars().count(),
                "observation_summary": preview(observation, 220),
            }),
        );
        Ok(None)
    }

    async fn before_finish(
        &self,
        finish: &AgentFinish,
        context: &mut MiddlewareContext,
    ) -> Result<Option<AgentFinish>, MiddlewareError> {
        if !Self::should_log_once(
            context,
            format!(
                "rt-before-finish-{}-{}",
                context.iteration,
                finish.output.len()
            ),
        ) {
            return Ok(None);
        }
        self.logger.log_event(
            &self.agent_role,
            "finish",
            "before_finish",
            json!({
                "iteration": context.iteration,
                "output_summary": preview(&finish.output, 220),
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
            &self.agent_role,
            "finish",
            "finish",
            json!({
                "iteration": context.iteration,
                "output_summary": preview(&finish.output, 220),
                "result_summary": preview(&result.generation, 220),
                "token_usage": result.tokens,
            }),
        );
        Ok(())
    }
}

#[cfg(feature = "full-evolution-experimental")]
struct ErrorIncidentTool;

#[cfg(feature = "full-evolution-experimental")]
#[async_trait]
impl Tool for ErrorIncidentTool {
    fn name(&self) -> String {
        "official_error_incidents".to_string()
    }

    fn description(&self) -> String {
        "Return deterministic incident samples aligned with official EvoMap repair signals."
            .to_string()
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
            .unwrap_or("case-primary");
        let payload = if case_id == "case-similar" {
            json!({
                "case_id": case_id,
                "symptoms": [
                    "CI Linux runner 报错: unknown command 'proccess'",
                    "脚本执行环境混用了 bash 与 powershell 参数风格",
                    "错误率上升且重试未恢复"
                ],
                "signals": ["error", "failed", "unstable", "windows_shell_incompatible", "perf_bottleneck"],
                "raw_log": "error: unknown command 'proccess'\\nCommand exited with code 1"
            })
        } else {
            json!({
                "case_id": case_id,
                "symptoms": [
                    "CLI 报错: unknown command 'process'",
                    "Windows shell 参数格式不兼容",
                    "执行链路失败并触发重试"
                ],
                "signals": ["error", "failed", "unstable", "log_error", "windows_shell_incompatible"],
                "raw_log": "error: unknown command 'process'\\nCommand exited with code 1"
            })
        };
        serde_json::to_string_pretty(&payload).map_err(|e| ToolError::ExecutionError(e.to_string()))
    }
}

#[cfg(feature = "full-evolution-experimental")]
struct ShellRepairPlaybookTool;

#[cfg(feature = "full-evolution-experimental")]
#[async_trait]
impl Tool for ShellRepairPlaybookTool {
    fn name(&self) -> String {
        "shell_repair_playbook".to_string()
    }

    fn description(&self) -> String {
        "Provide deterministic shell-compat repair patterns and command migration examples."
            .to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "platform": {"type": "string"}
            },
            "required": ["platform"]
        })
    }

    async fn run(&self, input: Value) -> Result<String, ToolError> {
        let platform = input
            .get("platform")
            .and_then(Value::as_str)
            .unwrap_or("cross-platform");
        let payload = json!({
            "platform": platform,
            "fix_patterns": [
                "统一入口脚本，禁止直接拼接 OS 特定参数",
                "将 shell 分支逻辑下沉到独立函数并显式检测平台",
                "命令别名校验：process/proccess/proc 归一到受控命令集"
            ],
            "examples": [
                {"before": "cmd /c tool process", "after": "tool run --mode process"},
                {"before": "powershell tool process", "after": "tool run --mode process"}
            ]
        });
        serde_json::to_string_pretty(&payload).map_err(|e| ToolError::ExecutionError(e.to_string()))
    }
}

#[cfg(feature = "full-evolution-experimental")]
struct VerificationChecklistTool;

#[cfg(feature = "full-evolution-experimental")]
#[async_trait]
impl Tool for VerificationChecklistTool {
    fn name(&self) -> String {
        "repair_verification_checklist".to_string()
    }

    fn description(&self) -> String {
        "Return deterministic verification and rollback checklist for command repair.".to_string()
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

    async fn run(&self, input: Value) -> Result<String, ToolError> {
        let scope = input
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or("command-repair");
        let payload = json!({
            "scope": scope,
            "verification_commands": [
                "cargo --version",
                "cargo check -p oris-runtime",
                "tool run --mode process --dry-run"
            ],
            "rollback_plan": [
                "恢复上一版入口脚本",
                "关闭新命令路由开关",
                "回滚到最近稳定 tag 并重新验证"
            ]
        });
        serde_json::to_string_pretty(&payload).map_err(|e| ToolError::ExecutionError(e.to_string()))
    }
}

#[cfg(feature = "full-evolution-experimental")]
fn build_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(ErrorIncidentTool),
        Arc::new(ShellRepairPlaybookTool),
        Arc::new(VerificationChecklistTool),
    ]
}

#[cfg(feature = "full-evolution-experimental")]
fn make_agent(
    system_prompt: &str,
    logger: RealtimeLogger,
    agent_role: &str,
) -> ExampleResult<UnifiedAgent> {
    let tools = build_tools();
    let streaming_logger = logger.clone();
    let streaming_role = agent_role.to_string();
    let callback = move |chunk: String| {
        let streaming_logger = streaming_logger.clone();
        let streaming_role = streaming_role.clone();
        async move {
            streaming_logger.log_token_chunk(&streaming_role, "llm_stream", &chunk);
            Ok(())
        }
    };
    let llm = Qwen::new()
        .with_api_key(std::env::var("QWEN_API_KEY").unwrap_or_default())
        .with_model("qwen3-max")
        .with_options(
            CallOptions::default()
                .with_temperature(0.4)
                .with_max_tokens(1800)
                .with_streaming_func(callback),
        );
    let middleware: Vec<Arc<dyn Middleware>> =
        vec![Arc::new(RealtimeMiddleware::new(logger, agent_role))];
    Ok(create_agent_from_llm(llm, &tools, Some(system_prompt))?
        .with_middleware(middleware)
        .with_max_iterations(16)
        .with_break_if_error(true))
}

#[cfg(feature = "full-evolution-experimental")]
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

#[cfg(feature = "full-evolution-experimental")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct AgentDirectiveExecution {
    route: DirectiveExecutionRoute,
    repair_hint: Option<String>,
    verification_hint: String,
    fallback_classification: Option<String>,
}

#[cfg(feature = "full-evolution-experimental")]
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

#[cfg(feature = "full-evolution-experimental")]
fn timestamp_run_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("run-{now}")
}

#[cfg(feature = "full-evolution-experimental")]
fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(feature = "full-evolution-experimental")]
fn resolve_paths() -> ExampleResult<DemoPaths> {
    let workspace_root = std::env::current_dir()?;
    let runs_root = std::env::var("ORIS_OFFICIAL_REUSE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace_root.join("docs/evokernel/official_reuse_runs"));
    let run_id = std::env::var("ORIS_OFFICIAL_REUSE_RUN_ID").unwrap_or_else(|_| timestamp_run_id());
    let run_root = runs_root.join(&run_id);
    let sandbox_root = std::env::var("ORIS_OFFICIAL_REUSE_SANDBOX_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("oris-official-reuse-sandbox"));

    Ok(DemoPaths {
        run_id,
        runs_root,
        run_root: run_root.clone(),
        official_store_root: run_root.join("official-store"),
        worker_store_root: run_root.join("worker-store"),
        official_workspace_root: run_root.join("official-workspace"),
        worker_workspace_root: run_root.join("worker-workspace"),
        official_sandbox_root: sandbox_root.join("official"),
        worker_sandbox_root: sandbox_root.join("worker"),
        primary_plan_path: run_root.join("qwen_repair_plan_primary.md"),
        similar_plan_path: run_root.join("qwen_repair_plan_similar.md"),
        report_path: run_root.join("verification_report.md"),
        events_summary_path: run_root.join("events_summary.json"),
        replay_evidence_path: run_root.join("replay_evidence.json"),
        realtime_log_path: run_root.join("agent_realtime.log"),
        realtime_jsonl_path: run_root.join("agent_realtime.jsonl"),
    })
}

#[cfg(feature = "full-evolution-experimental")]
fn prepare_dirs(paths: &DemoPaths) -> ExampleResult<()> {
    fs::create_dir_all(&paths.runs_root)?;
    if paths.run_root.exists() {
        let _ = fs::remove_dir_all(&paths.run_root);
    }
    for dir in [
        &paths.run_root,
        &paths.official_store_root,
        &paths.worker_store_root,
        &paths.official_workspace_root,
        &paths.worker_workspace_root,
        &paths.official_sandbox_root,
        &paths.worker_sandbox_root,
    ] {
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

#[cfg(feature = "full-evolution-experimental")]
fn setup_workspace(path: &Path) -> ExampleResult<()> {
    if path.exists() {
        let _ = fs::remove_dir_all(path);
    }
    fs::create_dir_all(path.join("docs/evolution"))?;
    fs::write(path.join("README.md"), "# Official Experience Reuse Demo\n")?;
    let _ = std::process::Command::new("git")
        .arg("init")
        .current_dir(path)
        .output();
    Ok(())
}

#[cfg(feature = "full-evolution-experimental")]
fn demo_policy() -> SandboxPolicy {
    SandboxPolicy {
        allowed_programs: vec!["cargo".into(), "git".into()],
        max_duration_ms: 120_000,
        max_output_bytes: 1_048_576,
        denied_env_prefixes: vec!["TOKEN".into(), "KEY".into(), "SECRET".into()],
    }
}

#[cfg(feature = "full-evolution-experimental")]
fn demo_validation_plan() -> ValidationPlan {
    ValidationPlan {
        profile: "official-reuse-validation".into(),
        stages: vec![ValidationStage::Command {
            program: "cargo".into(),
            args: vec!["--version".into()],
            timeout_ms: 20_000,
        }],
    }
}

#[cfg(feature = "full-evolution-experimental")]
fn build_demo_evo(
    label: &str,
    workspace_root: &Path,
    sandbox_root: &Path,
    store_root: &Path,
) -> ExampleResult<(EvoKernel<DemoState>, Arc<JsonlEvolutionStore>)> {
    setup_workspace(workspace_root)?;
    let _ = fs::remove_dir_all(sandbox_root);
    let _ = fs::remove_dir_all(store_root);
    fs::create_dir_all(sandbox_root)?;
    fs::create_dir_all(store_root)?;

    let kernel = Arc::new(Kernel::<DemoState> {
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
    let policy = demo_policy();
    let validator = Arc::new(CommandValidator::new(policy.clone()));
    let sandbox = Arc::new(LocalProcessSandbox::new(
        format!("run-{label}"),
        workspace_root,
        sandbox_root,
    ));

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
    .with_sandbox_policy(policy)
    .with_validation_plan(demo_validation_plan());

    Ok((evo, store))
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

#[cfg(feature = "full-evolution-experimental")]
fn merge_signals(base: &[String], extra: &[String]) -> Vec<String> {
    let mut merged = BTreeSet::new();
    for signal in base {
        merged.insert(signal.to_string());
    }
    for signal in extra {
        merged.insert(signal.to_string());
    }
    merged.into_iter().collect()
}

#[cfg(feature = "full-evolution-experimental")]
fn repair_quality_gate(plan: &str) -> ExampleResult<QualityCheckResult> {
    let report = evaluate_repair_quality_gate(plan);
    let checks = vec![
        ("包含根因分析".to_string(), report.root_cause),
        ("包含修复步骤".to_string(), report.fix),
        ("包含验证命令".to_string(), report.verification),
        ("包含回滚方案".to_string(), report.rollback),
        (
            "包含unknown command故障上下文".to_string(),
            report.incident_anchor,
        ),
        (
            "结构化修复信息至少满足3项（根因/修复/验证/回滚）".to_string(),
            report.structure_score >= 3,
        ),
        (
            "包含可执行验证命令或验证计划".to_string(),
            report.has_actionable_command || report.verification,
        ),
    ];

    let missing = checks
        .iter()
        .filter_map(|(name, passed)| if *passed { None } else { Some(name.clone()) })
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!("repair quality gate failed: {:?}", missing).into());
    }
    Ok(QualityCheckResult { checks })
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

        if matches!(
            kind.as_str(),
            "remote_asset_imported"
                | "promotion_evaluated"
                | "gene_promoted"
                | "capsule_released"
                | "capsule_reused"
                | "capsule_quarantined"
                | "mutation_declared"
        ) {
            summary
                .key_events
                .push(format!("seq_line={} {}", idx + 1, preview(line, 220)));
        }
    }

    Ok(summary)
}

#[cfg(feature = "full-evolution-experimental")]
fn summarize_realtime_file(path: &Path) -> ExampleResult<RealtimeSummary> {
    let mut summary = RealtimeSummary::default();
    if !path.exists() {
        return Ok(summary);
    }

    for (idx, line) in fs::read_to_string(path)?.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line)?;
        let event = value
            .get("event")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        match event {
            "token_chunk" => summary.token_chunk_count += 1,
            "tool_call_before" | "tool_call_after" => summary.tool_call_event_count += 1,
            "phase_transition" => summary.phase_transition_count += 1,
            "finish" => summary.finish_count += 1,
            _ => {}
        }
        if summary.excerpts.len() < 12 {
            summary
                .excerpts
                .push(format!("line{} {}", idx + 1, preview(line, 220)));
        }
    }

    Ok(summary)
}

#[cfg(feature = "full-evolution-experimental")]
fn write_events_summary(
    path: &Path,
    official_summary: &EventSummary,
    worker_summary: &EventSummary,
    realtime_log_path: &Path,
    realtime_jsonl_path: &Path,
) -> ExampleResult<()> {
    let payload = json!({
        "official": {
            "total": official_summary.total,
            "counts": official_summary.counts,
            "key_events": official_summary.key_events,
        },
        "worker": {
            "total": worker_summary.total,
            "counts": worker_summary.counts,
            "key_events": worker_summary.key_events,
        },
        "realtime_log": {
            "human_log_path": realtime_log_path.display().to_string(),
            "jsonl_log_path": realtime_jsonl_path.display().to_string(),
        },
    });
    fs::write(path, serde_json::to_string_pretty(&payload)?)?;
    Ok(())
}

#[cfg(feature = "full-evolution-experimental")]
fn write_replay_evidence(
    path: &Path,
    run_id: &str,
    official_gene_id: &str,
    official_capsule_id: &str,
    mutation_id: &str,
    asset_origin: &str,
    imported_asset_ids: usize,
    used_capsule: bool,
    fallback_to_planner: bool,
    reason: &str,
    realtime_log_path: &Path,
    realtime_jsonl_path: &Path,
) -> ExampleResult<()> {
    let payload = json!({
        "run_id": run_id,
        "official_gene_id": official_gene_id,
        "official_capsule_id": official_capsule_id,
        "mutation_id": mutation_id,
        "asset_origin": asset_origin,
        "imported_asset_ids": imported_asset_ids,
        "used_capsule": used_capsule,
        "fallback_to_planner": fallback_to_planner,
        "reason": reason,
        "realtime_log_path": realtime_log_path.display().to_string(),
        "realtime_jsonl_path": realtime_jsonl_path.display().to_string(),
    });
    fs::write(path, serde_json::to_string_pretty(&payload)?)?;
    Ok(())
}

#[cfg(feature = "full-evolution-experimental")]
fn write_verification_report(
    path: &Path,
    paths: &DemoPaths,
    official_gene_id: &str,
    official_capsule_id: &str,
    mutation_id: &str,
    imported_asset_ids: usize,
    used_capsule: bool,
    fallback_to_planner: bool,
    reason: &str,
    primary_quality: &QualityCheckResult,
    similar_quality: &QualityCheckResult,
    official_summary: &EventSummary,
    worker_summary: &EventSummary,
    realtime_summary: &RealtimeSummary,
    realtime_log_path: &Path,
    realtime_jsonl_path: &Path,
) -> ExampleResult<()> {
    let mut report = String::new();
    report.push_str("# Verification Report: Official Experience Reuse Agent\n\n");
    report.push_str("## Run Context\n");
    report.push_str(&format!("- run_id: `{}`\n", paths.run_id));
    report.push_str("- model: `qwen:qwen3-max`\n");
    report.push_str(&format!("- run_root: `{}`\n", paths.run_root.display()));
    report.push_str(&format!(
        "- official_store: `{}`\n",
        paths.official_store_root.display()
    ));
    report.push_str(&format!(
        "- worker_store: `{}`\n",
        paths.worker_store_root.display()
    ));
    report.push_str(&format!(
        "- primary_plan: `{}`\n",
        paths.primary_plan_path.display()
    ));
    report.push_str(&format!(
        "- similar_plan: `{}`\n",
        paths.similar_plan_path.display()
    ));
    report.push_str(&format!(
        "- events_summary: `{}`\n",
        paths.events_summary_path.display()
    ));
    report.push_str(&format!(
        "- replay_evidence: `{}`\n\n",
        paths.replay_evidence_path.display()
    ));
    report.push_str(&format!(
        "- realtime_log: `{}`\n",
        realtime_log_path.display()
    ));
    report.push_str(&format!(
        "- realtime_jsonl: `{}`\n\n",
        realtime_jsonl_path.display()
    ));

    report.push_str("## Official Asset Evidence\n");
    report.push_str(&format!("- gene_id: `{}`\n", official_gene_id));
    report.push_str(&format!("- capsule_id: `{}`\n", official_capsule_id));
    report.push_str(&format!("- mutation_id: `{}`\n", mutation_id));
    report.push_str(&format!(
        "- imported_asset_ids: `{}`\n\n",
        imported_asset_ids
    ));

    report.push_str("## Replay Decision\n");
    report.push_str(&format!("- used_capsule: `{}`\n", used_capsule));
    report.push_str(&format!(
        "- fallback_to_planner: `{}`\n",
        fallback_to_planner
    ));
    report.push_str(&format!("- reason: `{}`\n\n", reason));

    report.push_str("## Qwen Quality Gate (Primary Task)\n");
    for (name, passed) in &primary_quality.checks {
        report.push_str(&format!(
            "- [{}] {}\n",
            if *passed { "x" } else { " " },
            name
        ));
    }
    report.push('\n');

    report.push_str("## Qwen Quality Gate (Similar Task)\n");
    for (name, passed) in &similar_quality.checks {
        report.push_str(&format!(
            "- [{}] {}\n",
            if *passed { "x" } else { " " },
            name
        ));
    }
    report.push('\n');

    report.push_str("## Event Summary\n");
    report.push_str(&format!(
        "- official_total_events: {}\n",
        official_summary.total
    ));
    report.push_str(&format!(
        "- worker_total_events: {}\n",
        worker_summary.total
    ));
    report.push_str(&format!(
        "- official_counts: {:?}\n",
        official_summary.counts
    ));
    report.push_str(&format!("- worker_counts: {:?}\n\n", worker_summary.counts));

    report.push_str("## Realtime 日志证据\n");
    report.push_str(&format!(
        "- token_chunk_events: {}\n",
        realtime_summary.token_chunk_count
    ));
    report.push_str(&format!(
        "- tool_call_events: {}\n",
        realtime_summary.tool_call_event_count
    ));
    report.push_str(&format!(
        "- phase_transition_events: {}\n",
        realtime_summary.phase_transition_count
    ));
    report.push_str(&format!(
        "- finish_events: {}\n",
        realtime_summary.finish_count
    ));
    report.push_str("- realtime_excerpt:\n");
    for excerpt in &realtime_summary.excerpts {
        report.push_str(&format!("  - {}\n", excerpt));
    }
    report.push('\n');

    report.push_str("## Verdict\n");
    report.push_str("- all stages [0]-[7] completed\n");
    report.push_str("- official builtin assets fetched and reused\n");
    report.push_str("- replay hit before Qwen task execution\n");

    fs::write(path, report)?;
    Ok(())
}

#[cfg(feature = "full-evolution-experimental")]
async fn generate_repair_plan(
    agent: &UnifiedAgent,
    logger: &RealtimeLogger,
    agent_role: &str,
    case_id: &str,
    user_task: &str,
    directive_execution: &AgentDirectiveExecution,
) -> ExampleResult<String> {
    logger.log_event(
        agent_role,
        "task",
        "phase_transition",
        json!({
            "task_case": case_id,
            "message": "start_generate_repair_plan",
        }),
    );
    let prompt = format!(
        "你正在处理故障修复任务：{user_task}\n\
请先调用工具获取 incident/playbook/checklist，然后输出中文结构化结果，必须包含以下四节：\n\
1) 根因分析\n\
2) 修复步骤\n\
3) 验证命令\n\
4) 回滚方案\n\
并且明确引用 case_id={case_id} 的上下文，包含 unknown command 故障关键词。\n\
directive_route={:?}\n\
verification_hint={}\n\
repair_hint={}\n\
fallback_classification={}",
        directive_execution.route,
        directive_execution.verification_hint,
        directive_execution.repair_hint.as_deref().unwrap_or("none"),
        directive_execution
            .fallback_classification
            .as_deref()
            .unwrap_or("none")
    );
    let response = agent
        .invoke_messages(vec![Message::new_human_message(&prompt)])
        .await?;
    logger.log_event(
        agent_role,
        "task",
        "plan_generated",
        json!({
            "task_case": case_id,
            "content_len": response.chars().count(),
            "preview": preview(&response, 220),
        }),
    );
    Ok(response)
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

    println!("=== Official Experience Reuse Agent Demo ===");
    let paths = resolve_paths()?;
    prepare_dirs(&paths)?;
    let logger = RealtimeLogger::new(
        &paths.run_id,
        paths.realtime_log_path.clone(),
        paths.realtime_jsonl_path.clone(),
    )?;
    println!("model: qwen:qwen3-max");
    println!("run_id: {}", paths.run_id);
    println!("run_root: {}", paths.run_root.display());
    logger.log_event(
        "system",
        "bootstrap",
        "phase_transition",
        json!({
            "stage": "bootstrap",
            "model": "qwen:qwen3-max",
            "run_root": paths.run_root.display().to_string(),
        }),
    );

    let (_official_evo, official_store) = build_demo_evo(
        "official",
        &paths.official_workspace_root,
        &paths.official_sandbox_root,
        &paths.official_store_root,
    )?;
    let official_node =
        EvolutionNetworkNode::new(official_store.clone() as Arc<dyn EvolutionStore>);

    let (worker_evo, worker_store) = build_demo_evo(
        "worker",
        &paths.worker_workspace_root,
        &paths.worker_sandbox_root,
        &paths.worker_store_root,
    )?;

    let ensure = official_node.ensure_builtin_experience_assets("runtime-bootstrap")?;
    let official_projection = official_store.rebuild_projection()?;
    let official_gene = official_projection
        .genes
        .iter()
        .find(|gene| {
            gene.state == EvoAssetState::Promoted
                && strategy_value(&gene.strategy, "asset_origin").as_deref()
                    == Some("builtin_evomap")
                && gene
                    .signals
                    .iter()
                    .any(|signal| signal.to_ascii_lowercase().contains("error"))
        })
        .cloned()
        .ok_or("no promoted official builtin_evomap gene found")?;
    let official_capsule = official_projection
        .capsules
        .iter()
        .find(|capsule| {
            capsule.state == EvoAssetState::Promoted && capsule.gene_id == official_gene.id
        })
        .cloned()
        .ok_or("no promoted official capsule for selected gene")?;
    println!("[0] Official builtin assets ensured");
    println!(
        "    imported_asset_ids: {}",
        ensure.imported_asset_ids.len()
    );
    println!("    official_gene_id: {}", official_gene.id);
    println!("    official_capsule_id: {}", official_capsule.id);
    logger.log_event(
        "official-node",
        "stage-0",
        "phase_transition",
        json!({
            "stage": "[0] Official builtin assets ensured",
            "imported_asset_ids": ensure.imported_asset_ids.len(),
            "gene_id": &official_gene.id,
            "capsule_id": &official_capsule.id,
        }),
    );

    let fetched = official_node.fetch_assets(
        "agent-official",
        &FetchQuery {
            sender_id: "agent-official".to_string(),
            signals: OFFICIAL_FETCH_SIGNALS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            since_cursor: None,
            resume_token: None,
        },
    )?;
    if fetched.assets.is_empty() {
        logger.log_event(
            "official-node",
            "stage-1",
            "error",
            json!({"reason": "official assets fetch returned empty"}),
        );
        return Err("official assets fetch returned empty".into());
    }
    println!("[1] Official assets fetched");
    println!("    signals: {:?}", OFFICIAL_FETCH_SIGNALS);
    println!("    fetched_assets: {}", fetched.assets.len());
    logger.log_event(
        "official-node",
        "stage-1",
        "phase_transition",
        json!({
            "stage": "[1] Official assets fetched",
            "signals": OFFICIAL_FETCH_SIGNALS,
            "fetched_assets": fetched.assets.len(),
        }),
    );

    let envelope = official_node.publish_local_assets("agent-official")?;
    if envelope.assets.is_empty() {
        logger.log_event(
            "official-node",
            "stage-2",
            "error",
            json!({"reason": "official envelope is empty"}),
        );
        return Err("official envelope is empty".into());
    }
    println!("[2] Official envelope published");
    println!("    published_assets: {}", envelope.assets.len());
    logger.log_event(
        "official-node",
        "stage-2",
        "phase_transition",
        json!({
            "stage": "[2] Official envelope published",
            "published_assets": envelope.assets.len(),
        }),
    );

    let import = worker_evo.import_remote_envelope(&envelope)?;
    if !import.accepted || import.imported_asset_ids.is_empty() {
        logger.log_event(
            "worker-node",
            "stage-3",
            "error",
            json!({
                "reason": "worker failed to import official assets",
                "accepted": import.accepted,
                "imported_asset_ids": import.imported_asset_ids.len(),
            }),
        );
        return Err("worker failed to import official assets".into());
    }
    let worker_projection = worker_store.rebuild_projection()?;
    let imported_gene = worker_projection
        .genes
        .iter()
        .find(|gene| gene.id == official_gene.id)
        .cloned()
        .ok_or("imported official gene not found in worker projection")?;
    let imported_capsule = worker_projection
        .capsules
        .iter()
        .find(|capsule| capsule.id == official_capsule.id)
        .cloned()
        .ok_or("imported official capsule not found in worker projection")?;
    println!("[3] Worker imported official assets");
    println!("    gene_id: {}", imported_gene.id);
    println!("    capsule_id: {}", imported_capsule.id);
    println!("    mutation_id: {}", imported_capsule.mutation_id);
    println!(
        "    imported_asset_ids: {}",
        import.imported_asset_ids.len()
    );
    logger.log_event(
        "worker-node",
        "stage-3",
        "phase_transition",
        json!({
            "stage": "[3] Worker imported official assets",
            "gene_id": &imported_gene.id,
            "capsule_id": &imported_capsule.id,
            "mutation_id": &imported_capsule.mutation_id,
            "imported_asset_ids": import.imported_asset_ids.len(),
        }),
    );

    let replay_input_signals = merge_signals(
        &vec![
            "error".to_string(),
            "failed".to_string(),
            "unstable".to_string(),
            "log_error".to_string(),
            "windows_shell_incompatible".to_string(),
            "unknown command process".to_string(),
        ],
        &imported_gene.signals,
    );
    let decision = worker_evo
        .replay_or_fallback_for_run(
            &"official-replay-run".to_string(),
            SelectorInput {
                signals: replay_input_signals.clone(),
                env: imported_capsule.env.clone(),
                spec_id: None,
                limit: 1,
            },
        )
        .await?;
    let replay_feedback =
        EvoKernel::<DemoState>::replay_feedback_for_agent(&replay_input_signals, &decision);
    let directive_execution = consume_replay_directive(&replay_feedback);
    println!("[4] Worker replay decision");
    println!("    used_capsule: {}", decision.used_capsule);
    println!("    fallback_to_planner: {}", decision.fallback_to_planner);
    println!("    reason: {}", decision.reason);
    println!(
        "    planner_directive: {:?}",
        replay_feedback.planner_directive
    );
    println!("    next_action: {:?}", replay_feedback.next_action);
    println!("    repair_hint: {:?}", replay_feedback.repair_hint);
    logger.log_event(
        "worker-node",
        "stage-4",
        "phase_transition",
        json!({
            "stage": "[4] Worker replay decision",
            "used_capsule": decision.used_capsule,
            "fallback_to_planner": decision.fallback_to_planner,
            "reason": &decision.reason,
            "planner_directive": replay_feedback.planner_directive,
            "next_action": replay_feedback.next_action,
            "repair_hint": replay_feedback.repair_hint,
            "directive_route": format!("{:?}", directive_execution.route),
        }),
    );
    logger.log_event(
        "worker-node",
        "stage-4",
        "directive_consumed",
        json!({
            "route": format!("{:?}", directive_execution.route),
            "verification_hint": &directive_execution.verification_hint,
            "fallback_classification": &directive_execution.fallback_classification,
        }),
    );
    if !decision.used_capsule || decision.fallback_to_planner {
        logger.log_event(
            "worker-node",
            "stage-4",
            "error",
            json!({
                "reason": "replay did not hit required path",
                "used_capsule": decision.used_capsule,
                "fallback_to_planner": decision.fallback_to_planner,
            }),
        );
        return Err(format!(
            "replay did not hit required path: used_capsule={}, fallback_to_planner={}",
            decision.used_capsule, decision.fallback_to_planner
        )
        .into());
    }

    let agent = make_agent(
        "你是官方经验复用 Agent。你必须优先复用已导入经验，基于工具信息给出可执行、可验证、可回滚的故障修复方案。",
        logger.clone(),
        "worker-agent",
    )?;

    let primary_plan = generate_repair_plan(
        &agent,
        &logger,
        "worker-agent",
        "case-primary",
        "处理 unknown command 'process' + windows shell 不兼容导致的执行失败",
        &directive_execution,
    )
    .await?;
    fs::write(&paths.primary_plan_path, &primary_plan)?;
    let primary_quality = match repair_quality_gate(&primary_plan) {
        Ok(result) => result,
        Err(err) => {
            logger.log_event(
                "worker-agent",
                "stage-5",
                "error",
                json!({"reason": err.to_string()}),
            );
            return Err(err);
        }
    };
    println!("[5] Qwen repair plan generated");
    println!(
        "    primary_plan_path: {}",
        paths.primary_plan_path.display()
    );
    println!("    preview: {}", preview(&primary_plan, 220));
    logger.log_event(
        "worker-agent",
        "stage-5",
        "phase_transition",
        json!({
            "stage": "[5] Qwen repair plan generated",
            "primary_plan_path": paths.primary_plan_path.display().to_string(),
        }),
    );
    println!();

    let similar_plan = generate_repair_plan(
        &agent,
        &logger,
        "worker-agent",
        "case-similar",
        "处理 unknown command 'proccess' + Linux CI shell 参数风格冲突",
        &directive_execution,
    )
    .await?;
    fs::write(&paths.similar_plan_path, &similar_plan)?;
    let similar_quality = match repair_quality_gate(&similar_plan) {
        Ok(result) => result,
        Err(err) => {
            logger.log_event(
                "worker-agent",
                "stage-6",
                "error",
                json!({"reason": err.to_string()}),
            );
            return Err(err);
        }
    };
    println!("[6] Qwen similar-task repair generated");
    println!(
        "    similar_plan_path: {}",
        paths.similar_plan_path.display()
    );
    println!("    preview: {}", preview(&similar_plan, 220));
    logger.log_event(
        "worker-agent",
        "stage-6",
        "phase_transition",
        json!({
            "stage": "[6] Qwen similar-task repair generated",
            "similar_plan_path": paths.similar_plan_path.display().to_string(),
        }),
    );
    println!();

    let official_summary = summarize_event_file(&paths.official_store_root.join("events.jsonl"))?;
    let worker_summary = summarize_event_file(&paths.worker_store_root.join("events.jsonl"))?;
    write_events_summary(
        &paths.events_summary_path,
        &official_summary,
        &worker_summary,
        logger.human_path(),
        logger.jsonl_path(),
    )?;

    let asset_origin = strategy_value(&imported_gene.strategy, "asset_origin")
        .unwrap_or_else(|| "unknown".to_string());
    write_replay_evidence(
        &paths.replay_evidence_path,
        &paths.run_id,
        &imported_gene.id,
        &imported_capsule.id,
        &imported_capsule.mutation_id,
        &asset_origin,
        import.imported_asset_ids.len(),
        decision.used_capsule,
        decision.fallback_to_planner,
        &decision.reason,
        logger.human_path(),
        logger.jsonl_path(),
    )?;

    let realtime_summary = summarize_realtime_file(logger.jsonl_path())?;

    write_verification_report(
        &paths.report_path,
        &paths,
        &imported_gene.id,
        &imported_capsule.id,
        &imported_capsule.mutation_id,
        import.imported_asset_ids.len(),
        decision.used_capsule,
        decision.fallback_to_planner,
        &decision.reason,
        &primary_quality,
        &similar_quality,
        &official_summary,
        &worker_summary,
        &realtime_summary,
        logger.human_path(),
        logger.jsonl_path(),
    )?;

    println!("[7] Verification report");
    println!("    report: {}", paths.report_path.display());
    println!(
        "    events_summary: {}",
        paths.events_summary_path.display()
    );
    println!(
        "    replay_evidence: {}",
        paths.replay_evidence_path.display()
    );
    println!("    realtime_log: {}", logger.human_path().display());
    println!("    realtime_jsonl: {}", logger.jsonl_path().display());
    println!(
        "    official_store: {}",
        paths.official_store_root.display()
    );
    println!("    worker_store: {}", paths.worker_store_root.display());
    logger.log_event(
        "system",
        "stage-7",
        "phase_transition",
        json!({
            "stage": "[7] Verification report",
            "report": paths.report_path.display().to_string(),
            "token_chunk_events": realtime_summary.token_chunk_count,
            "tool_call_events": realtime_summary.tool_call_event_count,
        }),
    );

    Ok(())
}
