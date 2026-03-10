# Stream A Onboarding-First Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a deterministic first-run onboarding path that lets new contributors finish an Evo run in 30 minutes and produces observable artifacts on success or failure.

**Architecture:** Add one canonical first-run script that performs preflight checks, runs a minimal Evo scenario, and always emits structured artifacts under `target/evo_first_run`. Align top-level docs to this single entry path and add a CI gate that validates script execution and artifact schema without external API keys.

**Tech Stack:** Bash, Rust (`cargo`), Markdown docs, GitHub Actions CI.

References: `@test-driven-development`, `@verification-before-completion`.

---

### Task 1: Define first-run artifact contract and fixture expectations

**Files:**
- Create: `docs/evokernel/first-run-artifact-contract.md`
- Test: `scripts/tests/test_evo_first_run_contract.sh`

**Step 1: Write the failing test**

Create a shell test that fails when contract doc or required keys are missing:

```bash
#!/usr/bin/env bash
set -euo pipefail
rg -n '^# First Run Artifact Contract$' docs/evokernel/first-run-artifact-contract.md
rg -n '`status`|`duration_ms`|`scenario`|`timestamp`|`artifact_paths`' docs/evokernel/first-run-artifact-contract.md
```

**Step 2: Run test to verify it fails**

Run: `bash scripts/tests/test_evo_first_run_contract.sh`  
Expected: FAIL because contract file does not exist.

**Step 3: Write minimal implementation**

Add contract doc with:

- required output files:
  - `target/evo_first_run/summary.json`
  - `target/evo_first_run/run.log`
- required `summary.json` keys:
  - `status`
  - `duration_ms`
  - `scenario`
  - `timestamp`
  - `artifact_paths`
- pass/fail markers:
  - `FIRST_RUN_PASS`
  - `FIRST_RUN_FAIL`

**Step 4: Run test to verify it passes**

Run: `bash scripts/tests/test_evo_first_run_contract.sh`  
Expected: PASS.

**Step 5: Commit**

```bash
git add docs/evokernel/first-run-artifact-contract.md scripts/tests/test_evo_first_run_contract.sh
git commit -m "docs(onboarding): define first-run artifact contract"
```

### Task 2: Add first-run script skeleton with deterministic fail codes

**Files:**
- Create: `scripts/evo_first_run.sh`
- Test: `scripts/tests/test_evo_first_run_fail_codes.sh`

**Step 1: Write the failing test**

Create a test that asserts fail-code constants and pass/fail markers:

```bash
#!/usr/bin/env bash
set -euo pipefail
rg -n 'E_ENV|E_BUILD|E_RUNTIME|E_OUTPUT' scripts/evo_first_run.sh
rg -n 'FIRST_RUN_PASS|FIRST_RUN_FAIL' scripts/evo_first_run.sh
```

**Step 2: Run test to verify it fails**

Run: `bash scripts/tests/test_evo_first_run_fail_codes.sh`  
Expected: FAIL because script does not exist.

**Step 3: Write minimal implementation**

Implement script skeleton:

```bash
#!/usr/bin/env bash
set -euo pipefail

OUT_DIR="target/evo_first_run"
SUMMARY="${OUT_DIR}/summary.json"
LOG="${OUT_DIR}/run.log"

mkdir -p "${OUT_DIR}"
echo "FIRST_RUN_FAIL" > /dev/null
```

Add helpers:

- `fail_with_code E_ENV|E_BUILD|E_RUNTIME|E_OUTPUT`
- `write_summary_json ...`
- deterministic logging to `${LOG}`

**Step 4: Run test to verify it passes**

Run: `bash scripts/tests/test_evo_first_run_fail_codes.sh`  
Expected: PASS.

**Step 5: Commit**

```bash
git add scripts/evo_first_run.sh scripts/tests/test_evo_first_run_fail_codes.sh
git commit -m "feat(onboarding): scaffold evo first-run script with deterministic fail taxonomy"
```

