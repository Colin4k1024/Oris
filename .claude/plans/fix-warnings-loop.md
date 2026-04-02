# Loop Plan: Fix Unused Imports and Warnings

## Pattern: sequential
## Mode: safe
## Stop Condition: All crates pass `cargo clippy -- -D warnings` with no unused imports

## Crates with Warnings

1. **oris-evolution** - unused imports (GeneCategory, Serializer)
2. **oris-intake** - unused imports (IntakeError, IntakeResult, IntakeSourceType, IntakeSourceConfig)
3. **oris-mutation-evaluator** - dead code (CriticResponse, MutationResponse, into_scores_and_rationale)
4. **oris-evokernel** - unused imports
5. **evo_oris_repo** - unused variable (label in self_evolution_demo)

## Loop Steps

For each crate:
1. Run `cargo fix --lib -p <crate> --allow-dirty` to auto-fix
2. Run `cargo build -p <crate>` to verify
3. Run `cargo test -p <crate>` to verify no regressions
4. Commit changes if all pass
5. Move to next crate

## Verification
Final verification: `cargo clippy --all -- -D warnings` must pass
