//! Dedicated runtime benchmark harness for hot-path performance tracking.

#![cfg(all(feature = "execution-server", feature = "sqlite-persistence"))]

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration as StdDuration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::Utc;
use serde::Serialize;

use crate::graph::{
    function_node, interrupt, GraphError, InMemorySaver, MessagesState, StateGraph, END, START,
};
use crate::schemas::messages::Message;

use super::api_handlers::{inspect_job, list_jobs, replay_job, run_job, ExecutionApiState};
use super::api_models::{ListJobsQuery, ReplayJobRequest, RunJobRequest};
use super::models::AttemptExecutionStatus;
use super::scheduler::{SchedulerDecision, SkeletonScheduler};
use super::sqlite_runtime_repository::SqliteRuntimeRepository;
use crate::kernel::runtime::repository::RuntimeRepository;

pub const RUNTIME_BENCHMARK_BASELINE_DOC_PATH: &str = "docs/runtime-benchmark-baseline.json";

#[derive(Clone, Debug, Serialize)]
pub struct RuntimeBenchmarkSuiteReport {
    pub generated_at: String,
    pub sample_size: u32,
    pub environment: RuntimeBenchmarkEnvironment,
    pub benchmarks: Vec<RuntimeBenchmarkMetric>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RuntimeBenchmarkEnvironment {
    pub crate_version: &'static str,
    pub platform: String,
    pub regression_policy: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct RuntimeBenchmarkMetric {
    pub id: &'static str,
    pub group: &'static str,
    pub unit: &'static str,
    pub iterations: u32,
    pub total_ms: f64,
    pub avg_ms: f64,
    pub throughput_per_sec: f64,
}

pub async fn run_runtime_benchmark_suite(
    sample_size: u32,
) -> Result<RuntimeBenchmarkSuiteReport, Box<dyn std::error::Error>> {
    let sample_size = sample_size.max(1);
    let db_path = temp_benchmark_db_path();
    let _cleanup = BenchmarkDbCleanup {
        path: db_path.clone(),
    };

    let repo = SqliteRuntimeRepository::new(path_to_str(&db_path)?)?;
    let scheduler = SkeletonScheduler::new(repo.clone());

    let dispatch_metric = bench_dispatch_path(&repo, &scheduler, sample_size)?;
    let heartbeat_metric = bench_lease_heartbeat_path(&repo, sample_size)?;

    let simple_state = ExecutionApiState::with_sqlite_idempotency(
        build_benchmark_graph().await,
        path_to_str(&db_path)?,
    );
    let interrupt_state = ExecutionApiState::with_sqlite_idempotency(
        build_interrupt_benchmark_graph().await,
        path_to_str(&db_path)?,
    );
    let run_job_metric = bench_run_job_api(simple_state.clone(), sample_size).await?;
    let inspect_job_metric = bench_inspect_job_api(interrupt_state.clone(), sample_size).await?;
    let list_jobs_metric = bench_list_jobs_api(simple_state, sample_size).await?;
    let replay_metric = bench_replay_api(interrupt_state, sample_size).await?;

    Ok(RuntimeBenchmarkSuiteReport {
        generated_at: Utc::now().to_rfc3339(),
        sample_size,
        environment: RuntimeBenchmarkEnvironment {
            crate_version: env!("CARGO_PKG_VERSION"),
            platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            regression_policy: "manual-review-on-baseline-delta",
        },
        benchmarks: vec![
            dispatch_metric,
            heartbeat_metric,
            run_job_metric,
            inspect_job_metric,
            list_jobs_metric,
            replay_metric,
        ],
    })
}

pub fn runtime_benchmark_suite_pretty_json(
    sample_size: u32,
) -> Result<String, Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Runtime::new()?;
    let report = runtime.block_on(run_runtime_benchmark_suite(sample_size))?;
    Ok(serde_json::to_string_pretty(&report)?)
}

pub fn write_runtime_benchmark_suite(
    path: impl AsRef<Path>,
    sample_size: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let rendered = runtime_benchmark_suite_pretty_json(sample_size)?;
    fs::write(path, rendered)?;
    Ok(())
}

pub fn canonical_runtime_benchmark_baseline_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(RUNTIME_BENCHMARK_BASELINE_DOC_PATH)
}

async fn build_benchmark_graph() -> Arc<crate::graph::CompiledGraph<MessagesState>> {
    let node = function_node("research", |_state: &MessagesState| async move {
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message("ok")]).unwrap(),
        );
        Ok(update)
    });
    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("research", node).unwrap();
    graph.add_edge(START, "research");
    graph.add_edge("research", END);
    let saver = Arc::new(InMemorySaver::new());
    Arc::new(graph.compile_with_persistence(Some(saver), None).unwrap())
}

