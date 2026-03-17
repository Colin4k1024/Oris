use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use oris_runtime::agent_contract::{AgentTask, MutationProposal, ProposalTarget};
use oris_runtime::economics::{EvuAccount, EvuLedger};
use oris_runtime::evolution::{
    EvoEnvFingerprint as EnvFingerprint, EvoKernel, EvoSelectorInput as SelectorInput,
};
use oris_runtime::evolution_network::EvolutionEnvelope;
use oris_runtime::language_models::{llm::LLM, options::CallOptions, TokenUsage};
use oris_runtime::llm::ollama::client::Ollama;
use oris_runtime::llm::openai::{OpenAI, OpenAIConfig};
use oris_runtime::llm::Deepseek;
use oris_runtime::schemas::Message;
use serde::{Deserialize, Serialize};
use tiktoken_rs::{get_bpe_from_tokenizer, tokenizer::Tokenizer, CoreBPE};

use crate::{build_demo_evo, current_git_head, proposal_for, ExampleResult, ExampleState};

const GROUP_NON_EVO: &str = "non_evo";
const GROUP_EVO: &str = "evo";
const PLANNER_DEEPSEEK: &str = "deepseek";
const PLANNER_OLLAMA: &str = "ollama";
const PLANNER_OPENAI_COMPAT: &str = "openai-compatible";
const SHAREABLE_ASSET_RUNTIME_ROOT: &str = "examples/evo_oris_repo/assets/evo-shareable/runtime";
const SHAREABLE_ASSET_GENERATED_ROOT: &str =
    "examples/evo_oris_repo/assets/evo-shareable/generated";
const DEFAULT_OPENAI_COMPAT_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_OPENAI_COMPAT_MODEL: &str = "qwen3-235b-a22b";

