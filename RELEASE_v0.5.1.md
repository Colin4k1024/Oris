# v0.5.1 - Replay Failure Revocation Fix

Patch release fixing EvoKernel replay handling so repeated replay regressions revoke promoted assets before they can be reused again.

## What's in this release

- Records replay validation failures in the evolution event log and routes the updated failure count back through the governor policy.
- Auto-revokes and quarantines promoted assets after the configured replay failure threshold, so revoked assets immediately drop out of replay selection.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel second_replay_validation_failure_revokes_gene_immediately -- --nocapture
- cargo test -p oris-evokernel replay_hit_records_capsule_reused -- --nocapture
- cargo test -p oris-runtime --features full-evolution-experimental --test evolution_feature_wiring full_evolution_experimental_paths_resolve -- --nocapture
- cargo test --workspace
- /bin/zsh -lc "unset ORT_LIB_LOCATION ORT_PREFER_DYNAMIC_LINK ORT_LIB_PROFILE; cargo build --verbose --all --release --all-features"
- /bin/zsh -lc "unset ORT_LIB_LOCATION ORT_PREFER_DYNAMIC_LINK ORT_LIB_PROFILE; cargo test --release --all-features"
- /bin/zsh -lc "source ~/.zshrc >/dev/null 2>&1; unset ORT_LIB_LOCATION ORT_PREFER_DYNAMIC_LINK ORT_LIB_PROFILE; cargo publish -p oris-runtime --all-features --dry-run --registry crates-io"
- /bin/zsh -lc "source ~/.zshrc >/dev/null 2>&1; unset ORT_LIB_LOCATION ORT_PREFER_DYNAMIC_LINK ORT_LIB_PROFILE; cargo publish -p oris-runtime --all-features --registry crates-io"

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
