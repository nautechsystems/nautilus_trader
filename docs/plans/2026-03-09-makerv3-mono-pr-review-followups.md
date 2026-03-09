# MakerV3 Mono PR Review Follow-Ups Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Close the highest-signal documentation, UI/API, and repo-standards gaps surfaced by the PR/worktree sweep so PR #5 is reviewable against the current monorepo layout and operator contracts.

**Architecture:** Fix the review surface from the outside in. First, repair stale reviewer-facing documentation and contributor guidance so the branch points at current canonical paths. Next, harden Pulse and Fluxboard UI contracts where the backend already exposes broader capabilities than the frontend consumes. Finally, realign repo gates and CI/doc tooling with the monorepo ownership model so future work cannot reintroduce the same drift.

**Tech Stack:** Markdown docs, GitHub PR metadata, Python/Flask Pulse API, React/TypeScript Pulse UI and Fluxboard, Vitest, pytest, shell CI/tooling.

## Review Findings This Plan Closes

1. PR #5 still describes deleted `examples/live/poc/*` and old plan/test paths instead of the current branch surface.
2. Historical Makerv3 review/plan docs still point to removed `docs/flux/makerv3.md` instead of `systems/flux/docs/makerv3.md`.
3. Historical plan narratives still foreground obsolete `scripts/deploy/makerv3_stack.sh` commands even though current contracts reject those entrypoints.
4. `deploy/tokenmm/README.md` still contains rollout-date and topology language that has already gone stale.
5. `examples/live/makerv3/README.md` omits the `run_portfolio` sidecar needed for shared TokenMM balances and global inventory semantics.
6. `systems/flux/flux/runners/tokenmm/run_node.py` wires Redis sidecar clients that are not explicitly closed on shutdown.
7. `fluxboard/README.md` still points to removed `examples/live/makerv3_single_leg/README.md`, and `pulse-ui/` has no README.
8. Pulse top-level shell navigation is hardcoded to `tokenmm/*` despite shared TokenMM/equities hosting support.
9. Pulse group actions do not surface in-flight or deferred control-plane work even though the API models it.
10. Pulse error payloads always return `last_seen = null` even though the UI is built to display it.
11. Fluxboard’s resync completion contract is still underspecified in code and docs.
12. `scripts/ops/tokenmm_risk_audit.py` is a real file under a compatibility-only tree and currently fails `tooling/ci/check-repo-structure.sh`.
13. Active CI and tooling docs still mix canonical monorepo paths with legacy `scripts/*` and root-app paths.

## Repo Standards

Primary references for this plan:

