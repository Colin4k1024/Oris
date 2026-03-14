# W8 Self-Evolution Roadmap Implementation Plan

> Archive note: This implementation plan is preserved as historical planning material.
> The W8 issue chain it describes has already been executed through `#238` and merged to `main`.
> Treat this file as archival context, not an active execution checklist.

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Turn the approved W8 self-evolution direction into a sequenced set of GitHub-ready issue drafts and supporting roadmap notes.

**Architecture:** Keep the plan aligned to the existing self-evolution boundary. The work here is planning-only: define issue slices, validation expectations, and acceptance language so later implementation issues stay narrow and auditable.

**Tech Stack:** Markdown, GitHub issue templates in freeform markdown, existing Oris planning docs

---

### Task 1: Capture the Approved W8 Boundary in a Design Doc

**Files:**
- Create: `docs/plans/2026-03-14-w8-self-evolution-roadmap-design.md`

**Step 1: Write the design summary**

Document:
- why W8 starts after the W7 replay-memory hardening track
- why the roadmap remains strictly on the self-evolution line
- why the issue split follows closed-loop stages instead of cross-cutting refactors

**Step 2: Record the shared closed-loop flow**

Include the five stages:
- intake
- planning
- execution
- delivery
- audit gate

**Step 3: Record fail-closed rules and testing strategy**

Describe:
- the default reason-code mappings
- the no-partial-success rule
- the required regression, wiring, and storyline test layers for each issue

**Step 4: Commit the design doc**

Run:
`git add docs/plans/2026-03-14-w8-self-evolution-roadmap-design.md && git commit -m "docs(plans): outline W8 self-evolution roadmap"`

Expected:
- one commit containing the approved design

### Task 2: Draft the Five W8 Issues as GitHub-Ready Specs

**Files:**
- Modify: `docs/plans/2026-03-14-w8-self-evolution-roadmap-implementation-plan.md`
- Create: `docs/plans/2026-03-14-w8-self-evolution-issue-drafts.md`

**Step 1: Draft W8-01 and W8-02**

For each issue, include:
- title
- why
- scope
- definition of done
- non-goals
- minimum validation commands
- required machine-readable outputs

**Step 2: Draft W8-03 and W8-04**

For each issue, include:
- title
- execution boundary
- approval or supervision boundary
- fail-closed conditions
- targeted runtime and evokernel tests

**Step 3: Draft W8-05**

For the final issue, include:
- acceptance-gate contract expectations
- audit consistency checks
- track-level closeout criteria

**Step 4: Verify issue ordering and dependencies**

Check that:
- each issue can ship independently
- later issues depend only on contracts created by earlier issues
- no issue introduces autonomous merge or release

**Step 5: Commit the draft file**

Run:
`git add docs/plans/2026-03-14-w8-self-evolution-issue-drafts.md docs/plans/2026-03-14-w8-self-evolution-roadmap-implementation-plan.md && git commit -m "docs(plans): draft W8 self-evolution issues"`

Expected:
- one commit containing the GitHub-ready issue drafts

### Task 3: Align Acceptance Language with the New W8 Target

**Files:**
- Modify: `docs/evokernel/self-evolution-acceptance-checklist.md`
- Modify: `docs/evokernel/implementation-roadmap.md`

**Step 1: Add a planning note, not a shipped claim**

Update the docs to say:
- W7 is complete
- W8 is the planned next track
- the target boundary is supervised closed-loop self-evolution
- this is still below autonomous release or autonomous planning claims

**Step 2: Add the W8 issue references once numbers exist**

Prepare placeholders or a follow-up edit plan so the docs can point to the actual GitHub issues after they are created.

**Step 3: Review wording for overclaim**

Verify the edited docs do not claim that W8 behavior already ships.

**Step 4: Commit the doc alignment**

Run:
`git add docs/evokernel/self-evolution-acceptance-checklist.md docs/evokernel/implementation-roadmap.md && git commit -m "docs(evokernel): align W8 planning boundary"`

Expected:
- one commit containing planning-language updates only

### Task 4: Seed GitHub Issues from the Drafts

**Files:**
- Reference: `docs/plans/2026-03-14-w8-self-evolution-issue-drafts.md`
- Optional modify: `docs/issues-roadmap.csv`

**Step 1: Create the five GitHub issues**

For each W8 issue:
- create the issue with the drafted title and body
- add labels: `type/feature`, `priority/P1`, `area/evolution`
- add `plan` if the repository uses it for roadmap-tracking issues

**Step 2: Verify issue numbering and links**

Run:
`gh issue list --repo Colin4k1024/Oris --state open --limit 20`

Expected:
- the five W8 issues appear with final numbers

**Step 3: Backfill references in roadmap docs if needed**

If issue numbers are now known:
- update the roadmap and acceptance docs with the actual issue numbers
- keep wording future-looking, not shipped

**Step 4: Commit any link backfills**

Run:
`git add docs/issues-roadmap.csv docs/evokernel/self-evolution-acceptance-checklist.md docs/evokernel/implementation-roadmap.md && git commit -m "docs(roadmap): link W8 self-evolution issue set"`

Expected:
- one commit containing issue-number backfills only if edits were needed

### Task 5: Publish the Planning Branch for Review

**Files:**
- No new files required

**Step 1: Verify the planning branch state**

Run:
`git status --short --branch`

Expected:
- clean worktree on the planning branch

**Step 2: Push the branch**

Run:
`git push -u origin codex/w8-self-evolution-roadmap`

Expected:
- branch available for PR or further roadmap review

**Step 3: Summarize the output**

Report:
- design doc path
- issue draft path
- whether GitHub issues were created or only drafted
- any remaining manual decisions

**Step 4: Keep execution mode explicit**

Do not start implementing any W8 runtime code from this plan branch. This branch is for roadmap seeding and issue definition only.
