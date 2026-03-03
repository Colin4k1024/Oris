# v0.8.1 - Deterministic checkpoint recovery hardening

`oris-runtime` now rehydrates kernel runs from the latest checkpoint, persists step-level snapshots after every state transition, and keeps replay verification state hashes aligned with the projected execution log.

## What's in this release

- Kernel runs now resume from the latest saved checkpoint instead of always rebuilding from sequence 1, and interrupts/completions persist the current snapshot before returning.
- Replay and execution-log reconstruction now propagate snapshot store failures instead of silently falling back, which keeps deterministic replay verification honest.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-kernel -- --nocapture
- cargo test -p oris-kernel --features sqlite-persistence -- --nocapture
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-kernel --dry-run
- cargo publish -p oris-kernel
- cargo publish -p oris-runtime --all-features --dry-run
- cargo publish -p oris-runtime --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