1. `docs/developer_guide/docs.md`
2. `docs/developer_guide/testing.md`
3. `docs/developer_guide/coding_standards.md`
4. `docs/repo/structure.md`
5. `docs/repo/standards.md`
6. `.github/pull_request_template.md`

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | blocked | main | Tasks 1-3 and 5-10 are complete on the live branch after controller verification/review; Task 4 remains the only open item and is still blocked by preserved overlapping Makerv3 worktree edits |
| Task 1: Rewrite PR Body To Match The Actual Branch Surface | completed | main | Implementer verification plus spec and quality review approved the live PR `#5` body; no further edits required |
| Task 2: Repair Canonical Makerv3 Doc References | completed | main | Commit `8178e2cac` passed controller verification plus spec and quality review; `pytest -q tests/unit_tests/docs/test_makerv3_doc_links.py` -> `1 passed in 0.03s` |
| Task 3: Quarantine Historical Deploy Commands And Rollout-Specific TokenMM Copy | completed | main | Commit `0bc71a5df` passed controller verification plus spec and quality review; targeted pytest slices `2 passed` and `38 passed` |
| Task 4: Close TokenMM Redis Sidecars On Shutdown | blocked | main | Preserving existing dirty edits in `systems/flux/flux/strategies/makerv3/{strategy.py,failures.py}` and `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`; task cannot be committed cleanly without taking ownership of them |
| Task 5: Refresh Fluxboard README And Add Pulse UI README | completed | main | Commits `2805ad201` and `9fd145887` passed controller verification plus spec and quality review; `pytest -q tests/unit_tests/docs/test_app_readmes_contract.py` -> `2 passed in 0.03s` |
| Task 6: Make Pulse Shell Navigation Suite-Aware | completed | main | Commits `7c837b515` and `7ee89324c` passed spec plus follow-up quality review; controller verified `pytest -q tests/unit_tests/flux/pulse/test_api.py -k shell` (`2 passed, 9 deselected`) and `pnpm --dir pulse-ui exec vitest run src/App.test.tsx` (`6 passed`) before approval |
| Task 7: Surface Pending And Deferred Pulse Group Actions | completed | main | Completed externally on the live branch via Pulse UI commits `64e2720fb` and `10203a374`; controller verified `pnpm --dir pulse-ui exec vitest run src/App.test.tsx` (`10 passed`), including the in-flight disablement, pending/deferred feedback, and duplicate-submit tests |
| Task 8: Populate Pulse Error `last_seen` End-To-End | completed | main | Backend half landed in `28f82c852`, and the live branch now carries the matching UI/test coverage from the Pulse log-triage execution; controller verification remains green on `pytest -q tests/unit_tests/flux/pulse/test_api.py -k error` (`4 passed, 7 deselected`) and `pnpm --dir pulse-ui exec vitest run src/App.test.tsx` (`10 passed`) |
| Task 9: Define Fluxboard Resync Completion Ownership | completed | main | Commit `853b1c4ab` passed controller verification (`46 passed`) and spec review with no gaps found; controller quality pass found no maintainability or regression issues in the committed resync-ack contract, tests, or socket-contract docs |
| Task 10: Realign Canonical Repo Gates And Tooling Paths | completed | main | Commit `e2eaef32b` passed controller verification: `bash tooling/ci/check-repo-structure.sh` (`OK`), `pytest -q tests/unit_tests/examples/strategies/test_tokenmm_risk_validation_contract.py` (`3 passed`), and the active-surface legacy-path search returned no matches; controller spec/quality pass found no remaining repo-standards gaps in the landed scope |

---

### Task 1: Rewrite PR Body To Match The Actual Branch Surface

**Files:**
- Modify: external PR body for PR `#5` via `gh pr edit`
- Reference: `.github/pull_request_template.md`
- Reference: `docs/plans/2026-03-09-makerv3-mono-pr-review-followups.md`
- Reference: `systems/flux/docs/makerv3.md`
- Reference: `deploy/tokenmm/README.md`
- Reference: `fluxboard/README.md`

**Step 1: Capture the failing review surface**

Run:

```bash
gh pr view 5 --json title,body,baseRefName,headRefName,url
```

Expected: the body still mentions removed paths such as `examples/live/poc/*` and old Makerv3 filenames.

**Step 2: Draft the replacement body against the template**

Draft a new PR body that:
1. follows `.github/pull_request_template.md`
2. summarizes the current canonical domains (`systems/flux/`, `deploy/tokenmm/`, `fluxboard/`, `pulse-ui/`, docs/plans/reviews)
3. links this follow-up plan as the authoritative review debt tracker
4. explains what is already validated versus what remains follow-up work

**Step 3: Apply the new PR body**

Run:

```bash
gh pr edit 5 --body-file /tmp/makerv3-pr-body.md
```

Expected: `gh` reports the PR was updated successfully.

**Step 4: Verify the live PR body**

Run:

```bash
gh pr view 5 --json body | rg -n "systems/flux|deploy/tokenmm|fluxboard|pulse-ui|2026-03-09-makerv3-mono-pr-review-followups.md"
```

Expected: the new body references the canonical branch surface and this plan.

**Step 5: Commit**

No git commit for the GitHub metadata change. If you add any supporting repo docs while drafting, commit them separately under the relevant task below.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Repair Canonical Makerv3 Doc References

**Files:**
- Modify: `docs/reviews/2026-03-04-flux-makerv3-strategy-refactor-external-review-summary.md`
- Modify: `docs/plans/2026-03-04-flux-makerv3-strategy-refactor.md`
- Reference: `systems/flux/docs/makerv3.md`
- Test: `tests/unit_tests/docs/test_makerv3_doc_links.py`

