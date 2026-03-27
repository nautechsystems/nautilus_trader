# TokenMM Binance Shared-Account And Startup Reconciliation Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Restore TokenMM Binance production correctness by publishing one authoritative shared Binance collateral row and by auto-repairing the proven stale Binance spot startup cache shape without weakening fail-closed startup behavior.

**Architecture:** Treat the duplicate Binance USDT rows and the Binance spot startup failure as two related but separate reconciliation problems. The balances fix must come from explicit TokenMM shared-account contracts plus a Binance shared-account projection path that overlays the portfolio snapshot with one authoritative shared row; it must not be a UI-only or row-picking workaround. The startup fix stays in `execution_engine.py` and should only auto-clean up when the engine can prove the remaining startup orders are cache-only artifacts already missing at venue truth.

**Tech Stack:** Python, TOML, Redis-backed Flux portfolio/API runners, Nautilus live execution engine, pytest.

**Context Docs:**
- Design: `docs/plans/2026-03-26-tokenmm-startup-auto-repair-design.md`
- PRD: `none`
- Relevant specs/runbooks: `docs/architecture/tokenmm-portfolio-inventory-semantics.md`, `docs/plans/2026-03-26-tokenmm-startup-auto-repair.md`, `docs/runbooks/deploy-lanes.md`

**Decision Summary:**
- The duplicate Binance stable row fix must use shared-account provenance (`account_scope_id`, `source_scope`, `source_strategy_ids`) and an authoritative shared account projection, not a display-only dedupe.
- Binance spot and Binance perp should map to one TokenMM execution account scope for shared collateral semantics even though they still publish distinct strategy-local positions.
- Unsupported TokenMM venues do not need fake shared-account projectors in this wave; only scopes with a real provider should publish account projections.
- Startup reconciliation must remain fail-closed unless venue-flat state plus missing-at-venue startup order evidence proves the cache is stale.
- The telemetry shipper hotfix already shipped in prod on March 27, 2026 and is not part of this plan beyond preserving the config fix.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Task 1: Declare TokenMM shared-account contracts for Binance and portfolio consumers | completed | main | none | `deploy/tokenmm/tokenmm.live.toml`, `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py` | `lanes/task-1-tokenmm-contracts` | `/home/ubuntu/nautilus_trader/.worktrees/task-1-tokenmm-contracts` | `473f56cbb44ec59abc8d0f1ae94ceae12c5c313d` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -k "shared_account_scope or strategy_contract" PASS; PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -> 3 unrelated FAIL` | 2026-03-27: quality review passed; controller branch already matched the approved manifest diff, and targeted verification passed locally |
| Task 2: Thread TokenMM account-scope metadata through merged portfolio balance rows | completed | main | Task 1: Declare TokenMM shared-account contracts for Binance and portfolio consumers | `systems/flux/flux/common/strategy_contracts.py`, `systems/flux/flux/common/portfolio_snapshot.py`, `systems/flux/flux/api/_payloads_balances.py`, `systems/flux/flux/runners/tokenmm/run_portfolio.py`, `systems/flux/flux/runners/shared/portfolio_runner.py`, `tests/unit_tests/flux/common/test_portfolio_snapshot.py`, `tests/unit_tests/flux/api/test_balances_merge_dedupe.py` | `shared` | `shared` | `e21b069367` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/api/test_balances_merge_dedupe.py -k "binance and account_scope" PASS; PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/common/test_portfolio_snapshot.py -k "tokenmm and shared_account and binance" PASS` | 2026-03-27: controller committed scope plumbing through merge/snapshot path |
| Task 3: Add authoritative Binance shared-account projection and overlay it into TokenMM balances | completed | main | Task 1: Declare TokenMM shared-account contracts for Binance and portfolio consumers, Task 2: Thread TokenMM account-scope metadata through merged portfolio balance rows | `systems/flux/flux/runners/shared/profile_accounts.py`, `systems/flux/flux/runners/tokenmm/run_api.py`, `systems/flux/flux/api/app.py`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py` | `shared` | `shared` | `a221724720` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -p pytest_asyncio.plugin -q tests/unit_tests/flux/runners/shared/test_profile_accounts.py PASS; PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/api/test_app.py -k "discovers_strategy_ids_before_shared_account_overlay or tokenmm and binance and shared_account" PASS; PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_api.py -k strategy_contracts PASS` | 2026-03-27: quality review passed after projection-health and ISOLATED_MARGIN guard follow-up |
| Task 4: Auto-repair the live Binance spot stale-startup cache shape without weakening strict guards | in_review_quality | quality-reviewer | none | `nautilus_trader/live/execution_engine.py`, `tests/unit_tests/live/test_execution_recon.py`, `tests/unit_tests/live/test_execution_engine.py` | `lanes/task-4-binance-startup` | `/home/ubuntu/nautilus_trader/.worktrees/task-4-binance-startup` | `5388b8212e` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -p pytest_asyncio.plugin -q tests/unit_tests/live/test_execution_engine.py -k "mass_status_account_for_accountless_reports or accountless_reports or startup and cleanup or matching_account_scope" PASS; PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -p pytest_asyncio.plugin -q tests/unit_tests/live/test_execution_recon.py -k "binance_spot and startup" PASS` | 2026-03-27: reviewer-driven fallback-account fix applied; final quality re-review pending |
| Task 5: Verify in dev, deploy to pilot, validate, then promote the tested release to prod | not_started | unassigned | Task 2: Thread TokenMM account-scope metadata through merged portfolio balance rows, Task 3: Add authoritative Binance shared-account projection and overlay it into TokenMM balances, Task 4: Auto-repair the live Binance spot stale-startup cache shape without weakening strict guards | `deploy/tokenmm/README.md`, `docs/fluxboard/tokenmm_runbook.md`, `docs/plans/2026-03-27-tokenmm-binance-shared-account-reconciliation.md` | `shared` | `shared` | none | not_run | Plan created |

---

### Task 1: Declare TokenMM shared-account contracts for Binance and portfolio consumers

**Files:**
- Modify: `deploy/tokenmm/tokenmm.live.toml`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`

