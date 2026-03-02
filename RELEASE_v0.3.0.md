# v0.3.0 - EvoKernel Wave 0 Experimental Wiring

Minor release adding the first cohesive EvoKernel Wave 0 experimental surface for `oris-runtime`.

## What's in this release

- Splits the EvoKernel support crates into stable `lib.rs` entrypoints backed by internal `core.rs` modules while preserving the existing public re-export paths.
- Adds a `full-evolution-experimental` smoke test and keeps the checked-in runtime API contract plus all-features Postgres build path aligned for release validation.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-runtime --test evolution_feature_wiring --features "full-evolution-experimental"
- cargo test --workspace
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
