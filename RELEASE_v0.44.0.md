# v0.44.0 – Confidence Model: Time-Decay Weighting and Bayesian Update Path (P2-04)

## Released Crates

| Crate | Version |
|-------|---------|
| `oris-evolution` | 0.4.1 |
| `oris-mutation-evaluator` | 0.3.0 |

## Summary

Implements time-decay weighting and a Bayesian conjugate update path for mutation
confidence scoring (P2-04 of the Phase 2 evolution roadmap).

## Changes

### `oris-mutation-evaluator` 0.3.0

- **`CompositeScore`** – Composite mutation score that bundles a raw score, a
  time-decay-weighted score (`w_t = exp(-λ * age_days)`, λ=0.05 ≈ 14-day
  half-life), a Wilson score confidence interval, and the historical sample count.
- **`CompositeScore::compute(raw, age_days, sample_count)`** – Constructs the
  composite from a dimension score, the observation age in days, and the number of
  historical outcomes.
- **`CompositeScore::pessimistic()`** – Returns the upper Wilson CI bound when
  `sample_count < 10` (low-data regime), otherwise returns the raw score.
- **`wilson_interval(p, n)`** – Wilson score proportion interval at 95% CI
  (`z = 1.96`); returns `(0.0, 1.0)` when `n == 0`.

### `oris-evolution` 0.4.1

- **`BayesianConfidenceUpdater`** – Beta-Bernoulli conjugate model for tracking
  asset confidence.  Maintains `(α, β)` parameters; each success increments `α`,
  each failure increments `β`.  Posterior mean is `α / (α + β)`.
- **`BetaPrior`** – Struct holding `(alpha, beta)` prior hyperparameters.
- **`builtin_priors()`** – Returns the canonical weak-success prior `Beta(2, 1)`.
- **`ConfidenceSnapshot`** – Snapshot of the posterior: `mean`, `variance`,
  `sample_count`, and `is_stable` flag (true when ≥10 observations and
  variance < 0.01).

## Validation

- `cargo fmt --all -- --check` ✅
- `cargo test -p oris-mutation-evaluator --lib` → 18 passed ✅
- `cargo test -p oris-evolution --lib` → 91 passed ✅
- `cargo build --all --release --all-features` ✅