**Dependencies:** `none`

**Write Scope:** `deploy/tokenmm/tokenmm.live.toml`, `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -k "shared_account_scope or strategy_contract"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`

**Step 1: Write the failing contract tests**

Add focused TokenMM config contract tests that assert all of the following:
- `deploy/tokenmm/tokenmm.live.toml` declares `[[strategy_contracts]]` rows for the live TokenMM strategy allowlist.
- `plumeusdt_binance_spot_makerv3` and `plumeusdt_binance_perp_makerv3` share one `execution_account_scope_id`.
- The TokenMM config declares at least one concrete Binance account-scope provider for that shared execution scope.
- The manifest does not silently regress the March 27 telemetry shipper top-level path contract.

**Step 2: Run the targeted test to verify it fails**

Run: `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -k "shared_account_scope or strategy_contract"`
Expected: FAIL because TokenMM currently has no `[[strategy_contracts]]` or `[[account_scopes]]` manifest.

**Step 3: Add the minimal manifest**

In `deploy/tokenmm/tokenmm.live.toml`:
- Add `[[strategy_contracts]]` entries for the live TokenMM strategies that participate in the shared portfolio snapshot.
- Give both Binance strategies the same `execution_account_scope_id` so their shared collateral can be reasoned about as one account scope.
- Keep `reference_account_scope_id` deliberately simple; do not invent extra reference-account providers that the code will not use in this wave.
- Add the Binance shared account scope row under `[[account_scopes]]` with real provider settings and keep any non-Binance placeholder scopes intentionally inert.

**Step 4: Run the contract tests to verify they pass**

Run: `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`
Expected: PASS, including the new TokenMM shared-account manifest assertions and the telemetry shipper parse regression.

**Step 5: Commit**

