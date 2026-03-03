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
- /bin/zsh -lc "unset ORT_LIB_LOCATION ORT_PREFER_DYNAMIC_LINK ORT_LIB_PROFILE; cargo build --verbose --all --release --all-features"
- /bin/zsh -lc "unset ORT_LIB_LOCATION ORT_PREFER_DYNAMIC_LINK ORT_LIB_PROFILE; cargo test --release --all-features"
- cargo publish -p oris-execution-runtime --dry-run --registry crates-io
- cargo publish -p oris-execution-runtime --registry crates-io
- cargo publish -p oris-runtime --all-features --dry-run --registry crates-io --config 'patch.crates-io.oris-execution-runtime.path="/Users/jiafan/Desktop/work-code/Oris/crates/oris-execution-runtime"' --config 'patch.crates-io.oris-evokernel.path="/Users/jiafan/Desktop/work-code/Oris/crates/oris-evokernel"'
- cargo publish -p oris-runtime --all-features --registry crates-io --config 'patch.crates-io.oris-execution-runtime.path="/Users/jiafan/Desktop/work-code/Oris/crates/oris-execution-runtime"' --config 'patch.crates-io.oris-evokernel.path="/Users/jiafan/Desktop/work-code/Oris/crates/oris-evokernel"'

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