**Step 1: Write the failing test**

Create a doc-contract test that asserts the Makerv3 refactor docs point at `systems/flux/docs/makerv3.md` and do not reference removed `docs/flux/makerv3.md`.

**Step 2: Run test to verify it fails**

Run:

```bash
pytest -q tests/unit_tests/docs/test_makerv3_doc_links.py
```

Expected: FAIL because the current files still mention `docs/flux/makerv3.md`.

**Step 3: Write minimal implementation**

Update the two historical docs to:
1. point at `systems/flux/docs/makerv3.md`
2. add one short note that the document moved under the monorepo ownership split

**Step 4: Run test to verify it passes**

Run:

```bash
pytest -q tests/unit_tests/docs/test_makerv3_doc_links.py
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  docs/reviews/2026-03-04-flux-makerv3-strategy-refactor-external-review-summary.md \
  docs/plans/2026-03-04-flux-makerv3-strategy-refactor.md \
  tests/unit_tests/docs/test_makerv3_doc_links.py
git commit -m "docs(makerv3): fix canonical doc references"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Quarantine Historical Deploy Commands And Rollout-Specific TokenMM Copy

**Files:**
- Modify: `docs/plans/2026-03-04-fluxboard-tokenmm-minimal-migration.md`
- Modify: `docs/plans/2026-03-04-flux-makerv3-singleleg-productionize.md`
- Modify: `examples/live/makerv3/README.md`
- Modify: `deploy/tokenmm/README.md`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`

**Step 1: Write the failing test**

Extend the existing TokenMM doc contract to assert that:
1. current operator instructions foreground `ops/scripts/deploy/tokenmm_stack.sh` and `flux.runners.tokenmm.*`
2. `examples/live/makerv3/README.md` either includes `flux.runners.tokenmm.run_portfolio` in the bring-up order or explicitly documents that shared balances/global inventory semantics are unavailable without it
3. obsolete `scripts/deploy/makerv3_stack.sh` appears only inside explicitly marked historical context
4. `deploy/tokenmm/README.md` no longer claims the branch is the “current 7-node PLUME stack” or an in-progress March 7 rollout

**Step 2: Run test to verify it fails**

Run:

```bash
pytest -q tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -k 'historical or tokenmm'
```

Expected: FAIL on the obsolete deploy-command prominence and stale rollout wording.

**Step 3: Write minimal implementation**

Update the historical plan docs and deploy README to:
1. move obsolete commands into clearly labeled historical notes
2. put current TokenMM runner and deploy entrypoints first
3. document the `run_portfolio` sidecar requirement in the example bring-up or make the reduced local semantics explicit
4. replace time-bound rollout copy with stable contract language

**Step 4: Run test to verify it passes**

Run the same pytest command plus:

```bash
rg -n "scripts/deploy/makerv3_stack.sh|current 7-node PLUME|March 7, 2026" \
  docs/plans/2026-03-04-fluxboard-tokenmm-minimal-migration.md \
  docs/plans/2026-03-04-flux-makerv3-singleleg-productionize.md \
  deploy/tokenmm/README.md
```

Expected: only explicitly historical mentions remain, and no stale rollout statement remains in `deploy/tokenmm/README.md`.

**Step 5: Commit**

```bash
git add \
  docs/plans/2026-03-04-fluxboard-tokenmm-minimal-migration.md \
  docs/plans/2026-03-04-flux-makerv3-singleleg-productionize.md \
  examples/live/makerv3/README.md \
  deploy/tokenmm/README.md \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py
git commit -m "docs(tokenmm): quarantine obsolete deploy paths"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Close TokenMM Redis Sidecars On Shutdown

**Files:**
- Modify: `systems/flux/flux/runners/tokenmm/run_node.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`

**Step 1: Write the failing test**

Add tests that prove:
1. the runtime-params Redis client closes during node/strategy teardown
2. the portfolio-inventory Redis client closes during node/strategy teardown
3. repeated start/stop cycles do not retain old Redis sidecar clients

**Step 2: Run test to verify it fails**

Run:

```bash
pytest -q \
  tests/unit_tests/examples/strategies/test_tokenmm_run_node.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py -k 'redis or teardown or shutdown'