```bash
git add deploy/tokenmm/tokenmm.live.toml tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py
git commit -m "feat(tokenmm): declare shared account contracts for binance"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Thread TokenMM account-scope metadata through merged portfolio balance rows

**Files:**
- Modify: `systems/flux/flux/common/strategy_contracts.py`
- Modify: `systems/flux/flux/common/portfolio_snapshot.py`
- Modify: `systems/flux/flux/api/_payloads_balances.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_portfolio.py`
- Modify: `systems/flux/flux/runners/shared/portfolio_runner.py`
- Modify: `tests/unit_tests/flux/common/test_portfolio_snapshot.py`
- Modify: `tests/unit_tests/flux/api/test_balances_merge_dedupe.py`

**Dependencies:** `Task 1: Declare TokenMM shared-account contracts for Binance and portfolio consumers`

**Write Scope:** `systems/flux/flux/common/strategy_contracts.py`, `systems/flux/flux/common/portfolio_snapshot.py`, `systems/flux/flux/api/_payloads_balances.py`, `systems/flux/flux/runners/tokenmm/run_portfolio.py`, `systems/flux/flux/runners/shared/portfolio_runner.py`, `tests/unit_tests/flux/common/test_portfolio_snapshot.py`, `tests/unit_tests/flux/api/test_balances_merge_dedupe.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/common/test_portfolio_snapshot.py -k "tokenmm and shared_account"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/api/test_balances_merge_dedupe.py -k "binance and account_scope"`

**Step 1: Write the failing metadata tests**

Add focused tests that lock in these requirements:
- merged TokenMM portfolio cash rows can carry `account_scope_id` and `source_strategy_ids` derived from strategy contracts
- a shared-scope stable cash row should use the account scope as canonical provenance instead of depending on the raw venue account string
- identical non-stable multi-product cash dedupe behavior stays unchanged

**Step 2: Run the new tests to verify they fail**

Run: `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/common/test_portfolio_snapshot.py -k "tokenmm and shared_account"`
Expected: FAIL because TokenMM merged balance rows currently discard strategy-contract account scope metadata.

**Step 3: Implement the minimal metadata path**

Make the smallest set of changes required to thread account-scope metadata through the TokenMM snapshot pipeline:
- add a helper in `strategy_contracts.py` that resolves `strategy_id -> execution_account_scope_id`
- extend the TokenMM portfolio runner to pass that mapping into the portfolio balance merge path
- extend `merge_portfolio_balances_rows` / `build_portfolio_balance_rows` so merged cash rows keep `account_scope_id`, `source_strategy_ids`, and shared-account scope tags when the contributing strategies share a scope
- keep position netting behavior unchanged outside the existing shared-observation grouping

**Step 4: Run the focused tests to verify they pass**

Run:
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/common/test_portfolio_snapshot.py -k "tokenmm and shared_account"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/api/test_balances_merge_dedupe.py -k "binance and account_scope"`
Expected: PASS, with TokenMM merged cash rows now carrying the metadata needed for later overlay and canonicalization.

**Step 5: Commit**

```bash
git add systems/flux/flux/common/strategy_contracts.py systems/flux/flux/common/portfolio_snapshot.py systems/flux/flux/api/_payloads_balances.py systems/flux/flux/runners/tokenmm/run_portfolio.py systems/flux/flux/runners/shared/portfolio_runner.py tests/unit_tests/flux/common/test_portfolio_snapshot.py tests/unit_tests/flux/api/test_balances_merge_dedupe.py
git commit -m "feat(tokenmm): carry shared account scope metadata into portfolio rows"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Add authoritative Binance shared-account projection and overlay it into TokenMM balances

**Files:**
- Modify: `systems/flux/flux/runners/shared/profile_accounts.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_api.py`
- Modify: `systems/flux/flux/api/app.py`
- Modify: `tests/unit_tests/flux/api/test_app.py`
- Create: `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`

**Dependencies:** `Task 1: Declare TokenMM shared-account contracts for Binance and portfolio consumers`, `Task 2: Thread TokenMM account-scope metadata through merged portfolio balance rows`

**Write Scope:** `systems/flux/flux/runners/shared/profile_accounts.py`, `systems/flux/flux/runners/tokenmm/run_api.py`, `systems/flux/flux/api/app.py`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -p pytest_asyncio.plugin -q tests/unit_tests/flux/runners/shared/test_profile_accounts.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/api/test_app.py -k "tokenmm and binance and shared_account"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_api.py -k strategy_contracts`

