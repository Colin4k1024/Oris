# Stream A Onboarding-First Readiness

Date: 2026-03-10
Scope: issue `#173` execution evidence snapshot

## Checklist

- [x] first-run path documented with a single canonical entry command
- [x] pass marker (`FIRST_RUN_PASS`) emitted on successful run path
- [x] required artifacts generated:
  - `target/evo_first_run/summary.json`
  - `target/evo_first_run/run.log`
- [x] fail taxonomy implemented and test-covered:
  - `E_ENV`
  - `E_BUILD`
  - `E_RUNTIME`
  - `E_OUTPUT`
- [x] CI workflow includes `scripts/evo_first_run.sh` gate

## Verification Evidence

Executed:

```bash
bash scripts/tests/test_evo_first_run_contract.sh
bash scripts/tests/test_evo_first_run_fail_codes.sh
bash scripts/tests/test_evo_first_run_preflight.sh
bash scripts/tests/test_evo_first_run_artifacts.sh
bash scripts/tests/test_evo_first_run_doc_links.sh
```

Observed:

- all listed tests pass
- `summary.json` contains required contract keys
- canonical docs now reference `scripts/evo_first_run.sh`

## Environment Note

In the current sandbox, outbound registry resolution is restricted, so default
`cargo check -p evo_oris_repo` may fail with network DNS errors when dependency
index access is blocked.

To keep behavior testable under offline CI/sandbox conditions, the script
supports explicit test controls:

- `EVO_FIRST_RUN_SKIP_BUILD_CHECK=1`
- `EVO_FIRST_RUN_SCENARIO_CMD='...'`

Default behavior remains unchanged for real runs:

- build check enabled
- scenario command defaults to `cargo run -p evo_oris_repo`