```

Expected: FAIL because the current wiring tests cover attachment only, not teardown/closure.

**Step 3: Write minimal implementation**

Add an explicit shutdown path that closes both Redis sidecar clients or their connection pools when the node/strategy stops.

**Step 4: Run test to verify it passes**

Run:

```bash
pytest -q \
  tests/unit_tests/examples/strategies/test_tokenmm_run_node.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py -k 'redis or teardown or shutdown'
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/runners/tokenmm/run_node.py \
  systems/flux/flux/strategies/makerv3/strategy.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_node.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py
git commit -m "fix(tokenmm): close redis sidecars on shutdown"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Refresh Fluxboard README And Add Pulse UI README

**Files:**
- Modify: `fluxboard/README.md`
- Create: `pulse-ui/README.md`
- Reference: `apps/fluxboard/docs/tokenmm_runbook.md`
- Reference: `systems/flux/flux/runners/tokenmm/run_api.py`
- Reference: `systems/flux/flux/runners/equities/run_api.py`
- Test: `tests/unit_tests/docs/test_app_readmes_contract.py`

**Step 1: Write the failing test**

Create a doc-contract test that asserts:
1. `fluxboard/README.md` links to current runbooks and does not mention removed `examples/live/makerv3_single_leg/README.md`
2. `pulse-ui/README.md` exists and documents dev/build/test/base-path behavior
3. Fluxboard docs mention both `/tokenmm/*` and `/equities/*` hosting where relevant

**Step 2: Run test to verify it fails**

Run:

```bash
pytest -q tests/unit_tests/docs/test_app_readmes_contract.py
```

Expected: FAIL because `fluxboard/README.md` is stale and `pulse-ui/README.md` does not exist yet.

**Step 3: Write minimal implementation**

Rewrite the Fluxboard README and add the Pulse UI README with:
1. current runbook links
2. current build/test commands
3. base-path/shell-hosting behavior for TokenMM, equities, and Pulse

**Step 4: Run test to verify it passes**

Run:

```bash
pytest -q tests/unit_tests/docs/test_app_readmes_contract.py
```

Expected: PASS.

**Step 5: Commit**