async fn build_interrupt_benchmark_graph() -> Arc<crate::graph::CompiledGraph<MessagesState>> {
    let node = function_node("approval", |_state: &MessagesState| async move {
        let approved = interrupt("approve?")
            .await
            .map_err(GraphError::InterruptError)?;
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message(format!(
                "approved={}",
                approved
            ))])
            .unwrap(),
        );
        Ok(update)
    });
    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("approval", node).unwrap();
    graph.add_edge(START, "approval");
    graph.add_edge("approval", END);
    let saver = Arc::new(InMemorySaver::new());
    Arc::new(graph.compile_with_persistence(Some(saver), None).unwrap())
}

fn bench_dispatch_path(
    repo: &SqliteRuntimeRepository,
    scheduler: &SkeletonScheduler<SqliteRuntimeRepository>,
    sample_size: u32,
) -> Result<RuntimeBenchmarkMetric, Box<dyn std::error::Error>> {
    let mut total = StdDuration::ZERO;
    for idx in 0..sample_size {
        let attempt_id = format!("bench-dispatch-{}", idx);
        let run_id = format!("bench-run-dispatch-{}", idx);
        repo.enqueue_attempt(&attempt_id, &run_id)?;

        let started = Instant::now();
        let decision = scheduler.dispatch_one("bench-worker-dispatch")?;
        total += started.elapsed();

        match decision {
            SchedulerDecision::Dispatched {
                attempt_id: leased, ..
            } => {
                assert_eq!(leased, attempt_id);
            }
            SchedulerDecision::Noop => {
                return Err("dispatch benchmark unexpectedly produced noop".into());
            }
        }

        let outcome = repo.ack_attempt(
            &attempt_id,
            AttemptExecutionStatus::Completed,
            None,
            Utc::now(),
        )?;
        if outcome.status != AttemptExecutionStatus::Completed {
            return Err("dispatch benchmark ack did not complete".into());
        }
    }
    Ok(metric_from_duration(
        "scheduler_dispatch",
        "scheduler",
        total,
        sample_size,
    ))
}

fn bench_lease_heartbeat_path(
    repo: &SqliteRuntimeRepository,
    sample_size: u32,
) -> Result<RuntimeBenchmarkMetric, Box<dyn std::error::Error>> {
    let attempt_id = "bench-heartbeat-attempt";
    repo.enqueue_attempt(attempt_id, "bench-heartbeat-run")?;
    let lease = repo.upsert_lease(
        attempt_id,
        "bench-worker-heartbeat",
        Utc::now() + chrono::Duration::seconds(30),
    )?;
    let mut expected_version = lease.version;
    let mut total = StdDuration::ZERO;

    for _ in 0..sample_size {
        let heartbeat_at = Utc::now();
        let lease_expires_at = heartbeat_at + chrono::Duration::seconds(30);
        let started = Instant::now();
        repo.heartbeat_lease_with_version(
            &lease.lease_id,
            "bench-worker-heartbeat",
            expected_version,
            heartbeat_at,
            lease_expires_at,
        )?;
        total += started.elapsed();
        expected_version += 1;
    }

    Ok(metric_from_duration(
        "lease_heartbeat",
        "lease",
        total,
        sample_size,
    ))
}

async fn bench_run_job_api(
    state: ExecutionApiState,
    sample_size: u32,
) -> Result<RuntimeBenchmarkMetric, Box<dyn std::error::Error>> {
    let headers = HeaderMap::new();
    let mut total = StdDuration::ZERO;

    for idx in 0..sample_size {
        let started = Instant::now();
        let response = run_job(
            State(state.clone()),
            headers.clone(),
            Json(RunJobRequest {
                thread_id: format!("bench-run-job-{}", idx),
                input: Some("hello".to_string()),
                idempotency_key: None,
                timeout_policy: None,
                priority: None,
                tenant_id: None,
            }),
        )
        .await
        .map_err(boxed_api_err)?;
        total += started.elapsed();
        if response.0.data.thread_id.is_empty() {
            return Err("run job benchmark returned empty thread_id".into());
        }
    }

    Ok(metric_from_duration(
        "api_run_job",
        "api",
        total,
        sample_size,
    ))
}

