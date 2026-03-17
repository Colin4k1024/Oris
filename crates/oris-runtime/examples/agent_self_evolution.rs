//! Agent self-evolution demo (runtime-level, reproducible).
//!
//! Run:
//! `cargo run -p oris-runtime --example agent_self_evolution --features "full-evolution-experimental"`
//!
//! What this example shows:
//! 1. Detect a runtime anomaly from execution telemetry.
//! 2. Convert the anomaly into mutation signals and proposal metadata.
//! 3. Capture one successful mutation as reusable Gene/Capsule assets.
//! 4. Replay on a similar signal set (self-heal path) before fallback.

#[cfg(feature = "full-evolution-experimental")]
use std::path::{Path, PathBuf};
#[cfg(feature = "full-evolution-experimental")]
use std::sync::Arc;

#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::agent_contract::{AgentTask, MutationProposal};
#[cfg(feature = "full-evolution-experimental")]
use oris_runtime::evolution::{
    CommandValidator, EvoEvolutionStore as EvolutionStore, EvoKernel,
    EvoSandboxPolicy as SandboxPolicy, EvoSelectorInput as SelectorInput, JsonlEvolutionStore,
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
use serde::{Deserialize, Serialize};

#[cfg(not(feature = "full-evolution-experimental"))]
fn main() {
    eprintln!("This example requires feature `full-evolution-experimental`.\n");
    eprintln!(
        "Run: cargo run -p oris-runtime --example agent_self_evolution --features \"full-evolution-experimental\""
    );
}

#[cfg(feature = "full-evolution-experimental")]
type ExampleResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

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
#[derive(Clone, Debug)]
struct RuntimeSample {
    operation: &'static str,
    latency_ms: u64,
    retry_count: u32,
    success_ratio: f32,
}

#[cfg(feature = "full-evolution-experimental")]
#[derive(Clone, Debug)]
struct AnomalyReport {
    operation: String,
    reason: String,
    signals: Vec<String>,
    severity_score: f32,
}

#[cfg(feature = "full-evolution-experimental")]
fn detect_anomaly(samples: &[RuntimeSample]) -> Option<AnomalyReport> {
    samples
        .iter()
        .map(|sample| {
            // Severity combines latency, retries, and failure ratio.
            let latency_factor = (sample.latency_ms as f32 / 1200.0).min(2.0);
            let retry_factor = (sample.retry_count as f32 / 4.0).min(1.5);
            let failure_factor = (1.0 - sample.success_ratio).max(0.0) * 2.0;
            let severity = latency_factor + retry_factor + failure_factor;

            let mut signals = vec![
                "self_heal".to_string(),
                "latency_spike".to_string(),
                format!("op:{}", sample.operation.replace(' ', "_")),
            ];
            if sample.retry_count >= 2 {
                signals.push("retry_hotspot".to_string());
            }
            if sample.success_ratio < 0.85 {
                signals.push("degraded_success".to_string());
            }

            AnomalyReport {
                operation: sample.operation.to_string(),
                reason: format!(
                    "latency={}ms retries={} success_ratio={:.2}",
                    sample.latency_ms, sample.retry_count, sample.success_ratio
                ),
                signals,
                severity_score: severity,
            }
        })
        .max_by(|a, b| {
            a.severity_score
                .partial_cmp(&b.severity_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .filter(|report| report.severity_score >= 2.0)
}

#[cfg(feature = "full-evolution-experimental")]
fn demo_sandbox_policy() -> SandboxPolicy {
    SandboxPolicy {
        allowed_programs: vec!["cargo".into(), "git".into()],
        max_duration_ms: 180_000,
        max_output_bytes: 1_048_576,
        denied_env_prefixes: vec!["TOKEN".into(), "KEY".into(), "SECRET".into()],
        max_memory_bytes: None,
        max_cpu_secs: None,
        use_process_group: false,
    }
}

#[cfg(feature = "full-evolution-experimental")]
fn demo_validation_plan() -> ValidationPlan {
    ValidationPlan {
        profile: "self-evolution-validation".into(),
        stages: vec![ValidationStage::Command {
            program: "cargo".into(),
            args: vec![
                "check".into(),
                "-p".into(),
                "oris-runtime".into(),
                "--lib".into(),
                "--features".into(),
                "full-evolution-experimental".into(),
            ],
            timeout_ms: 180_000,
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
fn build_demo_evo(
    label: &str,
    promote_after_successes: u64,
) -> ExampleResult<(EvoKernel<ExampleState>, PathBuf, PathBuf)> {
    let workspace_root = std::env::current_dir()?;
    let sandbox_root = std::env::temp_dir().join(format!("oris-self-evo-{label}-sandbox"));
    let store_root = std::env::temp_dir().join(format!("oris-self-evo-{label}-store"));

    let _ = std::fs::remove_dir_all(&sandbox_root);
    let _ = std::fs::remove_dir_all(&store_root);
    std::fs::create_dir_all(&sandbox_root)?;
    std::fs::create_dir_all(&store_root)?;

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

    let policy = demo_sandbox_policy();
    let validator = Arc::new(CommandValidator::new(policy.clone()));
    let sandbox = Arc::new(LocalProcessSandbox::new(
        label,
        &workspace_root,
        sandbox_root.as_path(),
    ));
    let store: Arc<dyn EvolutionStore> = Arc::new(JsonlEvolutionStore::new(store_root.clone()));

    let mut governor_config = GovernorConfig::default();
    governor_config.promote_after_successes = promote_after_successes;

    let evo = EvoKernel::new(kernel, sandbox, validator, store)
        .with_governor(Arc::new(DefaultGovernor::new(governor_config)))
        .with_sandbox_policy(policy)
        .with_validation_plan(demo_validation_plan());

    Ok((evo, sandbox_root, store_root))
}

#[cfg(feature = "full-evolution-experimental")]
fn proposal_diff(path: &str, anomaly: &AnomalyReport) -> String {
    format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,8 @@\n+# Self-Heal Plan for {operation}\n+\n+Detected anomaly:\n+- {reason}\n+\n+Suggested optimization:\n+- Introduce local memoization for repeated context assembly\n+- Add retry jitter to avoid synchronized spikes\n+- Promote successful patch to reusable Gene\n",
        path = path,
        operation = anomaly.operation,
        reason = anomaly.reason
    )
}

#[cfg(feature = "full-evolution-experimental")]
#[tokio::main]
async fn main() -> ExampleResult<()> {
    println!("=== Agent Self-Evolution Demo ===\n");

    let telemetry = vec![
        RuntimeSample {
            operation: "planner build context",
            latency_ms: 1680,
            retry_count: 3,
            success_ratio: 0.72,
        },
        RuntimeSample {
            operation: "tool dispatch",
            latency_ms: 420,
            retry_count: 0,
            success_ratio: 0.98,
        },
        RuntimeSample {
            operation: "memory retrieval",
            latency_ms: 310,
            retry_count: 1,
            success_ratio: 0.95,
        },
    ];

    let anomaly = detect_anomaly(&telemetry).ok_or("no anomaly detected in telemetry")?;
    println!("[1] Detected anomaly");
    println!("    operation: {}", anomaly.operation);
    println!("    reason: {}", anomaly.reason);
    println!("    severity: {:.2}", anomaly.severity_score);
    println!("    signals: {:?}\n", anomaly.signals);

    let (evo, sandbox_root, store_root) = build_demo_evo("agent-self-evolution", 1)?;
    let workspace_root = std::env::current_dir()?;
    let base_revision = current_git_head(&workspace_root);

    let task = AgentTask {
        id: "self-heal-latency-hotspot".into(),
        description: format!("Optimize {} after anomaly detection", anomaly.operation),
    };
    let target_path = "docs/evolution-self-heal-generated.md";
    let proposal = MutationProposal {
        intent: format!("{} (self-heal)", task.description),
        files: vec![target_path.to_string()],
        expected_effect:
            "reduce latency spikes and retries, then convert validated patch into reusable gene"
                .into(),
    };

    println!("[2] Capture mutation -> Gene/Capsule");
    let capture = evo
        .capture_from_proposal(
            &"self-evo-capture-round-1".into(),
            &proposal,
            proposal_diff(target_path, &anomaly),
            base_revision,
        )
        .await?;

    println!("    gene_id: {}", capture.gene.id);
    println!("    capsule_id: {}", capture.capsule.id);
    println!("    strategy: {:?}", capture.gene.strategy);
    println!("    captured_signals: {:?}", capture.gene.signals);
    println!("    confidence: {:.2}", capture.capsule.confidence);
    println!("    state: {:?}\n", capture.gene.state);

    println!("[3] Replay similar anomaly (self-heal before fallback)");
    let selector_input = SelectorInput {
        signals: capture.gene.signals.clone(),
        env: capture.capsule.env.clone(),
        spec_id: None,
        limit: 1,
    };
    let decision = evo
        .replay_or_fallback_for_run(&"self-evo-replay-round-2".into(), selector_input)
        .await?;

    println!("    used_capsule: {}", decision.used_capsule);
    println!("    fallback_to_planner: {}", decision.fallback_to_planner);
    println!("    reason: {}", decision.reason);
    if let Some(capsule_id) = decision.capsule_id.as_deref() {
        println!("    selected_capsule_id: {}", capsule_id);
    }

    let feedback =
        EvoKernel::<ExampleState>::replay_feedback_for_agent(&anomaly.signals, &decision);
    println!("\n[4] Agent feedback");
    println!("    planner_directive: {:?}", feedback.planner_directive);
    println!(
        "    reasoning_steps_avoided: {}",
        feedback.reasoning_steps_avoided
    );
    println!("    summary: {}", feedback.summary);

    println!("\n[5] Artifact locations");
    println!("    sandbox_root: {}", sandbox_root.display());
    println!("    store_root: {}", store_root.display());

    println!("\n=== Demo complete: anomaly -> mutation capture -> replay/self-heal ===");
    Ok(())
}
