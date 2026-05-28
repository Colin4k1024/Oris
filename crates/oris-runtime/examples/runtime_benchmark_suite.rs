#[cfg(all(feature = "execution-server", feature = "sqlite-persistence"))]
use std::env;
#[cfg(all(feature = "execution-server", feature = "sqlite-persistence"))]
use std::path::PathBuf;

#[cfg(all(feature = "execution-server", feature = "sqlite-persistence"))]
use oris_runtime::execution_server::{
    canonical_runtime_benchmark_baseline_path, write_runtime_benchmark_suite,
    RUNTIME_BENCHMARK_BASELINE_DOC_PATH,
};

#[cfg(all(feature = "execution-server", feature = "sqlite-persistence"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let output_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(canonical_runtime_benchmark_baseline_path);
    let sample_size = args
        .next()
        .and_then(|raw| raw.to_str().and_then(|s| s.parse::<u32>().ok()))
        .unwrap_or(5);

    write_runtime_benchmark_suite(&output_path, sample_size)?;
    eprintln!(
        "Wrote runtime benchmark report to {} (canonical baseline: {}).",
        output_path.display(),
        RUNTIME_BENCHMARK_BASELINE_DOC_PATH,
    );
    Ok(())
}

#[cfg(not(all(feature = "execution-server", feature = "sqlite-persistence")))]
fn main() {
    eprintln!(
        "runtime_benchmark_suite example requires features: execution-server,sqlite-persistence"
    );
}
