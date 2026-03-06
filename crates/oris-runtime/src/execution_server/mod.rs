//! Graph-aware HTTP execution server and benchmark helpers for `oris-runtime`.

pub mod api_handlers;
#[cfg(feature = "sqlite-persistence")]
pub mod benchmark_suite;
mod graph_bridge;

pub use api_handlers::{build_router, ApiRole, ExecutionApiState};
#[cfg(feature = "mcp-experimental")]
pub use api_handlers::{McpBootstrapConfig, McpCapabilityMapping, McpTransportKind};
#[cfg(feature = "sqlite-persistence")]
pub use benchmark_suite::{
    canonical_runtime_benchmark_baseline_path, run_runtime_benchmark_suite,
    runtime_benchmark_suite_pretty_json, write_runtime_benchmark_suite,
    RuntimeBenchmarkEnvironment, RuntimeBenchmarkMetric, RuntimeBenchmarkSuiteReport,
    RUNTIME_BENCHMARK_BASELINE_DOC_PATH,
};
