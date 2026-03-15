# v0.32.0 - Pipeline Detect/Execute Stage Integration with SignalExtractor and Sandbox

`oris-runtime` now wires the evolution pipeline's Detect stage to `SignalExtractorPort` for runtime signal ingestion (compiler errors, panics, test failures) and the Execute stage to `SandboxPort` for safe sandboxed mutation execution, with per-stage wall-clock timing written to `PipelineContext.stage_timings`.

## What's in this release

- Added `SignalExtractorPort` and `SandboxPort` traits in `oris-evolution::port`, enabling injection of real signal extraction and sandbox execution into `StandardEvolutionPipeline` without creating circular dependencies.
- Detect stage: when a `SignalExtractorPort` is injected, `PipelineContext.signals` is populated from runtime diagnostics (compiler output, stack traces, execution logs). Falls back to pass-through when no extractor is provided (backward-compatible).
- Execute stage: when a `SandboxPort` is injected, the first `MutationProposal` is applied via the sandbox and the result is recorded as `PipelineContext.execution_result`. Falls back to synthetic stub when no sandbox is provided.
- All 8 pipeline stages now record wall-clock duration in `PipelineContext.stage_timings: HashMap<String, Duration>`, replacing the always-`None` `duration_ms` fields.
- Added `RuntimeSignalExtractorAdapter` and `LocalSandboxAdapter` in `oris-evokernel::adapters`, providing ready-to-use implementations of the new port traits backed by the existing `RuntimeSignalExtractor` and `LocalProcessSandbox`.
- Added `PipelineContext.extractor_input: Option<SignalExtractorInput>` carrying raw compiler/trace/log text into the Detect stage.
- Added `StandardEvolutionPipeline::with_signal_extractor()` and `with_sandbox()` builder methods for port injection.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evolution
- cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --dry-run --registry crates-io --allow-dirty
- cargo publish -p oris-runtime --all-features --registry crates-io --allow-dirty

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