**Step 1: Write the failing projection and API tests**

Add focused tests that reproduce the current production symptom:
- two Binance strategy cash snapshots with different raw `account_id`s must resolve to one authoritative shared Binance USDT row once a fresh TokenMM portfolio snapshot includes account projections
- the live fallback path must use shared-account projection rows plus strategy-contract scope mapping when no fresh portfolio snapshot is available
- the shared row must keep `source_scope="shared_account"` and the correct `account_scope_id`
- TokenMM `global_qty_base` and component completeness must not change just because stable cash rows are canonicalized

**Step 2: Run the tests to verify they fail**

Run:
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/api/test_app.py -k "tokenmm and binance and shared_account"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_api.py -k strategy_contracts`
Expected: FAIL because TokenMM currently does not publish a Binance shared-account projection and the TokenMM API snapshot path does not combine `balances.rows` with `accounts.rows`.

**Step 3: Implement the minimal authoritative projection path**

Make the following changes and nothing broader:
- refactor the Binance projection path in `profile_accounts.py` so it can publish one authoritative shared Binance collateral snapshot for the configured account scope
- keep the provider output explicitly tagged as `shared_account`
- pass `strategy_contracts=config.get("strategy_contracts")` from `tokenmm/run_api.py` into `create_flux_api_app`
- update the TokenMM fresh-snapshot branch in `api/app.py` to mirror the equities overlay path: combine `portfolio_snapshot["balances"]["rows"]` with `portfolio_snapshot["accounts"]["rows"]`, preserve shared-account rows, and compute totals/risk groups from the combined reconciliation rows
- update the TokenMM live fallback branch in `api/app.py` to combine merged strategy rows with shared-account projection rows and to derive `account_scope_id` from strategy contracts before the merge

**Step 4: Run the focused tests to verify they pass**

Run:
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -p pytest_asyncio.plugin -q tests/unit_tests/flux/runners/shared/test_profile_accounts.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/api/test_app.py -k "tokenmm and binance and shared_account"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_api.py -k strategy_contracts`
Expected: PASS, with one shared Binance USDT row in the TokenMM snapshot-backed response.

**Step 5: Commit**

```bash
git add systems/flux/flux/runners/shared/profile_accounts.py systems/flux/flux/runners/tokenmm/run_api.py systems/flux/flux/api/app.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/runners/shared/test_profile_accounts.py tests/unit_tests/examples/strategies/test_tokenmm_run_api.py
git commit -m "fix(tokenmm): overlay authoritative binance shared balances"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Auto-repair the live Binance spot stale-startup cache shape without weakening strict guards

**Files:**
- Modify: `nautilus_trader/live/execution_engine.py`
- Modify: `tests/unit_tests/live/test_execution_recon.py`
- Modify: `tests/unit_tests/live/test_execution_engine.py`

**Dependencies:** `none`

**Write Scope:** `nautilus_trader/live/execution_engine.py`, `tests/unit_tests/live/test_execution_recon.py`, `tests/unit_tests/live/test_execution_engine.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/live/test_execution_recon.py -k "binance_spot and startup"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/live/test_execution_engine.py -k "startup and cleanup"`

**Step 1: Write the failing live-shape regression**

Add a regression that matches the current production evidence more closely than the March 26 test:
- restored startup net short position
- multiple cached startup open orders
- venue open-order sweep returns nothing
- targeted startup order queries prove the cached orders are missing at venue
- venue position report is flat
- startup fill reports are absent or insufficient
- `generate_missing_orders = false`

Assert that the current code still fails closed even though the remaining startup orders are cache-only artifacts.

**Step 2: Run the test to verify it fails**

Run: `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/live/test_execution_recon.py -k "binance_spot and startup"`
Expected: FAIL because the current stale-startup cleanup gate still sees open cache orders and refuses cleanup.

**Step 3: Implement the minimal repair**

Adjust `execution_engine.py` so startup cleanup can proceed only when the engine has already proven the remaining startup open orders are missing at venue truth. Keep the guardrails explicit:
- do not bypass the existing case where the bulk open-order sweep still reports a live order
- do not create synthetic missing orders
- keep ambiguous shapes on the `ambiguous_startup_mismatch` fail-closed path
- keep the current unmatched-fill repair path intact for the earlier March 26 shape

**Step 4: Run the focused startup suite**

Run:
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/live/test_execution_recon.py -k "binance_spot and startup"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/live/test_execution_engine.py -k "startup and cleanup"`
Expected: PASS, including the new cache-only Binance regression and the existing strict guard that still blocks cleanup when venue truth reports a live startup order.