```bash
git add fluxboard/README.md pulse-ui/README.md tests/unit_tests/docs/test_app_readmes_contract.py
git commit -m "docs(apps): refresh fluxboard and pulse ui readmes"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 6: Make Pulse Shell Navigation Suite-Aware

**Files:**
- Modify: `pulse-ui/src/components/TopBar.tsx`
- Modify: `pulse-ui/src/api.ts`
- Modify: `systems/flux/flux/pulse/api.py`
- Test: `pulse-ui/src/App.test.tsx`
- Test: `tests/unit_tests/flux/pulse/test_api.py`

**Step 1: Write the failing tests**

Add tests that prove:
1. Pulse can render shell links for TokenMM-only hosting
2. Pulse can render TokenMM plus equities links when both are available
3. the backend exposes a canonical link/source-of-truth payload instead of forcing `tokenmm/*`

**Step 2: Run test to verify it fails**

Run:

```bash
pytest -q tests/unit_tests/flux/pulse/test_api.py -k shell
pnpm --dir pulse-ui exec vitest run src/App.test.tsx
```

Expected: FAIL because `TopBar.tsx` hardcodes `tokenmm/*` and the API does not expose suite navigation metadata.

**Step 3: Write minimal implementation**

Expose canonical shell links from Pulse API and make the UI render those links instead of embedding `tokenmm/*` constants.

**Step 4: Run test to verify it passes**

Run the same pytest and pnpm commands.

Expected: PASS for TokenMM-only and shared-host cases.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/pulse/api.py \
  pulse-ui/src/api.ts \
  pulse-ui/src/components/TopBar.tsx \
  pulse-ui/src/App.test.tsx \
  tests/unit_tests/flux/pulse/test_api.py
git commit -m "feat(pulse): make shell navigation suite-aware"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 7: Surface Pending And Deferred Pulse Group Actions

**Files:**
- Modify: `pulse-ui/src/App.tsx`
- Modify: `pulse-ui/src/components/JobGroup.tsx`
- Modify: `pulse-ui/src/api.ts`
- Test: `pulse-ui/src/App.test.tsx`

**Step 1: Write the failing test**

Add UI tests that prove:
1. group action buttons disable while the request is in flight
2. success banners include `pending` / `deferred` information when returned by the API
3. repeated clicks do not submit duplicate group actions while the first request is outstanding

**Step 2: Run test to verify it fails**

Run:

```bash
pnpm --dir pulse-ui exec vitest run src/App.test.tsx
```

Expected: FAIL because group actions currently ignore `pending` / `deferred` and do not track group-level busy state.

**Step 3: Write minimal implementation**

Track busy group actions in `App.tsx`, disable group controls while busy, and render concise operator feedback for deferred self-service responses.

**Step 4: Run test to verify it passes**

Run the same pnpm command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  pulse-ui/src/App.tsx \
  pulse-ui/src/components/JobGroup.tsx \
  pulse-ui/src/api.ts \
  pulse-ui/src/App.test.tsx
git commit -m "fix(pulse): expose deferred group action state"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 8: Populate Pulse Error `last_seen` End-To-End

**Files:**
- Modify: `systems/flux/flux/pulse/api.py`
- Modify: `pulse-ui/src/components/JobRow.tsx`
- Test: `tests/unit_tests/flux/pulse/test_api.py`
- Test: `pulse-ui/src/App.test.tsx`

**Step 1: Write the failing tests**

Add tests that prove:
1. `_extract_error_info()` returns the timestamp of the latest matching error line
2. the `/api/pulse/jobs` payload includes `errors.last_seen`
3. the UI renders that timestamp when present and preserves current behavior when absent

**Step 2: Run test to verify it fails**

Run:

```bash
pytest -q tests/unit_tests/flux/pulse/test_api.py -k error
pnpm --dir pulse-ui exec vitest run src/App.test.tsx
```

Expected: FAIL because the API hardcodes `last_seen` to `None`.

**Step 3: Write minimal implementation**

Parse the journald timestamp from the last matched error line, return it in the API payload, and render it in the existing UI affordance.

**Step 4: Run test to verify it passes**

Run the same pytest and pnpm commands.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/pulse/api.py \
  pulse-ui/src/components/JobRow.tsx \
  tests/unit_tests/flux/pulse/test_api.py \
  pulse-ui/src/App.test.tsx
git commit -m "fix(pulse): publish error last-seen timestamps"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 9: Define Fluxboard Resync Completion Ownership

**Files:**
- Modify: `fluxboard/stores.ts`
- Modify: `fluxboard/stores/orderViewStore.ts`
- Modify: `fluxboard/docs/tokenmm_socket_contract.md`
- Test: `fluxboard/__tests__/resync-contract.test.tsx`
- Test: `fluxboard/__tests__/TradesStore.test.ts`
- Test: `fluxboard/stores/orderViewStore.test.ts`

**Step 1: Write the failing tests**

Add tests that prove:
1. resync completion does not clear after only one consumer acknowledges the current epoch
2. both Trades and Order View must acknowledge the epoch before `isResyncing` clears
3. stale acknowledgements from an older epoch do not clear the current resync

**Step 2: Run test to verify it fails**

Run:

```bash
pnpm --dir fluxboard exec vitest run \
  __tests__/resync-contract.test.tsx \
  __tests__/TradesStore.test.ts \
  stores/orderViewStore.test.ts
```

Expected: FAIL because the current store clears `isResyncing` after the first consumer applies the epoch.

**Step 3: Write minimal implementation**

Define one authoritative resync-ack contract in `stores.ts`, wire both consumers into it, and document that ownership in the socket contract.

**Step 4: Run test to verify it passes**

Run the same vitest command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  fluxboard/stores.ts \
  fluxboard/stores/orderViewStore.ts \
  fluxboard/docs/tokenmm_socket_contract.md \
  fluxboard/__tests__/resync-contract.test.tsx \
  fluxboard/__tests__/TradesStore.test.ts \
  fluxboard/stores/orderViewStore.test.ts
git commit -m "fix(fluxboard): define resync completion contract"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 10: Realign Canonical Repo Gates And Tooling Paths

**Files:**
- Create: `ops/scripts/tokenmm_risk_audit.py`
- Modify: `scripts/ops/tokenmm_risk_audit.py`
- Modify: `docs/runbooks/tokenmm-risk-validation.md`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_risk_validation_contract.py`
- Modify: `.github/actions/common-setup/action.yml`
- Modify: `.github/workflows/build.yml`
- Modify: `.github/workflows/coverage.yml`
- Modify: `tooling/ci/check-repo-structure.sh`
- Modify: `tooling/ci/check-flux-leakage.sh`
- Modify: `docs/developer_guide/testing.md`
- Modify: `docs/repo/structure.md`
- Modify: `README.md`
- Modify: `CONTRIBUTING.md`

**Step 1: Write the failing tests/checks**

Capture the current drift with:

```bash
bash tooling/ci/check-repo-structure.sh
pytest -q tests/unit_tests/examples/strategies/test_tokenmm_risk_validation_contract.py
rg -n "scripts/rust-toolchain.sh|scripts/python-version.sh|scripts/pre-commit-version.sh|scripts/package-version.sh|pnpm --dir fluxboard|codecov|nautilus_trader/flux" \
  .github docs README.md tooling
```

Expected:
1. repo-structure check fails on `scripts/ops/tokenmm_risk_audit.py`
2. docs/workflows still reference legacy helper paths, paused coverage behavior, root-app paths, or legacy Flux scan targets

**Step 2: Run test to verify it fails**

Treat the command output above as the RED state for this standards task. Do not change implementation until the failure set is captured in notes/tests.

**Step 3: Write minimal implementation**

Implement the standards realignment:
1. move the canonical TokenMM risk audit script under `ops/scripts/` and leave `scripts/ops/` as a compatibility shim only
2. update runbooks/tests to point at the canonical script location
3. migrate active workflow helper references away from legacy `scripts/*`
4. make `tooling/ci/check-repo-structure.sh` reject active workflow references to legacy helper paths
5. retarget `tooling/ci/check-flux-leakage.sh` to canonical `systems/flux/flux` and current app docs paths
6. reconcile `docs/developer_guide/testing.md`, `.github/workflows/coverage.yml`, `docs/repo/structure.md`, `README.md`, and `CONTRIBUTING.md` with the actual coverage, PR-template, and app-path story

**Step 4: Run test to verify it passes**

Run:

```bash
bash tooling/ci/check-repo-structure.sh
pytest -q tests/unit_tests/examples/strategies/test_tokenmm_risk_validation_contract.py
rg -n "scripts/rust-toolchain.sh|scripts/python-version.sh|scripts/pre-commit-version.sh|scripts/package-version.sh|nautilus_trader/flux" \
  .github docs README.md tooling
```

Expected:
1. repo-structure check passes
2. TokenMM risk-validation contract passes against the new canonical audit path
3. no active workflow/docs references remain for the legacy helper paths or legacy Flux scan root

**Step 5: Commit**

```bash
git add \
  ops/scripts/tokenmm_risk_audit.py \
  scripts/ops/tokenmm_risk_audit.py \
  docs/runbooks/tokenmm-risk-validation.md \
  tests/unit_tests/examples/strategies/test_tokenmm_risk_validation_contract.py \
  .github/actions/common-setup/action.yml \
  .github/workflows/build.yml \
  .github/workflows/coverage.yml \
  tooling/ci/check-repo-structure.sh \
  tooling/ci/check-flux-leakage.sh \
  docs/developer_guide/testing.md \
  docs/repo/structure.md \
  README.md \
  CONTRIBUTING.md
git commit -m "chore(repo): align tooling with canonical paths"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.
