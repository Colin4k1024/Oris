use std::path::PathBuf;

use evo_oris_repo::benchmark::{run_evo_vs_non_evo_benchmark, BenchmarkConfig};
use evo_oris_repo::ExampleResult;

fn parse_bool(input: &str) -> Option<bool> {
    match input.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "y" => Some(true),
        "0" | "false" | "no" | "n" => Some(false),
        _ => None,
    }
}

fn parse_args() -> ExampleResult<BenchmarkConfig> {
    let mut config = BenchmarkConfig::default();
    let default_model = config.model.clone();
    let mut args = std::env::args().skip(1).peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--planner" => {
                config.planner = args.next().ok_or("missing value for --planner")?;
            }
            "--planner-base-url" => {
                config.planner_base_url =
                    Some(args.next().ok_or("missing value for --planner-base-url")?);
            }
            "--iterations" => {
                let value = args
                    .next()
                    .ok_or("missing value for --iterations")?
                    .parse::<u32>()?;
                config.iterations = value.max(1);
            }
            "--model" => {
                config.model = args.next().ok_or("missing value for --model")?;
            }
            "--output-json" => {
                config.output_json =
                    PathBuf::from(args.next().ok_or("missing value for --output-json")?);
            }
            "--output-md" => {
                config.output_md =
                    PathBuf::from(args.next().ok_or("missing value for --output-md")?);
            }
            "--output-assets-json" => {
                config.output_assets_json = PathBuf::from(
                    args.next()
                        .ok_or("missing value for --output-assets-json")?,
                );
            }
            "--log-file" => {
                config.log_file = PathBuf::from(args.next().ok_or("missing value for --log-file")?);
            }
            "--allow-skip-non-evo" => {
                let raw = args
                    .next()
                    .ok_or("missing value for --allow-skip-non-evo")?;
                config.allow_skip_non_evo =
                    parse_bool(&raw).ok_or("--allow-skip-non-evo expects true/false or 1/0")?;
            }
            "--verbose" | "-v" => {
                config.verbose = true;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: cargo run -p evo_oris_repo --bin evo_vs_non_evo_benchmark [options]\n\n  --planner <openai-compatible|deepseek|ollama>  default: openai-compatible\n  --model <string>                               default: qwen3-235b-a22b\n  --planner-base-url <url>                       default: https://api.openai.com/v1\n  --iterations <u32>                             default: 10\n  --output-json <path>                           default: target/evo_bench/report.json\n  --output-md <path>                             default: target/evo_bench/report.md\n  --output-assets-json <path>                    default: target/evo_bench/shareable_assets.json\n  --log-file <path>                              default: target/evo_bench/benchmark.log\n  --allow-skip-non-evo <bool>                    default: true\n  --verbose, -v                                  print per-step benchmark logs\n"
                );
                std::process::exit(0);
            }
            _ => {
                return Err(format!("unknown argument: {arg}").into());
            }
        }
    }

    if config.model == default_model {
        if config.planner.eq_ignore_ascii_case("ollama") {
            config.model = "llama3".to_string();
        } else if config.planner.eq_ignore_ascii_case("deepseek") {
            config.model = "deepseek-chat".to_string();
        }
    }

    Ok(config)
}

#[tokio::main]
async fn main() -> ExampleResult<()> {
    let config = parse_args()?;
    let report = run_evo_vs_non_evo_benchmark(&config).await?;

    println!(
        "benchmark completed: baseline_status={}, groups={}, runs={}",
        report.baseline_status,
        report.group_summaries.len(),
        report.runs.len()
    );
    println!("json report: {}", config.output_json.display());
    println!("markdown report: {}", config.output_md.display());
    println!("shareable assets: {}", config.output_assets_json.display());
    println!("log file: {}", config.log_file.display());

    Ok(())
}
