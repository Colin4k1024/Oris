# v0.46.0 - Remote Capsule Auto-Promotion Pipeline (P2-06)

## Released Crates

| Crate | Version |
|-------|---------|
| `oris-evolution-network` | 0.4.2 |

## Summary

Adds an automatic quarantine decision path for remote capsules in the evolution
network. Newly received capsules can now be scored locally, promoted when they
meet the threshold, or kept in quarantine with an explicit audit trail.

## Changes

### `oris-evolution-network` 0.4.2

- Added `RemoteCapsuleReceiver` with `on_capsule_received()`.
- Added configurable `PROMOTE_THRESHOLD` support with default threshold `0.70`.
- Added `CapsuleDisposition` (`Promoted` / `Quarantined`) and
  `QuarantineReason` for rejection auditability.
- Added append-only JSONL audit logging for every remote capsule decision.
- Added tests covering promotion, quarantine, threshold handling, and audit log
  persistence.

## Validation

- `cargo fmt --all -- --check`
- `cargo test -p oris-evolution-network`
- `cargo test --release --all-features`

## Resolves

- Closes #301