# v0.8.0 - Kernel Trace Context and Observability

Minor release adding real kernel observability snapshots and trace-aware timeline exports to `oris-runtime`.

## What's in this release

- The execution server now derives `KernelObservability` from checkpoint history and active lease context instead of returning placeholder-only telemetry.
- The job timeline and timeline export APIs now include optional trace context so callers can correlate timeline reads with runtime spans without breaking existing clients.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-execution-runtime --features execution-server kernel_observability_from_checkpoint_history -- --nocapture
- cargo test -p oris-execution-runtime --features execution-server generated_runtime_api_contract_matches_checked_in_artifact -- --nocapture
- cargo test -p oris-execution-runtime --features "execution-server,sqlite-persistence" attempt_trace_context_round_trip_and_advances -- --nocapture
- cargo test -p oris-runtime --features "execution-server,sqlite-persistence" run_to_worker_flow_propagates_trace_context_end_to_end -- --nocapture
- cargo test -p oris-kernel --features "sqlite-persistence,kernel-postgres" -- --nocapture
- cargo test --workspace

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
