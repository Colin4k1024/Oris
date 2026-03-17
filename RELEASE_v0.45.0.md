# v0.45.0 – OS-level Sandbox Isolation (P2-05)

## Released Crates

| Crate | Version |
|-------|---------|
| `oris-sandbox` | 0.3.0 |
| `oris-evokernel` | 0.14.1 |
| `oris-runtime` | 0.44.0 |

## Summary

Adds OS-level resource isolation to the sandbox executor (P2-05 of the Phase 2
evolution roadmap): `setrlimit` memory/CPU limits on Linux, `sandbox-exec`
(Seatbelt) wrapping on macOS, and process-group isolation to prevent zombie
children on timeout.

## Changes

### `oris-sandbox` 0.3.0

- **`SandboxPolicy.max_memory_bytes`** – Optional address-space ceiling in bytes.
  Applied via `RLIMIT_AS` on Linux (feature-gated behind `resource-limits`).
- **`SandboxPolicy.max_cpu_secs`** – Optional CPU-time ceiling in seconds.
  Applied via `RLIMIT_CPU` on Linux.
- **`SandboxPolicy.use_process_group`** – When `true`, places the child in a new
  process group via `setsid(2)`; the entire group is killed on timeout, preventing
  zombie grandchildren.
- **`resource_limits::apply_linux_limits()`** – `pre_exec` hook that calls
  `setrlimit` for memory and CPU limits (Linux only; no-op elsewhere).
- **`resource_limits::apply_macos_sandbox()`** – Rewires the command to run via
  `sandbox-exec -p <profile>` (macOS Seatbelt; compiled only on `cfg(target_os =
  "macos")`).
- **`resource-limits` feature flag** – Opt-in via `features = ["resource-limits"]`
  to enable the `dep:nix` dependency and OS-level enforcement.

### `oris-evokernel` 0.14.1

- Updated `oris-sandbox` dependency: `0.2.0 → 0.3.0`.
- Updated `SandboxPolicy` struct literals in integration tests and `core.rs` to
  include the three new fields (`max_memory_bytes`, `max_cpu_secs`,
  `use_process_group`).

### `oris-runtime` 0.44.0

- Updated `oris-evokernel` dependency: `0.14.0 → 0.14.1`.
- Updated `SandboxPolicy` struct literals in examples and integration tests.

## Validation

- `cargo fmt --all -- --check` ✅
- `cargo test -p oris-sandbox --features resource-limits --lib` → 6 passed ✅
- `cargo build --all --release --all-features` ✅