async fn bench_inspect_job_api(
    state: ExecutionApiState,
    sample_size: u32,
) -> Result<RuntimeBenchmarkMetric, Box<dyn std::error::Error>> {
    let headers = HeaderMap::new();
    let thread_id = "bench-inspect-seed".to_string();
    let _ = run_job(
        State(state.clone()),
        headers.clone(),
        Json(RunJobRequest {
            thread_id: thread_id.clone(),
            input: Some("seed".to_string()),
            idempotency_key: None,
            timeout_policy: None,
            priority: None,
            tenant_id: None,
        }),
    )
    .await
    .map_err(boxed_api_err)?;

    let mut total = StdDuration::ZERO;
    for _ in 0..sample_size {
        let started = Instant::now();
        let response = inspect_job(
            State(state.clone()),
            AxumPath(thread_id.clone()),
            headers.clone(),
        )
        .await
        .map_err(boxed_api_err)?;
        total += started.elapsed();
        if response.0.data.thread_id != thread_id {
            return Err("inspect job benchmark returned unexpected thread".into());
        }
    }

    Ok(metric_from_duration(
        "api_inspect_job",
        "api",
        total,
        sample_size,
    ))
}

async fn bench_list_jobs_api(
    state: ExecutionApiState,
    sample_size: u32,
) -> Result<RuntimeBenchmarkMetric, Box<dyn std::error::Error>> {
    let headers = HeaderMap::new();
    for idx in 0..sample_size {
        let _ = run_job(
            State(state.clone()),
            headers.clone(),
            Json(RunJobRequest {
                thread_id: format!("bench-list-seed-{}", idx),
                input: Some("seed".to_string()),
                idempotency_key: None,
                timeout_policy: None,
                priority: None,
                tenant_id: None,
            }),
        )
        .await
        .map_err(boxed_api_err)?;
    }

    let mut total = StdDuration::ZERO;
    for _ in 0..sample_size {
        let started = Instant::now();
        let response = list_jobs(
            State(state.clone()),
            headers.clone(),
            Query(ListJobsQuery {
                status: None,
                limit: Some(50),
                offset: Some(0),
            }),
        )
        .await
        .map_err(boxed_api_err)?;
        total += started.elapsed();
        if response.0.data.jobs.is_empty() {
            return Err("list jobs benchmark returned no jobs".into());
        }
    }

    Ok(metric_from_duration(
        "api_list_jobs",
        "api",
        total,
        sample_size,
    ))
}

async fn bench_replay_api(
    state: ExecutionApiState,
    sample_size: u32,
) -> Result<RuntimeBenchmarkMetric, Box<dyn std::error::Error>> {
    let headers = HeaderMap::new();
    let mut total = StdDuration::ZERO;

    for idx in 0..sample_size {
        let thread_id = format!("bench-replay-{}", idx);
        let _ = run_job(
            State(state.clone()),
            headers.clone(),
            Json(RunJobRequest {
                thread_id: thread_id.clone(),
                input: Some("seed".to_string()),
                idempotency_key: None,
                timeout_policy: None,
                priority: None,
                tenant_id: None,
            }),
        )
        .await
        .map_err(boxed_api_err)?;

        let started = Instant::now();
        let response = replay_job(
            State(state.clone()),
            AxumPath(thread_id),
            headers.clone(),
            Json(ReplayJobRequest {
                checkpoint_id: None,
            }),
        )
        .await
        .map_err(boxed_api_err)?;
        total += started.elapsed();
        if response.0.data.status.is_empty() {
            return Err("replay benchmark returned empty status".into());
        }
    }

    Ok(metric_from_duration(
        "api_replay_job",
        "replay",
        total,
        sample_size,
    ))
}

fn metric_from_duration(
    id: &'static str,
    group: &'static str,
    total: StdDuration,
    iterations: u32,
) -> RuntimeBenchmarkMetric {
    let total_ms = total.as_secs_f64() * 1000.0;
    let avg_ms = total_ms / iterations as f64;
    let throughput_per_sec = if total.as_secs_f64() > 0.0 {
        iterations as f64 / total.as_secs_f64()
    } else {
        0.0
    };
    RuntimeBenchmarkMetric {
        id,
        group,
        unit: "ms",
        iterations,
        total_ms,
        avg_ms,
        throughput_per_sec,
    }
}

fn temp_benchmark_db_path() -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    std::env::temp_dir().join(format!(
        "oris-runtime-benchmark-{}-{}.sqlite",
        std::process::id(),
        ts
    ))
}

fn path_to_str(path: &Path) -> Result<&str, Box<dyn std::error::Error>> {
    path.to_str()
        .ok_or_else(|| "benchmark path is not valid UTF-8".into())
}

fn boxed_api_err(err: super::api_errors::ApiError) -> Box<dyn std::error::Error> {
    io::Error::other(format!("{:?}", err)).into()
}

struct BenchmarkDbCleanup {
    path: PathBuf,
}

impl Drop for BenchmarkDbCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
