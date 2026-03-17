# v0.43.0 – Automatic Task Class Inference (P2-03)

## Released Crates

| Crate | Version |
|-------|---------|
| `oris-evolution` | 0.4.0 |
| `oris-runtime` | 0.43.0 |

## Summary

Implements automatic task class inference in `oris-evolution` via keyword recall
scoring (P2-03 of the Phase 2 evolution roadmap).

## Changes

### `oris-evolution` 0.4.0

- **`TaskClassDefinition`** – Extended task-class descriptor with a natural-language
  `description` field used for matching and TOML persistence.
- **`builtin_task_class_definitions()`** – Returns canonical definitions with descriptions.
- **`TaskClassInferencer`** – Infers task class from signal descriptions using keyword
  recall scoring (score = `|signal_tokens ∩ class_keywords| / |class_keywords|`).
  Falls back to `"generic_fix"` when the best score is below the threshold (default `0.75`).
- **`load_task_classes()`** – Loads definitions from `~/.oris/oris-task-classes.toml`
  when present (requires `evolution-experimental` feature), otherwise returns builtins.
- **`load_task_classes_from_toml(path)`** – Parses a TOML file of task class definitions
  (feature-gated behind `evolution-experimental`).
- **`StandardEvolutionPipeline::with_task_class_inferencer()`** – Attach an inferencer;
  Detect stage populates `PipelineResult::inferred_task_class_id`.
- **`StandardEvolutionPipeline::with_default_task_class_inferencer()`** – Convenience
  builder that loads the current `load_task_classes()` registry.
- **`PipelineContext::inferred_task_class_id`** and
  **`PipelineResult::inferred_task_class_id`** – New fields carrying the inferred class
  through the pipeline.
- New `[features]` section with `evolution-experimental = ["dep:toml"]`.

### `oris-runtime` 0.43.0

Coordinated version bump; no direct changes.

## Validation

```
cargo fmt --all -- --check
cargo test -p oris-evolution --features evolution-experimental
cargo build --all --release --all-features
cargo test --release --all-features
cargo publish -p oris-evolution --all-features --dry-run
```

## Acceptance Criteria Met

- [x] `TaskClassInferencer::infer()` returns correct class for canonical compiler-error signals
- [x] Score below 0.75 falls back to `"generic_fix"`
- [x] `builtin_task_classes()` configurable via TOML file (`~/.oris/oris-task-classes.toml`)
- [x] `cargo test -p oris-evolution --features evolution-experimental` — 86 tests pass