### Task 3: Implement preflight checks and actionable recovery hints

**Files:**
- Modify: `scripts/evo_first_run.sh`
- Test: `scripts/tests/test_evo_first_run_preflight.sh`

**Step 1: Write the failing test**

Add tests for preflight branch behavior:

```bash
#!/usr/bin/env bash
set -euo pipefail
OUT="$(EVO_FIRST_RUN_FORCE_ENV_FAIL=1 bash scripts/evo_first_run.sh || true)"
printf '%s' "$OUT" | rg 'E_ENV'
printf '%s' "$OUT" | rg 'rustup toolchain install stable|cargo --version'
```

**Step 2: Run test to verify it fails**

Run: `bash scripts/tests/test_evo_first_run_preflight.sh`  
Expected: FAIL until script supports forced-fail hooks and hint text.

**Step 3: Write minimal implementation**

Add preflight:

- check `cargo` exists
- check `examples/evo_oris_repo/Cargo.toml` exists
- optional forced fail env switches for tests:
  - `EVO_FIRST_RUN_FORCE_ENV_FAIL=1`
  - `EVO_FIRST_RUN_FORCE_BUILD_FAIL=1`

Error output must include one concrete next command.

**Step 4: Run test to verify it passes**

Run: `bash scripts/tests/test_evo_first_run_preflight.sh`  
Expected: PASS.

**Step 5: Commit**

```bash
git add scripts/evo_first_run.sh scripts/tests/test_evo_first_run_preflight.sh
git commit -m "feat(onboarding): add first-run preflight checks with actionable recovery guidance"
```

### Task 4: Run minimal Evo scenario and generate required artifacts

**Files:**
- Modify: `scripts/evo_first_run.sh`
- Test: `scripts/tests/test_evo_first_run_artifacts.sh`

**Step 1: Write the failing test**

Test artifact generation and schema keys:

```bash
#!/usr/bin/env bash
set -euo pipefail
rm -rf target/evo_first_run
bash scripts/evo_first_run.sh
test -f target/evo_first_run/summary.json
test -f target/evo_first_run/run.log
rg -n '"status"|"duration_ms"|"scenario"|"timestamp"|"artifact_paths"' target/evo_first_run/summary.json
```

**Step 2: Run test to verify it fails**

Run: `bash scripts/tests/test_evo_first_run_artifacts.sh`  
Expected: FAIL until scenario execution and summary writing are implemented.

**Step 3: Write minimal implementation**

Execute minimal command:

```bash
cargo run -p evo_oris_repo >>"${LOG}" 2>&1
```

On success:

- write summary with `status=pass`
- print `FIRST_RUN_PASS`

On runtime failure:

- write summary with `status=fail`, `error_code=E_RUNTIME`
- print `FIRST_RUN_FAIL`

**Step 4: Run test to verify it passes**

Run: `bash scripts/tests/test_evo_first_run_artifacts.sh`  
Expected: PASS.

**Step 5: Commit**

```bash
git add scripts/evo_first_run.sh scripts/tests/test_evo_first_run_artifacts.sh
git commit -m "feat(onboarding): run minimal evo scenario and emit first-run artifacts"
```

### Task 5: Align docs to one canonical first-run path

**Files:**
- Modify: `README.md`
- Modify: `docs/open-source-onboarding-zh.md`
- Modify: `docs/evokernel/examples.md`
- Modify: `examples/evo_oris_repo/README.md`
- Test: `scripts/tests/test_evo_first_run_doc_links.sh`

**Step 1: Write the failing test**

Add test to ensure all docs reference the canonical entry script:

```bash
#!/usr/bin/env bash
set -euo pipefail
for f in README.md docs/open-source-onboarding-zh.md docs/evokernel/examples.md examples/evo_oris_repo/README.md; do
  rg -n 'scripts/evo_first_run.sh' "$f"
done
```