#[derive(Clone, Debug)]
pub struct BenchmarkConfig {
    pub planner: String,
    pub iterations: u32,
    pub model: String,
    pub planner_base_url: Option<String>,
    pub output_json: PathBuf,
    pub output_md: PathBuf,
    pub output_assets_json: PathBuf,
    pub log_file: PathBuf,
    pub allow_skip_non_evo: bool,
    pub verbose: bool,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            planner: PLANNER_OPENAI_COMPAT.to_string(),
            iterations: 10,
            model: DEFAULT_OPENAI_COMPAT_MODEL.to_string(),
            planner_base_url: Some(DEFAULT_OPENAI_COMPAT_BASE_URL.to_string()),
            output_json: PathBuf::from("target/evo_bench/report.json"),
            output_md: PathBuf::from("target/evo_bench/report.md"),
            output_assets_json: PathBuf::from("target/evo_bench/shareable_assets.json"),
            log_file: PathBuf::from("target/evo_bench/benchmark.log"),
            allow_skip_non_evo: true,
            verbose: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchmarkTask {
    pub asset_id: String,
    pub label: String,
    pub target_path: String,
    pub task_description: String,
    pub expected_effect: String,
    pub diff_payload: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssetBundle {
    pub asset_id: String,
    pub gene_id: String,
    pub capsule_id: String,
    pub signals: Vec<String>,
    pub env: EnvFingerprint,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharingSummary {
    pub consumer_id: String,
    pub imported_asset_ids: usize,
    pub imported_record_ids: usize,
    pub replay_hits: usize,
    pub replay_total: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunRecord {
    pub group: String,
    pub iteration: u32,
    pub task_id: String,
    pub selected_asset_id: String,
    pub success: bool,
    pub duration_ms: u128,
    pub replay_hit: bool,
    pub fallback_used: bool,
    pub real_prompt_tokens: u32,
    pub real_completion_tokens: u32,
    pub real_total_tokens: u32,
    pub offline_prompt_tokens: u32,
    pub offline_completion_tokens: u32,
    pub offline_total_tokens: u32,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricSummary {
    pub sum: f64,
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GroupSummary {
    pub group: String,
    pub runs: usize,
    pub success_rate: f64,
    pub duration_ms: MetricSummary,
    pub real_tokens: MetricSummary,
    pub offline_tokens: MetricSummary,
    pub replay_hit_rate: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComparisonDelta {
    pub real_token_reduction_pct: f64,
    pub offline_token_reduction_pct: f64,
    pub duration_reduction_pct: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub generated_at_unix_ms: u128,
    pub run_id: String,
    pub planner: String,
    pub iterations: u32,
    pub model: String,
    pub sharing_topology: String,
    pub allow_skip_non_evo: bool,
    pub baseline_status: String,
    pub asset_manifest_path: String,
    pub envelope_path: String,
    pub log_path: String,
    pub tasks: Vec<BenchmarkTask>,
    pub task_ids: Vec<String>,
    pub created_assets: Vec<AssetBundle>,
    pub sharing: Vec<SharingSummary>,
    pub group_summaries: Vec<GroupSummary>,
    pub comparison: Option<ComparisonDelta>,
    pub runs: Vec<RunRecord>,
}

#[derive(Clone, Debug)]
struct PlannerChoice {
    selected_asset_id: String,
    real_tokens: TokenUsage,
    offline_tokens: TokenUsage,
    note: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct ShareableAssetInfo {
    asset_id: String,
    target_path: String,
    gene_id: String,
    capsule_id: String,
    signals: Vec<String>,
    env: EnvFingerprint,
}

#[derive(Clone, Debug, Serialize)]
struct ShareableAssetsManifest {
    generated_at_unix_ms: u128,
    run_id: String,
    asset_root: String,
    envelope_path: String,
    planner: String,
    model: String,
    assets: Vec<ShareableAssetInfo>,
    sharing: Vec<SharingSummary>,
}

#[derive(Clone, Debug, Serialize)]
struct EnvelopeSnapshot<'a> {
    generated_at_unix_ms: u128,
    run_id: String,
    planner: String,
    model: String,
    envelope: &'a EvolutionEnvelope,
}

#[derive(Clone, Debug)]
enum PlannerBackend {
    Deepseek { api_key: String },
    Ollama,
    OpenAICompat { api_key: String, base_url: String },
}

impl PlannerBackend {
    fn name(&self) -> &'static str {
        match self {
            Self::Deepseek { .. } => PLANNER_DEEPSEEK,
            Self::Ollama => PLANNER_OLLAMA,
            Self::OpenAICompat { .. } => PLANNER_OPENAI_COMPAT,
        }
    }
}

#[derive(Clone)]
struct BenchLogger {
    run_id: String,
    verbose: bool,
    log_path: PathBuf,
    file: Arc<Mutex<fs::File>>,
}

impl BenchLogger {
    fn new(run_id: String, verbose: bool, log_path: PathBuf) -> ExampleResult<Self> {
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        Ok(Self {
            run_id,
            verbose,
            log_path,
            file: Arc::new(Mutex::new(file)),
        })
    }

    fn path_str(&self) -> String {
        self.log_path.to_string_lossy().to_string()
    }

    fn log(
        &self,
        status: &str,
        stage: &str,
        group: Option<&str>,
        iteration: Option<u32>,
        task_id: Option<&str>,
        duration_ms: Option<u128>,
        replay_hit: Option<bool>,
        fallback_used: Option<bool>,
        token_real: Option<u32>,
        token_offline: Option<u32>,
        reason: Option<&str>,
    ) -> ExampleResult<()> {
        let fields = vec![
            format!("run_id={}", self.run_id),
            format!("status={status}"),
            format!("stage={stage}"),
            format!("group={}", group.unwrap_or("-")),
            format!(
                "iteration={}",
                iteration
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".into())
            ),
            format!("task_id={}", task_id.unwrap_or("-")),
            format!(
                "duration_ms={}",
                duration_ms
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".into())
            ),
            format!(
                "replay_hit={}",
                replay_hit
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".into())
            ),
            format!(
                "fallback_used={}",
                fallback_used
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".into())
            ),
            format!(
                "token_real={}",
                token_real
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".into())
            ),
            format!(
                "token_offline={}",
                token_offline
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".into())
            ),
            format!(
                "reason={}",
                reason.map(sanitize_log_field).unwrap_or_else(|| "-".into())
            ),
        ];

        let line = format!("[evo-bench] {}", fields.join(" "));
        {
            let mut file = self
                .file
                .lock()
                .map_err(|_| "failed to lock benchmark log file mutex")?;
            writeln!(file, "{line}")?;
        }
        if self.verbose {
            println!("{line}");
        }
        Ok(())
    }
}

struct OfflineTokenEstimator {
    bpe: CoreBPE,
}

impl OfflineTokenEstimator {
    fn new() -> ExampleResult<Self> {
        let bpe = get_bpe_from_tokenizer(Tokenizer::Cl100kBase)
            .map_err(|err| format!("failed to initialize cl100k_base tokenizer: {err}"))?;
        Ok(Self { bpe })
    }

    fn estimate(&self, prompt: &str, completion: &str) -> TokenUsage {
        let prompt_tokens = self.bpe.encode_ordinary(prompt).len() as u32;
        let completion_tokens = self.bpe.encode_ordinary(completion).len() as u32;
        TokenUsage::new(prompt_tokens, completion_tokens)
    }
}

pub async fn run_evo_vs_non_evo_benchmark(
    config: &BenchmarkConfig,
) -> ExampleResult<BenchmarkReport> {
    let run_id = format!("run-{}-{}", now_unix_ms(), std::process::id());
    let source_label = format!("benchmark-source-{run_id}");
    let consumer_a_label = format!("benchmark-consumer-a-{run_id}");
    let consumer_b_label = format!("benchmark-consumer-b-{run_id}");
    let baseline_label = format!("benchmark-baseline-{run_id}");
    let logger = BenchLogger::new(run_id.clone(), config.verbose, config.log_file.clone())?;
    logger.log(
        "START",
        "benchmark",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(&format!(
            "planner={} model={} iterations={}",
            config.planner, config.model, config.iterations
        )),
    )?;

    let output_root = config
        .output_json
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("target/evo_bench"));
    let canonical_envelope_path = output_root.join("shareable_envelope.json");
    let snapshot_root = output_root.join("runs").join(&run_id);
    let snapshot_assets_path = snapshot_root.join("shareable_assets.json");
    let snapshot_envelope_path = snapshot_root.join("shareable_envelope.json");
    let visible_asset_root = PathBuf::from(SHAREABLE_ASSET_GENERATED_ROOT).join(&run_id);

    let workspace_root = std::env::current_dir()?;
    let base_revision = current_git_head(&workspace_root);
    let tasks = benchmark_tasks(&run_id);
    logger.log(
        "START",
        "prepare_tasks",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(&format!(
            "task_count={} asset_root={}",
            tasks.len(),
            visible_asset_root.to_string_lossy()
        )),
    )?;
    let task_by_id = tasks
        .iter()
        .cloned()
        .map(|task| (task.asset_id.clone(), task))
        .collect::<BTreeMap<_, _>>();

    let source_economics = Arc::new(Mutex::new(EvuLedger {
        accounts: vec![EvuAccount {
            node_id: "source-agent".into(),
            balance: 16,
        }],
        reputations: vec![],
    }));
    let source = build_demo_evo(&source_label, 1)?.with_economics(source_economics);
    let created_assets = seed_assets(&source, &tasks, base_revision.clone(), &logger).await?;
    logger.log(
        "DONE",
        "seed_assets",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(&format!("asset_count={}", created_assets.len())),
    )?;

    materialize_visible_assets(
        &tasks,
        &created_assets,
        &visible_asset_root,
        &run_id,
        &logger,
    )?;
    println!(
        "[evo-bench] visible shareable assets root: {}",
        visible_asset_root.to_string_lossy()
    );

    logger.log(
        "START",
        "export_assets",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some("exporting promoted assets from source-agent"),
    )?;
    let envelope = source.export_promoted_assets("source-agent")?;
    logger.log(
        "DONE",
        "export_assets",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some("exported promoted assets from source-agent"),
    )?;

    write_envelope_snapshots(
        &run_id,
        config,
        &envelope,
        &canonical_envelope_path,
        &snapshot_envelope_path,
        &logger,
    )?;

    let consumer_a = build_demo_evo(&consumer_a_label, 1)?;
    let consumer_b = build_demo_evo(&consumer_b_label, 1)?;

    logger.log(
        "START",
        "import_assets",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some("importing envelope into consumer agents"),
    )?;
    let import_a = consumer_a.import_remote_envelope(&envelope)?;
    let import_b = consumer_b.import_remote_envelope(&envelope)?;
    logger.log(
        "DONE",
        "import_assets",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(&format!(
            "consumer-a_records={} consumer-b_records={}",
            import_a.imported_asset_ids.len(),
            import_b.imported_asset_ids.len()
        )),
    )?;

    let sharing_a = replay_shared_assets(
        &consumer_a,
        "consumer-a",
        &created_assets,
        &import_a.imported_asset_ids,
        &logger,
    )
    .await?;
    let sharing_b = replay_shared_assets(
        &consumer_b,
        "consumer-b",
        &created_assets,
        &import_b.imported_asset_ids,
        &logger,
    )
    .await?;
    logger.log(
        "DONE",
        "sharing_preflight",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(&format!(
            "consumer-a={}/{} consumer-b={}/{}",
            sharing_a.replay_hits,
            sharing_a.replay_total,
            sharing_b.replay_hits,
            sharing_b.replay_total
        )),
    )?;
    write_shareable_assets_manifest(
        &run_id,
        config,
        &created_assets,
        &canonical_envelope_path,
        &config.output_assets_json,
        &snapshot_assets_path,
        &[sharing_a.clone(), sharing_b.clone()],
        &logger,
    )?;

    println!("[evo-bench] evidence paths:");
    println!("- asset_root: {}", visible_asset_root.to_string_lossy());
    println!(
        "- asset_manifest: {}",
        config.output_assets_json.to_string_lossy()
    );
    println!("- envelope: {}", canonical_envelope_path.to_string_lossy());
    println!("- log_file: {}", logger.path_str());

    let mut all_runs = Vec::<RunRecord>::new();
    let estimator = OfflineTokenEstimator::new()?;
    let (planner_backend, baseline_status, planner_unavailable_reason) = resolve_planner(config)?;
    let baseline_log_status = if planner_backend.is_some() {
        "DONE"
    } else {
        "SKIP"
    };
    logger.log(
        baseline_log_status,
        "baseline_gate",
        Some(GROUP_NON_EVO),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(&baseline_status),
    )?;

    if let Some(planner_backend) = planner_backend.as_ref() {
        let baseline = build_demo_evo(&baseline_label, 1)?;
        for iteration in 1..=config.iterations {
            logger.log(
                "START",
                "baseline_task_batch",
                Some(GROUP_NON_EVO),
                Some(iteration),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )?;
            for task in &tasks {
                let started = Instant::now();
                logger.log(
                    "START",
                    "baseline_task",
                    Some(GROUP_NON_EVO),
                    Some(iteration),
                    Some(&task.asset_id),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some("planner selection"),
                )?;
                let choice = choose_asset_with_planner(
                    planner_backend,
                    &config.model,
                    task,
                    &tasks,
                    &estimator,
                )
                .await?;

                let selected_task = task_by_id
                    .get(&choice.selected_asset_id)
                    .cloned()
                    .unwrap_or_else(|| task.clone());
                let mut note = choice.note.clone();
                if selected_task.asset_id != task.asset_id {
                    note = Some(format!(
                        "model selected asset '{}' for target '{}'",
                        selected_task.asset_id, task.asset_id
                    ));
                }
                let success = capture_task(
                    &baseline,
                    &selected_task,
                    &format!("baseline-iter{}-{}", iteration, task.asset_id),
                    base_revision.clone(),
                )
                .await;

                let note_for_log = note.clone();
                all_runs.push(RunRecord {
                    group: GROUP_NON_EVO.to_string(),
                    iteration,
                    task_id: task.asset_id.clone(),
                    selected_asset_id: selected_task.asset_id.clone(),
                    success,
                    duration_ms: started.elapsed().as_millis(),
                    replay_hit: false,
                    fallback_used: false,
                    real_prompt_tokens: choice.real_tokens.prompt_tokens,
                    real_completion_tokens: choice.real_tokens.completion_tokens,
                    real_total_tokens: choice.real_tokens.total_tokens,
                    offline_prompt_tokens: choice.offline_tokens.prompt_tokens,
                    offline_completion_tokens: choice.offline_tokens.completion_tokens,
                    offline_total_tokens: choice.offline_tokens.total_tokens,
                    note,
                });
                logger.log(
                    if success { "DONE" } else { "FAIL" },
                    "baseline_task",
                    Some(GROUP_NON_EVO),
                    Some(iteration),
                    Some(&task.asset_id),
                    Some(started.elapsed().as_millis()),
                    Some(false),
                    Some(false),
                    Some(choice.real_tokens.total_tokens),
                    Some(choice.offline_tokens.total_tokens),
                    note_for_log.as_deref(),
                )?;
            }
        }
    }

    for iteration in 1..=config.iterations {
        logger.log(
            "START",
            "evo_task_batch",
            Some(GROUP_EVO),
            Some(iteration),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )?;
        for bundle in &created_assets {
            let task = task_by_id
                .get(&bundle.asset_id)
                .ok_or_else(|| format!("task not found for asset {}", bundle.asset_id))?;
            let started = Instant::now();
            let decision = consumer_a
                .replay_or_fallback_for_run(
                    &format!("evo-iter{}-{}", iteration, task.asset_id),
                    SelectorInput {
                        signals: focused_replay_signals(bundle),
                        env: bundle.env.clone(),
                        spec_id: None,
                        limit: 1,
                    },
                )
                .await?;
            logger.log(
                "START",
                "evo_task",
                Some(GROUP_EVO),
                Some(iteration),
                Some(&task.asset_id),
                None,
                Some(decision.used_capsule),
                None,
                None,
                None,
                Some(&decision.reason),
            )?;

            let mut success = decision.used_capsule;
            let mut fallback_used = false;
            let mut selected_asset_id = task.asset_id.clone();
            let mut real_tokens = TokenUsage::default();
            let mut offline_tokens = TokenUsage::default();
            let mut note = Some(decision.reason.clone());

            if !decision.used_capsule {
                fallback_used = true;
                if let Some(planner_backend) = planner_backend.as_ref() {
                    logger.log(
                        "START",
                        "fallback",
                        Some(GROUP_EVO),
                        Some(iteration),
                        Some(&task.asset_id),
                        None,
                        Some(false),
                        Some(true),
                        None,
                        None,
                        Some("planner selection"),
                    )?;
                    let choice = choose_asset_with_planner(
                        planner_backend,
                        &config.model,
                        task,
                        &tasks,
                        &estimator,
                    )
                    .await?;
                    real_tokens = choice.real_tokens;
                    offline_tokens = choice.offline_tokens;
                    selected_asset_id = choice.selected_asset_id.clone();
                    let selected_task = task_by_id
                        .get(&choice.selected_asset_id)
                        .cloned()
                        .unwrap_or_else(|| task.clone());
                    success = capture_task(
                        &consumer_a,
                        &selected_task,
                        &format!("evo-fallback-iter{}-{}", iteration, task.asset_id),
                        base_revision.clone(),
                    )
                    .await;
                    note = choice.note.or_else(|| {
                        Some(format!(
                            "fallback executed with {} planning",
                            planner_backend.name()
                        ))
                    });
                    logger.log(
                        if success { "DONE" } else { "FAIL" },
                        "fallback",
                        Some(GROUP_EVO),
                        Some(iteration),
                        Some(&task.asset_id),
                        Some(started.elapsed().as_millis()),
                        Some(false),
                        Some(true),
                        Some(real_tokens.total_tokens),
                        Some(offline_tokens.total_tokens),
                        note.as_deref(),
                    )?;
                } else {
                    success = false;
                    note = planner_unavailable_reason.clone().or_else(|| {
                        Some("fallback required planner but planner unavailable".to_string())
                    });
                    logger.log(
                        "SKIP",
                        "fallback",
                        Some(GROUP_EVO),
                        Some(iteration),
                        Some(&task.asset_id),
                        Some(started.elapsed().as_millis()),
                        Some(false),
                        Some(true),
                        Some(0),
                        Some(0),
                        note.as_deref(),
                    )?;
                }
            }

            let note_for_log = note.clone();
            all_runs.push(RunRecord {
                group: GROUP_EVO.to_string(),
                iteration,
                task_id: task.asset_id.clone(),
                selected_asset_id,
                success,
                duration_ms: started.elapsed().as_millis(),
                replay_hit: decision.used_capsule,
                fallback_used,
                real_prompt_tokens: real_tokens.prompt_tokens,
                real_completion_tokens: real_tokens.completion_tokens,
                real_total_tokens: real_tokens.total_tokens,
                offline_prompt_tokens: offline_tokens.prompt_tokens,
                offline_completion_tokens: offline_tokens.completion_tokens,
                offline_total_tokens: offline_tokens.total_tokens,
                note,
            });
            logger.log(
                if success { "DONE" } else { "FAIL" },
                "evo_task",
                Some(GROUP_EVO),
                Some(iteration),
                Some(&task.asset_id),
                Some(started.elapsed().as_millis()),
                Some(decision.used_capsule),
                Some(fallback_used),
                Some(real_tokens.total_tokens),
                Some(offline_tokens.total_tokens),
                note_for_log.as_deref(),
            )?;
        }
    }

    let non_evo_runs = all_runs
        .iter()
        .filter(|record| record.group == GROUP_NON_EVO)
        .cloned()
        .collect::<Vec<_>>();
    let evo_runs = all_runs
        .iter()
        .filter(|record| record.group == GROUP_EVO)
        .cloned()
        .collect::<Vec<_>>();

    let mut group_summaries = Vec::new();
    if !non_evo_runs.is_empty() {
        group_summaries.push(summarize_group(GROUP_NON_EVO, &non_evo_runs, false));
    }
    group_summaries.push(summarize_group(GROUP_EVO, &evo_runs, true));

    let comparison = if let (Some(non_evo), Some(evo)) = (
        group_summaries
            .iter()
            .find(|summary| summary.group == GROUP_NON_EVO),
        group_summaries
            .iter()
            .find(|summary| summary.group == GROUP_EVO),
    ) {
        Some(ComparisonDelta {
            real_token_reduction_pct: reduction_pct(non_evo.real_tokens.mean, evo.real_tokens.mean),
            offline_token_reduction_pct: reduction_pct(
                non_evo.offline_tokens.mean,
                evo.offline_tokens.mean,
            ),
            duration_reduction_pct: reduction_pct(non_evo.duration_ms.mean, evo.duration_ms.mean),
        })
    } else {
        None
    };

    let report = BenchmarkReport {
        generated_at_unix_ms: now_unix_ms(),
        run_id: run_id.clone(),
        planner: config.planner.clone(),
        iterations: config.iterations,
        model: config.model.clone(),
        sharing_topology: "1-source-2-consumers".to_string(),
        allow_skip_non_evo: config.allow_skip_non_evo,
        baseline_status,
        asset_manifest_path: config.output_assets_json.to_string_lossy().to_string(),
        envelope_path: canonical_envelope_path.to_string_lossy().to_string(),
        log_path: logger.path_str(),
        tasks: tasks.clone(),
        task_ids: tasks.iter().map(|task| task.asset_id.clone()).collect(),
        created_assets,
        sharing: vec![sharing_a, sharing_b],
        group_summaries,
        comparison,
        runs: all_runs,
    };

    logger.log(
        "START",
        "report_write",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some("writing benchmark json/markdown report"),
    )?;
    write_report_outputs(config, &report)?;
    logger.log(
        "DONE",
        "report_write",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(&format!(
            "json={} md={} groups={} runs={}",
            config.output_json.display(),
            config.output_md.display(),
            report.group_summaries.len(),
            report.runs.len()
        )),
    )?;
    Ok(report)
}

fn resolve_planner(
    config: &BenchmarkConfig,
) -> ExampleResult<(Option<PlannerBackend>, String, Option<String>)> {
    let planner = config.planner.trim().to_ascii_lowercase();
    match planner.as_str() {
        PLANNER_DEEPSEEK => {
            let deepseek_key = std::env::var("DEEPSEEK_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty());
            if let Some(api_key) = deepseek_key {
                Ok((
                    Some(PlannerBackend::Deepseek { api_key }),
                    "executed".to_string(),
                    None,
                ))
            } else if config.allow_skip_non_evo {
                Ok((
                    None,
                    "skipped_missing_key".to_string(),
                    Some("DEEPSEEK_API_KEY missing; planner unavailable".to_string()),
                ))
            } else {
                Err("DEEPSEEK_API_KEY missing and --allow-skip-non-evo=false".into())
            }
        }
        PLANNER_OLLAMA => Ok((Some(PlannerBackend::Ollama), "executed".to_string(), None)),
        PLANNER_OPENAI_COMPAT => {
            let api_key = std::env::var("OPENAI_COMPAT_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty());
            let base_url = config
                .planner_base_url
                .clone()
                .unwrap_or_else(|| DEFAULT_OPENAI_COMPAT_BASE_URL.to_string());

            if let Some(api_key) = api_key {
                Ok((
                    Some(PlannerBackend::OpenAICompat { api_key, base_url }),
                    "executed".to_string(),
                    None,
                ))
            } else if config.allow_skip_non_evo {
                Ok((
                    None,
                    "skipped_missing_key".to_string(),
                    Some("OPENAI_COMPAT_API_KEY missing; planner unavailable".to_string()),
                ))
            } else {
                Err("OPENAI_COMPAT_API_KEY missing and --allow-skip-non-evo=false".into())
            }
        }
        other => Err(format!(
            "unknown planner '{}' ; expected one of: {}, {}, {}",
            other, PLANNER_DEEPSEEK, PLANNER_OLLAMA, PLANNER_OPENAI_COMPAT
        )
        .into()),
    }
}

async fn seed_assets(
    source: &EvoKernel<ExampleState>,
    tasks: &[BenchmarkTask],
    base_revision: Option<String>,
    logger: &BenchLogger,
) -> ExampleResult<Vec<AssetBundle>> {
    let mut bundles = Vec::new();

    for task in tasks {
        logger.log(
            "START",
            "seed_asset",
            None,
            None,
            Some(&task.asset_id),
            None,
            None,
            None,
            None,
            None,
            Some(&format!("target_path={}", task.target_path)),
        )?;
        let proposal = benchmark_proposal(task, "benchmark-source");
        let capture = source
            .capture_from_proposal(
                &format!("seed-{}", task.asset_id),
                &proposal,
                task.diff_payload.clone(),
                base_revision.clone(),
            )
            .await?;
        logger.log(
            "DONE",
            "seed_asset",
            None,
            None,
            Some(&task.asset_id),
            None,
            None,
            None,
            None,
            None,
            Some(&format!(
                "gene_id={} capsule_id={}",
                capture.gene.id, capture.capsule.id
            )),
        )?;
        bundles.push(AssetBundle {
            asset_id: task.asset_id.clone(),
            gene_id: capture.gene.id,
            capsule_id: capture.capsule.id,
            signals: capture.gene.signals,
            env: capture.capsule.env,
        });
    }

    Ok(bundles)
}

async fn replay_shared_assets(
    consumer: &EvoKernel<ExampleState>,
    consumer_id: &str,
    bundles: &[AssetBundle],
    imported_record_ids: &[String],
    logger: &BenchLogger,
) -> ExampleResult<SharingSummary> {
    let mut replay_hits = 0usize;
    let imported_set = imported_record_ids.iter().cloned().collect::<BTreeSet<_>>();
    let imported_asset_ids = bundles
        .iter()
        .filter(|bundle| {
            imported_set.contains(&bundle.gene_id) || imported_set.contains(&bundle.capsule_id)
        })
        .count();
    logger.log(
        "START",
        "sharing_preflight",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(&format!(
            "consumer_id={} imported_records={} mapped_task_assets={}",
            consumer_id,
            imported_record_ids.len(),
            imported_asset_ids
        )),
    )?;

    for bundle in bundles {
        logger.log(
            "START",
            "sharing_preflight_asset",
            None,
            None,
            Some(&bundle.asset_id),
            None,
            None,
            None,
            None,
            None,
            Some(&format!("consumer_id={consumer_id}")),
        )?;
        let decision = consumer
            .replay_or_fallback_for_run(
                &format!("{consumer_id}-preflight-{}", bundle.asset_id),
                SelectorInput {
                    signals: focused_replay_signals(bundle),
                    env: bundle.env.clone(),
                    spec_id: None,
                    limit: 1,
                },
            )
            .await?;
        if decision.used_capsule {
            replay_hits += 1;
        }
        logger.log(
            if decision.used_capsule {
                "DONE"
            } else {
                "FAIL"
            },
            "sharing_preflight_asset",
            None,
            None,
            Some(&bundle.asset_id),
            None,
            Some(decision.used_capsule),
            None,
            None,
            None,
            Some(&format!(
                "consumer_id={} reason={}",
                consumer_id, decision.reason
            )),
        )?;
    }

    Ok(SharingSummary {
        consumer_id: consumer_id.to_string(),
        imported_asset_ids,
        imported_record_ids: imported_record_ids.len(),
        replay_hits,
        replay_total: bundles.len(),
    })
}

fn focused_replay_signals(bundle: &AssetBundle) -> Vec<String> {
    let mut focused = bundle
        .signals
        .iter()
        .filter(|signal| {
            let trimmed = signal.trim();
            if trimmed.len() < 6 {
                return false;
            }
            if trimmed.chars().all(|ch| ch.is_ascii_digit()) {
                return false;
            }
            if trimmed == "100644"
                || trimmed == "0000000"
                || trimmed == "1111111"
                || trimmed == "validation passed"
            {
                return false;
            }
            trimmed.chars().any(|ch| ch.is_ascii_alphabetic())
        })
        .cloned()
        .collect::<Vec<_>>();

    focused.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
    focused.dedup();
    focused.truncate(3);

    if focused.is_empty() {
        bundle.signals.clone()
    } else {
        focused
    }
}

async fn choose_asset_with_planner(
    planner_backend: &PlannerBackend,
    model: &str,
    target: &BenchmarkTask,
    tasks: &[BenchmarkTask],
    estimator: &OfflineTokenEstimator,
) -> ExampleResult<PlannerChoice> {
    let task_list = tasks
        .iter()
        .map(|task| format!("- {}: {}", task.asset_id, task.label))
        .collect::<Vec<_>>()
        .join("\n");
    let system_prompt = "You are a deterministic planner. Return strict JSON only.";
    let user_prompt = format!(
        "Target task id: {}\nTarget description: {}\nAvailable assets:\n{}\nReturn JSON as: {{\"asset_id\":\"<one asset id>\"}}.",
        target.asset_id, target.task_description, task_list
    );

    let messages = vec![
        Message::new_system_message(system_prompt),
        Message::new_human_message(&user_prompt),
    ];
    let result = match planner_backend {
        PlannerBackend::Deepseek { api_key } => {
            let planner = Deepseek::new()
                .with_model(model.to_string())
                .with_api_key(api_key.to_string())
                .with_json_mode(true)
                .with_options(
                    CallOptions::default()
                        .with_temperature(0.0)
                        .with_max_tokens(64),
                );
            planner
                .generate(&messages)
                .await
                .map_err(|err| format!("deepseek planner request failed: {err}"))?
        }
        PlannerBackend::Ollama => {
            let planner = Ollama::default().with_model(model.to_string());
            planner.generate(&messages).await.map_err(|err| {
                format!(
                    "ollama planner request failed: {err}. ensure `ollama serve` is running and model `{model}` is available (for example: `ollama pull {model}`)"
                )
            })?
        }
        PlannerBackend::OpenAICompat { api_key, base_url } => {
            let config = OpenAIConfig::new()
                .with_api_key(api_key.clone())
                .with_api_base(base_url.clone());
            let planner = OpenAI::new(config)
                .with_model(model.to_string())
                .with_options(
                    CallOptions::default()
                        .with_temperature(0.0)
                        .with_max_tokens(64),
                );
            planner.generate(&messages).await.map_err(|err| {
                format!(
                    "openai-compatible planner request failed: {err}. check base_url `{base_url}` and model `{model}`"
                )
            })?
        }
    };
    let real_tokens = result.tokens.unwrap_or_default();
    let offline_tokens = estimator.estimate(
        &format!("{}\n{}", system_prompt, user_prompt),
        &result.generation,
    );

    let parsed = parse_asset_id(&result.generation);
    let mut note = None;
    let selected_asset_id = match parsed {
        Some(value) if tasks.iter().any(|task| task.asset_id == value) => value,
        Some(value) => {
            note = Some(format!(
                "planner returned unknown asset '{}' ; fallback to target '{}'",
                value, target.asset_id
            ));
            target.asset_id.clone()
        }
        None => {
            note = Some(format!(
                "planner response parse failed; fallback to target '{}'",
                target.asset_id
            ));
            target.asset_id.clone()
        }
    };

    Ok(PlannerChoice {
        selected_asset_id,
        real_tokens,
        offline_tokens,
        note,
    })
}

fn parse_asset_id(raw: &str) -> Option<String> {
    let trimmed = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return value
            .get("asset_id")
            .and_then(|value| value.as_str())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
    }

    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    let candidate = &trimmed[start..=end];
    serde_json::from_str::<serde_json::Value>(candidate)
        .ok()?
        .get("asset_id")
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

async fn capture_task(
    evo: &EvoKernel<ExampleState>,
    task: &BenchmarkTask,
    run_id: &str,
    base_revision: Option<String>,
) -> bool {
    let proposal = benchmark_proposal(task, "benchmark-runner");
    evo.capture_from_proposal(
        &run_id.to_string(),
        &proposal,
        task.diff_payload.clone(),
        base_revision,
    )
    .await
    .is_ok()
}

fn benchmark_proposal(task: &BenchmarkTask, source: &str) -> MutationProposal {
    let agent_task = AgentTask {
        id: format!("task-{}", task.asset_id),
        description: task.task_description.clone(),
    };
    let target = ProposalTarget::Paths(vec![task.target_path.clone()]);
    proposal_for(&agent_task, &target, source, &task.expected_effect)
}

fn benchmark_tasks(run_id: &str) -> Vec<BenchmarkTask> {
    let docs_path = format!("{SHAREABLE_ASSET_RUNTIME_ROOT}/{run_id}/docs/evo-bench-doc.md");
    let code_path = format!("{SHAREABLE_ASSET_RUNTIME_ROOT}/{run_id}/code/bench_helper.rs");
    let config_path = format!("{SHAREABLE_ASSET_RUNTIME_ROOT}/{run_id}/config/evo-bench-alert.yml");

    vec![
        BenchmarkTask {
            asset_id: "asset-doc".into(),
            label: "docs single-file patch".into(),
            target_path: docs_path.clone(),
            task_description: "Create a docs benchmark note for evo comparison".into(),
            expected_effect: "docs benchmark note added".into(),
            diff_payload: benchmark_diff(
                &docs_path,
                "Evo Bench Doc Asset",
                "This docs asset is used for benchmark replay.",
            ),
        },
        BenchmarkTask {
            asset_id: "asset-code".into(),
            label: "code helper patch".into(),
            target_path: code_path.clone(),
            task_description: "Add a benchmark helper module in example space".into(),
            expected_effect: "code helper file created".into(),
            diff_payload: benchmark_diff(
                &code_path,
                "Evo Bench Helper",
                "pub fn bench_helper_value() -> &'static str { \"evo-bench\" }",
            ),
        },
        BenchmarkTask {
            asset_id: "asset-config".into(),
            label: "observability config patch".into(),
            target_path: config_path.clone(),
            task_description: "Add an observability alert config snippet for benchmark".into(),
            expected_effect: "config benchmark file added".into(),
            diff_payload: benchmark_diff(
                &config_path,
                "Evo Bench Alert",
                "alert: EvoBenchReplayHitRate\nexpr: oris_evolution_replay_success_total > 0",
            ),
        },
    ]
}

fn task_visible_content(task: &BenchmarkTask, bundle: &AssetBundle, run_id: &str) -> String {
    match task.asset_id.as_str() {
        "asset-code" => format!(
            "// generated-by=evo-benchmark\n// run_id={run_id}\n// gene_id={}\n// capsule_id={}\n\npub fn bench_helper_value() -> &'static str {{\n    \"evo-bench\"\n}}\n",
            bundle.gene_id, bundle.capsule_id
        ),
        "asset-config" => format!(
            "# generated-by=evo-benchmark\n# run_id={run_id}\n# gene_id={}\n# capsule_id={}\nalert: EvoBenchReplayHitRate\nexpr: oris_evolution_replay_success_total > 0\n",
            bundle.gene_id, bundle.capsule_id
        ),
        _ => format!(
            "# Evo Bench Doc Asset\n\nThis docs asset is used for benchmark replay.\n\ngenerated-by=evo-benchmark\nrun_id={run_id}\ngene_id={}\ncapsule_id={}\n",
            bundle.gene_id, bundle.capsule_id
        ),
    }
}

fn visible_asset_path(run_id: &str, asset_id: &str) -> PathBuf {
    match asset_id {
        "asset-code" => PathBuf::from(format!(
            "{SHAREABLE_ASSET_GENERATED_ROOT}/{run_id}/code/bench_helper.rs"
        )),
        "asset-config" => PathBuf::from(format!(
            "{SHAREABLE_ASSET_GENERATED_ROOT}/{run_id}/config/evo-bench-alert.yml"
        )),
        _ => PathBuf::from(format!(
            "{SHAREABLE_ASSET_GENERATED_ROOT}/{run_id}/docs/evo-bench-doc.md"
        )),
    }
}

fn benchmark_diff(path: &str, title: &str, body: &str) -> String {
    let body_lines = body
        .lines()
        .map(|line| format!("+{line}\n"))
        .collect::<String>();
    let total_added_lines = 3 + body.lines().count();

    format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{total_added_lines} @@\n+# {title}\n+\n{body_lines}+generated-by=evo-benchmark\n"
    )
}

fn summarize_group(group: &str, runs: &[RunRecord], include_replay_rate: bool) -> GroupSummary {
    let duration_values = runs
        .iter()
        .map(|record| record.duration_ms as f64)
        .collect::<Vec<_>>();
    let real_token_values = runs
        .iter()
        .map(|record| record.real_total_tokens as f64)
        .collect::<Vec<_>>();
    let offline_token_values = runs
        .iter()
        .map(|record| record.offline_total_tokens as f64)
        .collect::<Vec<_>>();

    let success_count = runs.iter().filter(|record| record.success).count();
    let replay_hits = runs.iter().filter(|record| record.replay_hit).count();

    GroupSummary {
        group: group.to_string(),
        runs: runs.len(),
        success_rate: ratio(success_count, runs.len()),
        duration_ms: summarize_metric(&duration_values),
        real_tokens: summarize_metric(&real_token_values),
        offline_tokens: summarize_metric(&offline_token_values),
        replay_hit_rate: include_replay_rate.then(|| ratio(replay_hits, runs.len())),
    }
}

fn summarize_metric(values: &[f64]) -> MetricSummary {
    if values.is_empty() {
        return MetricSummary {
            sum: 0.0,
            mean: 0.0,
            p50: 0.0,
            p95: 0.0,
        };
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));

    let sum = sorted.iter().sum::<f64>();
    let mean = sum / sorted.len() as f64;

    MetricSummary {
        sum,
        mean,
        p50: percentile(&sorted, 0.50),
        p95: percentile(&sorted, 0.95),
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

fn reduction_pct(baseline: f64, evolved: f64) -> f64 {
    if baseline <= f64::EPSILON {
        0.0
    } else {
        ((baseline - evolved) / baseline) * 100.0
    }
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn sanitize_log_field(raw: &str) -> String {
    raw.replace('\n', "\\n").replace(' ', "_")
}

fn write_report_outputs(config: &BenchmarkConfig, report: &BenchmarkReport) -> ExampleResult<()> {
    if let Some(parent) = config.output_json.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = config.output_md.parent() {
        fs::create_dir_all(parent)?;
    }

    let json = serde_json::to_string_pretty(report)?;
    fs::write(&config.output_json, json)?;

    let markdown = render_markdown_report(report, config);
    fs::write(&config.output_md, markdown)?;

    Ok(())
}

fn materialize_visible_assets(
    tasks: &[BenchmarkTask],
    bundles: &[AssetBundle],
    visible_asset_root: &Path,
    run_id: &str,
    logger: &BenchLogger,
) -> ExampleResult<()> {
    fs::create_dir_all(visible_asset_root.join("docs"))?;
    fs::create_dir_all(visible_asset_root.join("code"))?;
    fs::create_dir_all(visible_asset_root.join("config"))?;

    let task_by_id = tasks
        .iter()
        .map(|task| (task.asset_id.clone(), task.clone()))
        .collect::<BTreeMap<_, _>>();

    for bundle in bundles {
        let task = task_by_id
            .get(&bundle.asset_id)
            .ok_or_else(|| format!("missing task for asset {}", bundle.asset_id))?;
        let visible_path = visible_asset_path(run_id, &bundle.asset_id);
        if let Some(parent) = visible_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = task_visible_content(task, bundle, run_id);
        fs::write(&visible_path, content)?;
        println!(
            "[evo-bench][asset] asset_id={} target_path={} gene_id={} capsule_id={} signals_count={}",
            bundle.asset_id,
            visible_path.to_string_lossy(),
            bundle.gene_id,
            bundle.capsule_id,
            bundle.signals.len()
        );
        logger.log(
            "DONE",
            "materialize_asset",
            None,
            None,
            Some(&bundle.asset_id),
            None,
            None,
            None,
            None,
            None,
            Some(&format!(
                "target_path={} gene_id={} capsule_id={} signals_count={}",
                visible_path.to_string_lossy(),
                bundle.gene_id,
                bundle.capsule_id,
                bundle.signals.len()
            )),
        )?;
    }

    Ok(())
}

fn write_envelope_snapshots(
    run_id: &str,
    config: &BenchmarkConfig,
    envelope: &EvolutionEnvelope,
    canonical_envelope_path: &Path,
    snapshot_envelope_path: &Path,
    logger: &BenchLogger,
) -> ExampleResult<()> {
    logger.log(
        "START",
        "envelope_write",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some("writing shareable envelope snapshots"),
    )?;
    if let Some(parent) = canonical_envelope_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = snapshot_envelope_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let snapshot = EnvelopeSnapshot {
        generated_at_unix_ms: now_unix_ms(),
        run_id: run_id.to_string(),
        planner: config.planner.clone(),
        model: config.model.clone(),
        envelope,
    };
    let payload = serde_json::to_string_pretty(&snapshot)?;
    fs::write(canonical_envelope_path, &payload)?;
    fs::write(snapshot_envelope_path, payload)?;
    logger.log(
        "DONE",
        "envelope_write",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(&format!(
            "canonical={} snapshot={}",
            canonical_envelope_path.to_string_lossy(),
            snapshot_envelope_path.to_string_lossy()
        )),
    )?;
    Ok(())
}

fn write_shareable_assets_manifest(
    run_id: &str,
    config: &BenchmarkConfig,
    bundles: &[AssetBundle],
    canonical_envelope_path: &Path,
    canonical_assets_path: &Path,
    snapshot_assets_path: &Path,
    sharing: &[SharingSummary],
    logger: &BenchLogger,
) -> ExampleResult<()> {
    logger.log(
        "START",
        "manifest_write",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some("writing shareable asset manifests"),
    )?;
    if let Some(parent) = canonical_assets_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = snapshot_assets_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let assets = bundles
        .iter()
        .map(|bundle| ShareableAssetInfo {
            asset_id: bundle.asset_id.clone(),
            target_path: visible_asset_path(run_id, &bundle.asset_id)
                .to_string_lossy()
                .to_string(),
            gene_id: bundle.gene_id.clone(),
            capsule_id: bundle.capsule_id.clone(),
            signals: bundle.signals.clone(),
            env: bundle.env.clone(),
        })
        .collect::<Vec<_>>();
    let manifest = ShareableAssetsManifest {
        generated_at_unix_ms: now_unix_ms(),
        run_id: run_id.to_string(),
        asset_root: format!("{SHAREABLE_ASSET_GENERATED_ROOT}/{run_id}"),
        envelope_path: canonical_envelope_path.to_string_lossy().to_string(),
        planner: config.planner.clone(),
        model: config.model.clone(),
        assets,
        sharing: sharing.to_vec(),
    };

    let json = serde_json::to_string_pretty(&manifest)?;
    fs::write(canonical_assets_path, &json)?;
    fs::write(snapshot_assets_path, json)?;
    logger.log(
        "DONE",
        "manifest_write",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(&format!(
            "canonical={} snapshot={}",
            canonical_assets_path.to_string_lossy(),
            snapshot_assets_path.to_string_lossy()
        )),
    )?;
    Ok(())
}

fn render_markdown_report(report: &BenchmarkReport, config: &BenchmarkConfig) -> String {
    let mut out = String::new();
    out.push_str("# Evo vs Non-Evo Benchmark Report\n\n");
    out.push_str(&format!(
        "- generated_at_unix_ms: `{}`\n",
        report.generated_at_unix_ms
    ));
    out.push_str(&format!("- run_id: `{}`\n", report.run_id));
    out.push_str(&format!("- planner: `{}`\n", report.planner));
    out.push_str(&format!("- model: `{}`\n", report.model));
    out.push_str(&format!("- iterations: `{}`\n", report.iterations));
    out.push_str(&format!(
        "- sharing_topology: `{}`\n",
        report.sharing_topology
    ));
    out.push_str(&format!(
        "- baseline_status: `{}`\n",
        report.baseline_status
    ));
    out.push_str(&format!(
        "- created_assets: `{}`\n",
        report.created_assets.len()
    ));
    out.push_str(&format!("- tasks: `{}`\n\n", report.task_ids.join(", ")));

    out.push_str("## Asset Sharing\n\n");
    out.push_str(
        "| Consumer | Imported Task Assets | Imported Records | Replay Hits | Replay Total |\n",
    );
    out.push_str("| --- | ---: | ---: | ---: | ---: |\n");
    for item in &report.sharing {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            item.consumer_id,
            item.imported_asset_ids,
            item.imported_record_ids,
            item.replay_hits,
            item.replay_total
        ));
    }
    out.push('\n');

    out.push_str("## Group Summaries\n\n");
    out.push_str("| Group | Runs | Success Rate | Duration Sum(ms) | Duration Mean(ms) | Duration P50(ms) | Duration P95(ms) | Real Tokens Sum | Real Tokens Mean | Real Tokens P50 | Real Tokens P95 | Offline Tokens Sum | Offline Tokens Mean | Offline Tokens P50 | Offline Tokens P95 | Replay Hit Rate |\n");
    out.push_str("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    for summary in &report.group_summaries {
        out.push_str(&format!(
            "| {} | {} | {:.2}% | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {} |\n",
            summary.group,
            summary.runs,
            summary.success_rate * 100.0,
            summary.duration_ms.sum,
            summary.duration_ms.mean,
            summary.duration_ms.p50,
            summary.duration_ms.p95,
            summary.real_tokens.sum,
            summary.real_tokens.mean,
            summary.real_tokens.p50,
            summary.real_tokens.p95,
            summary.offline_tokens.sum,
            summary.offline_tokens.mean,
            summary.offline_tokens.p50,
            summary.offline_tokens.p95,
            summary
                .replay_hit_rate
                .map(|value| format!("{:.2}%", value * 100.0))
                .unwrap_or_else(|| "n/a".to_string())
        ));
    }
    out.push('\n');

    out.push_str("## Comparison\n\n");
    match &report.comparison {
        Some(delta) => {
            out.push_str(&format!(
                "- real_token_reduction_pct: `{:.2}%`\n",
                delta.real_token_reduction_pct
            ));
            out.push_str(&format!(
                "- offline_token_reduction_pct: `{:.2}%`\n",
                delta.offline_token_reduction_pct
            ));
            out.push_str(&format!(
                "- duration_reduction_pct: `{:.2}%`\n",
                delta.duration_reduction_pct
            ));
        }
        None => {
            out.push_str("- comparison unavailable (baseline group skipped)\n");
        }
    }
    out.push('\n');

    out.push_str("## Output Paths\n\n");
    out.push_str(&format!(
        "- json: `{}`\n",
        config.output_json.to_string_lossy()
    ));
    out.push_str(&format!(
        "- markdown: `{}`\n",
        config.output_md.to_string_lossy()
    ));
    out.push_str(&format!(
        "- asset_manifest: `{}`\n",
        report.asset_manifest_path
    ));
    out.push_str(&format!("- envelope: `{}`\n", report.envelope_path));
    out.push_str(&format!("- log: `{}`\n", report.log_path));

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_summary_percentiles_are_stable() {
        let summary = summarize_metric(&[10.0, 20.0, 30.0, 40.0, 50.0]);
        assert_eq!(summary.sum, 150.0);
        assert_eq!(summary.mean, 30.0);
        assert_eq!(summary.p50, 30.0);
        assert_eq!(summary.p95, 50.0);
    }

    #[test]
    fn markdown_contains_required_sections() {
        let config = BenchmarkConfig::default();
        let report = BenchmarkReport {
            generated_at_unix_ms: 1,
            run_id: "run-1".into(),
            planner: PLANNER_OPENAI_COMPAT.into(),
            iterations: 10,
            model: "deepseek-chat".into(),
            sharing_topology: "1-source-2-consumers".into(),
            allow_skip_non_evo: true,
            baseline_status: "skipped_missing_key".into(),
            asset_manifest_path: "target/evo_bench/shareable_assets.json".into(),
            envelope_path: "target/evo_bench/shareable_envelope.json".into(),
            log_path: "target/evo_bench/benchmark.log".into(),
            tasks: vec![],
            task_ids: vec!["asset-doc".into()],
            created_assets: vec![],
            sharing: vec![SharingSummary {
                consumer_id: "consumer-a".into(),
                imported_asset_ids: 2,
                imported_record_ids: 4,
                replay_hits: 1,
                replay_total: 1,
            }],
            group_summaries: vec![GroupSummary {
                group: GROUP_EVO.into(),
                runs: 1,
                success_rate: 1.0,
                duration_ms: MetricSummary {
                    sum: 1.0,
                    mean: 1.0,
                    p50: 1.0,
                    p95: 1.0,
                },
                real_tokens: MetricSummary {
                    sum: 0.0,
                    mean: 0.0,
                    p50: 0.0,
                    p95: 0.0,
                },
                offline_tokens: MetricSummary {
                    sum: 0.0,
                    mean: 0.0,
                    p50: 0.0,
                    p95: 0.0,
                },
                replay_hit_rate: Some(1.0),
            }],
            comparison: None,
            runs: vec![],
        };

        let markdown = render_markdown_report(&report, &config);
        assert!(markdown.contains("# Evo vs Non-Evo Benchmark Report"));
        assert!(markdown.contains("## Asset Sharing"));
        assert!(markdown.contains("## Group Summaries"));
        assert!(markdown.contains("sharing_topology"));
        assert!(markdown.contains("asset_manifest"));
        assert!(markdown.contains("comparison unavailable"));
    }

    #[test]
    fn report_json_has_required_fields() {
        let report = BenchmarkReport {
            generated_at_unix_ms: 123,
            run_id: "run-2".into(),
            planner: PLANNER_OPENAI_COMPAT.into(),
            iterations: 1,
            model: "deepseek-chat".into(),
            sharing_topology: "1-source-2-consumers".into(),
            allow_skip_non_evo: true,
            baseline_status: "executed".into(),
            asset_manifest_path: "target/evo_bench/shareable_assets.json".into(),
            envelope_path: "target/evo_bench/shareable_envelope.json".into(),
            log_path: "target/evo_bench/benchmark.log".into(),
            tasks: vec![BenchmarkTask {
                asset_id: "asset-doc".into(),
                label: "docs".into(),
                target_path: format!(
                    "{SHAREABLE_ASSET_GENERATED_ROOT}/run-2/docs/evo-bench-doc.md"
                ),
                task_description: "desc".into(),
                expected_effect: "effect".into(),
                diff_payload: "diff".into(),
            }],
            task_ids: vec!["asset-doc".into()],
            created_assets: vec![AssetBundle {
                asset_id: "asset-doc".into(),
                gene_id: "gene-1".into(),
                capsule_id: "capsule-1".into(),
                signals: vec!["sig".into()],
                env: EnvFingerprint {
                    rustc_version: "rustc".into(),
                    cargo_lock_hash: "lock".into(),
                    target_triple: "x86_64-unknown-linux".into(),
                    os: "linux".into(),
                },
            }],
            sharing: vec![SharingSummary {
                consumer_id: "consumer-a".into(),
                imported_asset_ids: 1,
                imported_record_ids: 2,
                replay_hits: 1,
                replay_total: 1,
            }],
            group_summaries: vec![GroupSummary {
                group: GROUP_EVO.into(),
                runs: 1,
                success_rate: 1.0,
                duration_ms: MetricSummary {
                    sum: 1.0,
                    mean: 1.0,
                    p50: 1.0,
                    p95: 1.0,
                },
                real_tokens: MetricSummary {
                    sum: 0.0,
                    mean: 0.0,
                    p50: 0.0,
                    p95: 0.0,
                },
                offline_tokens: MetricSummary {
                    sum: 0.0,
                    mean: 0.0,
                    p50: 0.0,
                    p95: 0.0,
                },
                replay_hit_rate: Some(1.0),
            }],
            comparison: Some(ComparisonDelta {
                real_token_reduction_pct: 100.0,
                offline_token_reduction_pct: 100.0,
                duration_reduction_pct: 50.0,
            }),
            runs: vec![],
        };

        let json = serde_json::to_value(&report).expect("report must serialize");
        assert_eq!(
            json["sharing_topology"].as_str(),
            Some("1-source-2-consumers")
        );
        assert_eq!(json["planner"].as_str(), Some(PLANNER_OPENAI_COMPAT));
        assert_eq!(json["run_id"].as_str(), Some("run-2"));
        assert!(json["tasks"].is_array());
        assert!(json["created_assets"].is_array());
        assert!(json["sharing"].is_array());
        assert!(json["group_summaries"].is_array());
    }
}
