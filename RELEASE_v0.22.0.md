# v0.22.0 - Runtime Signal Extraction

Add automatic runtime signal extraction as a dedicated stage in the evolution loop for self-evolving agents.

## What's in this release

- **Runtime Signal Extraction**: New `RuntimeSignalExtractor` module in oris-evokernel for automatic signal extraction from execution context
  - `CompilerDiagnosticsParser`: Extract signals from rustc errors and warnings
  - `StackTraceParser`: Extract signals from panic stack traces
  - `LogAnalyzer`: Extract signals from execution logs (timeouts, resource exhaustion, test failures)
- Signal types: CompilerDiagnostic, RuntimePanic, Timeout, TestFailure, PerformanceIssue, ResourceExhaustion, ConfigError, SecurityIssue, GenericError
- Signals are deterministically extracted using regex pattern matching

## Validation

- cargo fmt --all
- cargo test -p oris-evokernel (30 tests passed)
- cargo build -p oris-runtime --all-features
- cargo publish -p oris-runtime --all-features --dry-run
- cargo publish -p oris-runtime --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