**Step 2: Run test to verify it fails**

Run: `bash scripts/tests/test_evo_first_run_doc_links.sh`  
Expected: FAIL before doc updates.

**Step 3: Write minimal implementation**

Doc updates:

- `README.md`: add "First run (30 minutes)" section using one command.
- `docs/open-source-onboarding-zh.md`: mirror canonical path in Chinese.
- `docs/evokernel/examples.md`: split `First Run` vs `Advanced Scenarios`.
- `examples/evo_oris_repo/README.md`: point to root script as default entry.

**Step 4: Run test to verify it passes**

Run: `bash scripts/tests/test_evo_first_run_doc_links.sh`  
Expected: PASS.

**Step 5: Commit**

```bash
git add README.md docs/open-source-onboarding-zh.md docs/evokernel/examples.md examples/evo_oris_repo/README.md scripts/tests/test_evo_first_run_doc_links.sh
git commit -m "docs(onboarding): converge on single canonical evo first-run path"
```

### Task 6: Add CI gate for first-run artifact contract

**Files:**
- Modify: `.github/workflows/ci.yml`
- Test: local script invocation used by CI

**Step 1: Write the failing CI step**

Add CI step:

```yaml
- name: Run evo first-run onboarding gate
  shell: bash
  run: |
    bash scripts/evo_first_run.sh
```

**Step 2: Run local command to verify baseline**

Run: `bash scripts/evo_first_run.sh`  
Expected: establish local behavior before CI merge.

**Step 3: Write minimal implementation adjustments**

Ensure CI-safe defaults:

- no external API key required
- deterministic artifact path under `target/evo_first_run`
- explicit non-zero exit on `FIRST_RUN_FAIL`

**Step 4: Run focused validations**

Run:

```bash
bash scripts/evo_first_run.sh
bash scripts/tests/test_evo_first_run_contract.sh
bash scripts/tests/test_evo_first_run_fail_codes.sh
bash scripts/tests/test_evo_first_run_preflight.sh
bash scripts/tests/test_evo_first_run_artifacts.sh
bash scripts/tests/test_evo_first_run_doc_links.sh
```

Expected: PASS.

**Step 5: Commit**

```bash
git add .github/workflows/ci.yml scripts/evo_first_run.sh scripts/tests
git commit -m "ci(onboarding): add evo first-run gate with deterministic artifact checks"
```

### Task 7: Final verification and readiness summary

**Files:**
- Create: `docs/plans/2026-03-10-stream-a-onboarding-first-readiness.md`

**Step 1: Write readiness checklist template**

```md
- [ ] first-run path <= 8 steps
- [ ] first-run pass marker emitted
- [ ] summary.json and run.log generated
- [ ] fail taxonomy verified (E_ENV/E_BUILD/E_RUNTIME/E_OUTPUT)
- [ ] CI gate merged and green
```

**Step 2: Run full verification**

Run:

```bash
cargo fmt --all -- --check
cargo test -p evo_oris_repo --no-run
bash scripts/evo_first_run.sh
bash scripts/tests/test_evo_first_run_contract.sh
bash scripts/tests/test_evo_first_run_fail_codes.sh
bash scripts/tests/test_evo_first_run_preflight.sh
bash scripts/tests/test_evo_first_run_artifacts.sh
bash scripts/tests/test_evo_first_run_doc_links.sh
```

Expected: PASS; first-run artifacts present in `target/evo_first_run/`.

**Step 3: Write readiness report**

Include:

- execution duration sample
- artifact files observed
- any residual risks (for example slower cold-build machines)

**Step 4: Commit**

```bash
git add docs/plans/2026-03-10-stream-a-onboarding-first-readiness.md
git commit -m "docs(onboarding): add stream a first-run readiness summary"
```

### Notes for execution

- This plan is intended for execution in a dedicated worktree.
- Keep scope strict: first-run path only, no broad scenario refactor in this cycle.