**Step 5: Commit**

```bash
git add nautilus_trader/live/execution_engine.py tests/unit_tests/live/test_execution_recon.py tests/unit_tests/live/test_execution_engine.py
git commit -m "fix(reconciliation): repair stale binance startup cache only when venue proof is sufficient"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Verify in dev, deploy to pilot, validate, then promote the tested release to prod

**Files:**
- Modify: `deploy/tokenmm/README.md`
- Modify: `docs/fluxboard/tokenmm_runbook.md`
- Modify: `docs/plans/2026-03-27-tokenmm-binance-shared-account-reconciliation.md`

**Dependencies:** `Task 2: Thread TokenMM account-scope metadata through merged portfolio balance rows`, `Task 3: Add authoritative Binance shared-account projection and overlay it into TokenMM balances`, `Task 4: Auto-repair the live Binance spot stale-startup cache shape without weakening strict guards`

**Write Scope:** `deploy/tokenmm/README.md`, `docs/fluxboard/tokenmm_runbook.md`, `docs/plans/2026-03-27-tokenmm-binance-shared-account-reconciliation.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/common/test_account_projection.py tests/unit_tests/flux/common/test_portfolio_snapshot.py tests/unit_tests/flux/api/test_balances_merge_dedupe.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/api/test_app.py -k "tokenmm"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/live/test_execution_recon.py -k "startup"`
- `curl -fsS http://127.0.0.1:5022/api/v1/balances?profile=tokenmm | jq '.data.rows[] | select(.asset==\"USDT\" and (.exchange|test(\"binance\")))'`
- `curl -fsS http://127.0.0.1:5022/api/v1/readiness?profile=tokenmm | jq`

**Step 1: Run the focused dev verification suite**

Run the pytest commands above and record exact pass/fail status in the tracker before any live rollout work.

**Step 2: Update operator docs**

Document the new behavior in `deploy/tokenmm/README.md` and `docs/fluxboard/tokenmm_runbook.md`:
- TokenMM balances should show one authoritative Binance shared stable row
- the Binance spot node still fails closed for ambiguous startup mismatches
- live rollout must use pilot first and promote the exact tested pilot release to prod

**Step 3: Deploy to pilot and validate**

Use the deploy-lane contract from `docs/runbooks/deploy-lanes.md`:
- create a pinned `pilot` TokenMM release from the verified dev checkout
- restart only the pilot TokenMM services
- confirm pilot `/api/v1/balances?profile=tokenmm` shows one shared Binance USDT row
- confirm pilot Binance spot startup completes without the stale-cache mismatch

**Step 4: Promote the tested pilot release to prod**

Promote the exact validated pilot release to `prod`, then verify:
- prod `/api/v1/balances?profile=tokenmm` shows one shared Binance USDT row
- prod `/api/v1/readiness?profile=tokenmm` includes the Binance spot node as healthy
- `flux@tokenmm-telemetry-shipper.service` remains healthy on the already-fixed config path

**Step 5: Commit**

```bash
git add deploy/tokenmm/README.md docs/fluxboard/tokenmm_runbook.md docs/plans/2026-03-27-tokenmm-binance-shared-account-reconciliation.md
git commit -m "docs(tokenmm): record shared-account rollout and startup repair procedure"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
