# Release: oris-runtime v0.59.0

## Summary

Adds A2A Council Workflow contract tests ([EVOMAP-152][P1], issue #332).  
Closes #332.

## What Changed

### New Tests — A2A Council Workflow (`a2a_council_*`)

Added 13 contract tests covering the full session→propose→vote→execute lifecycle:

**Session management**
- Added `a2a_council_session_open_returns_ok` — verifies session open shape (`session_id`, `status`, `action`, `idempotent`)
- Added `a2a_council_session_open_idempotent_same_settings` — repeated open with identical quorum settings returns `idempotent: true`
- Added `a2a_council_session_invalid_action_rejected` — unknown action returns 400
- Added `a2a_council_session_close_returns_ok` — session close returns `status: closed`

**Proposal submission**
- Added `a2a_council_propose_missing_title_rejected` — missing `title` returns 400
- Added `a2a_council_propose_records_proposal` — valid proposal yields `status: proposed`
- Added `a2a_council_propose_idempotent_on_repeat` — identical proposal re-submission returns `idempotent: true`

**Voting**
- Added `a2a_council_vote_records_vote` — yes vote is tallied in `votes.yes`
- Added `a2a_council_vote_idempotent_on_repeat` — same sender re-casting the same vote returns `idempotent: true` and counts only once
- Added `a2a_council_vote_conflict_rejected` — same sender casting a different vote returns 409 with `reason: vote_conflict`

**Execution**
- Added `a2a_council_execute_insufficient_quorum_rejected` — execute with zero votes against quorum=2 returns 409 with `reason: insufficient_quorum`
- Added `a2a_council_execute_approved_proposal_succeeds` — full session→propose→vote→execute flow yields `status: executed`
- Added `a2a_council_execute_idempotent_on_repeat` — second execute on an already-executed proposal returns `idempotent: true`

## Validation

- `cargo fmt --all -- --check` ✅
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_council_ -- --nocapture`: **13 passed, 0 failed** ✅
- `cargo build --all --release --all-features` ✅
- `cargo test --release --all-features`: **all ok, 0 failed** ✅
- `cargo publish -p oris-runtime --all-features --dry-run` ✅
