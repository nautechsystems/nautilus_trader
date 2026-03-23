# Equities Balances Reconciliation Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make `equities.fluxboard.balances` a trustworthy independent reconciliation surface by restoring fresh shared-account projections where possible, preventing stale shared-account rows from being silently merged into equities balances, and surfacing degraded scope state clearly when a scope still cannot refresh.

**Architecture:** Keep the existing shared-account projection pipeline, but make freshness and failure state explicit per account scope instead of inferring health from page render time. Fix the two live provider failures first: restore the IBKR reference balance refresh path so it can reconnect and publish fresh rows again after timeout failures, and harden the Binance portfolio-margin shared-account provider against live position-risk payload drift. Then tighten the equities balances API and Fluxboard so stale scopes are obvious and do not masquerade as current reconciliation truth: stale shared-account rows remain visible for operators, but they are flagged as stale and excluded from fresh reconciliation totals and risk group calculations.

**Tech Stack:** Python, Flask API, Redis, Nautilus Trader live adapters (IBKR, Binance Futures PM, Hyperliquid), React/TypeScript Fluxboard, pytest, npm test, systemd live services.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Task 1: Lock the failing reconciliation contract in tests | completed | lead | none | `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`, `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`, `tests/unit_tests/flux/common/test_account_projection.py`, `tests/unit_tests/flux/common/test_portfolio_snapshot.py`, `fluxboard/__tests__/store-freshness-contract.test.ts` | shared | shared | `18a6e1a2b4`, `accc98e146`, `07bd425b6f`, `d32cf7150a` | Focused `pytest` slices remain red for the intended missing production behaviors: `last_attempt_ts_ms`, provider `projection_status`, `scope_status`, stale-row preservation, and degraded overlay/fallback API handling | Red contract locked; review findings folded back into the suite until no open known gaps remained |
| Task 2: Restore IBKR shared-account refresh reliability and freshness semantics | completed | lead | Task 1: Lock the failing reconciliation contract in tests | `systems/flux/flux/strategies/makerv4/reference_balances.py`, `systems/flux/flux/common/account_projection.py`, `systems/flux/flux/runners/shared/portfolio_runner.py`, `nautilus_trader/adapters/interactive_brokers/factories.py`, `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`, `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`, `tests/unit_tests/adapters/interactive_brokers/test_gateway.py`, `tests/unit_tests/flux/common/test_account_projection.py`, `tests/unit_tests/flux/common/test_portfolio_snapshot.py` | shared | shared | working tree | passed | Added standalone IBKR client invalidation/reset, explicit provider `projection_status`, stale-row preservation on refresh failures, and account-scope propagation into portfolio snapshots; focused and full touched backend suites are green |
| Task 3: Fix Binance PM shared-account projection parsing and publishing | completed | lead | Task 1: Lock the failing reconciliation contract in tests | `systems/flux/flux/runners/shared/profile_accounts.py`, `nautilus_trader/adapters/binance/futures/http/account.py`, `nautilus_trader/adapters/binance/futures/schemas/account.py`, `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`, `tests/integration_tests/adapters/binance/test_execution_futures.py`, `tests/integration_tests/adapters/binance/test_providers.py` | shared | shared | working tree | passed | Current branch already carried the PM parser drift fix; this execution completed the shared-account publishing contract by adding explicit success/failure status and stale-total suppression to the Binance projection path |
| Task 4: Make the equities balances API fail-safe and per-scope explicit | completed | lead | Task 2: Restore IBKR shared-account refresh reliability and freshness semantics, Task 3: Fix Binance PM shared-account projection parsing and publishing | `systems/flux/flux/api/app.py`, `systems/flux/flux/common/account_projection.py`, `systems/flux/flux/runners/equities/readiness.py`, `ops/scripts/deploy/check_equities_live_readiness.sh`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`, `tests/unit_tests/examples/strategies/test_equities_readiness.py`, `tests/unit_tests/flux/common/test_account_projection.py`, `tests/unit_tests/flux/common/test_portfolio_snapshot.py` | shared | shared | working tree | passed | `/api/v1/balances?profile=equities` now exposes `scope_status`, marks `degraded` on unhealthy scopes in both fallback and `portfolio_snapshot_v2` paths, and excludes stale/excluded shared-account rows from response totals and risk groups while leaving them visible in `rows` |
| Task 5: Surface balances freshness and degraded scopes in Fluxboard | completed | lead | Task 4: Make the equities balances API fail-safe and per-scope explicit | `fluxboard/api.ts`, `fluxboard/types.ts`, `fluxboard/stores.ts`, `fluxboard/Balances.tsx`, `fluxboard/components/panels/BalancesPanel.tsx`, `fluxboard/Balances.test.tsx`, `fluxboard/api.flux.test.ts`, `fluxboard/__tests__/store-freshness-contract.test.ts` | shared | shared | working tree | passed | Fluxboard balances now preserves `degraded`/`scope_status`, renders a shared-account reconciliation status bar, and keeps the freshness selector anchored to backend `generated_at` rather than local receive time |
| Task 6: Verify live reconciliation behavior and rollout guards | completed | lead | Task 2: Restore IBKR shared-account refresh reliability and freshness semantics, Task 3: Fix Binance PM shared-account projection parsing and publishing, Task 4: Make the equities balances API fail-safe and per-scope explicit, Task 5: Surface balances freshness and degraded scopes in Fluxboard | `docs/plans/2026-03-23-equities-balances-reconciliation.md` | shared | shared | working tree | passed | Verification complete for touched backend/frontend suites; one unrelated existing red remains outside scope in `tests/unit_tests/flux/api/test_app.py::test_signals_profile_tokenmm_overlays_portfolio_inventory_metadata_onto_rows` when that entire file is run, but the balances slices and full equities profile contract are green |

---

### Task 1: Lock the failing reconciliation contract in tests

**Files:**
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`
- Modify: `tests/unit_tests/flux/api/test_app.py`
- Modify: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Modify: `tests/unit_tests/flux/common/test_account_projection.py`
- Modify: `tests/unit_tests/flux/common/test_portfolio_snapshot.py`
- Modify: `fluxboard/__tests__/store-freshness-contract.test.ts`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`, `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`, `tests/unit_tests/flux/common/test_account_projection.py`, `tests/unit_tests/flux/common/test_portfolio_snapshot.py`, `fluxboard/__tests__/store-freshness-contract.test.ts`

**Verification Commands:**
- `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`
- `./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -k 'binance or projection or stale'`
- `./.venv/bin/pytest -q tests/unit_tests/flux/api/test_app.py -k 'balances_profile_equities'`
- `./.venv/bin/pytest -q tests/unit_tests/flux/api/test_equities_profile_contract.py -k 'balances'`
- `./.venv/bin/pytest -q tests/unit_tests/flux/common/test_account_projection.py tests/unit_tests/flux/common/test_portfolio_snapshot.py`
- `cd fluxboard && npm test -- --runInBand store-freshness-contract`

**Step 1: Write the failing IBKR provider tests**

Add tests that pin the live failure shape instead of the happy path only:

```python
def test_ibkr_reference_balance_provider_does_not_refresh_success_timestamp_on_timeout(...):
    provider = IbkrReferenceBalanceSnapshotProvider(...)
    provider._latest_snapshot = {"rows": [{"asset": "AAPL"}], "server_ts_ms": 1000}
    provider._last_refresh_monotonic = 10.0
    ...
    assert snapshot["projection_status"]["healthy"] is False
    assert snapshot["projection_status"]["last_error_type"] == "TimeoutError"
    assert snapshot["projection_status"]["last_success_ts_ms"] == 1000
```

```python
def test_equities_portfolio_aggregator_marks_stale_projection_scope_without_republishing_ancient_rows(...):
    ...
    assert snapshot["accounts"]["scope_status"][0]["account_scope_id"] == "ibkr.reference.main"
    assert snapshot["accounts"]["scope_status"][0]["stale"] is True
```

**Step 2: Write the failing Binance PM shared-account tests**

Add tests that recreate the live payload drift:

```python
def test_equities_portfolio_runner_parses_portfolio_margin_position_risk_without_isolated_margin(...):
    ...
    assert snapshot["rows"][0]["instrument_id"] == "INTCUSDT-PERP.BINANCE_PERP"
```

```python
def test_equities_portfolio_runner_keeps_previous_binance_snapshot_status_when_refresh_fails(...):
    ...
    assert snapshot["projection_status"]["last_error_type"] == "ValidationError"
    assert snapshot["projection_status"]["healthy"] is False
```

**Step 3: Write the failing API contract tests**

Add tests that pin the desired fail-safe behavior for `/api/v1/balances?profile=equities`:

```python
def test_balances_profile_equities_marks_stale_shared_account_scope_in_response(...):
    ...
    assert body["data"]["degraded"] is True
    assert body["data"]["scope_status"][0]["account_scope_id"] == "ibkr.reference.main"
    assert body["data"]["scope_status"][0]["stale"] is True
```

```python
def test_balances_profile_equities_does_not_treat_stale_projection_rows_as_fresh_reconciliation_truth(...):
    ...
    assert body["data"]["totals"]["account_equity_raw"] == pytest.approx(expected_without_stale_scope)
    assert any(row.get("stale") for row in body["data"]["rows"] if row["account_scope_id"] == "ibkr.reference.main")
    assert all(
        not row.get("include_in_reconciliation", True)
        for row in body["data"]["rows"]
        if row["account_scope_id"] == "ibkr.reference.main"
    )
```

```python
def test_balances_profile_equities_keeps_healthy_scopes_fresh_when_one_scope_is_stale(...):
    ...
    assert body["data"]["degraded"] is True
    assert any(
        row["account_scope_id"] == "binance.futures.main"
        and not row.get("stale", False)
        and row.get("include_in_reconciliation", False)
        for row in body["data"]["rows"]
    )
    assert any(
        scope["account_scope_id"] == "ibkr.reference.main" and scope["stale"]
        for scope in body["data"]["scope_status"]
    )
```

Also extend the common snapshot-builder tests so the low-level contracts preserve:
- `projection_status`
- `scope_status`
- row-level `stale`
- row-level `include_in_reconciliation`

**Step 4: Write the failing Fluxboard freshness tests**

Add store/UI tests that prove balances freshness is driven by backend data freshness, not websocket receive time:

```ts
it('prefers degraded balances data timestamp over last receive time', () => {
  expect(selectBalancesFreshnessTs(state)).toBe(backendTs);
});
```

**Step 5: Run the focused test slices and verify they fail**

Run:

```bash
./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py
./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -k 'binance or projection or stale'
./.venv/bin/pytest -q tests/unit_tests/flux/api/test_app.py -k 'balances_profile_equities'
./.venv/bin/pytest -q tests/unit_tests/flux/api/test_equities_profile_contract.py -k 'balances'
cd fluxboard && npm test -- --runInBand store-freshness-contract
```

Expected: FAIL on the new tests because the current branch still serves stale IBKR rows, does not expose per-scope degradation clearly enough, and still relies on simplistic balances panel freshness.

**Step 6: Commit the red contract**

```bash
git add \
  tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/flux/common/test_account_projection.py \
  tests/unit_tests/flux/common/test_portfolio_snapshot.py \
  fluxboard/__tests__/store-freshness-contract.test.ts
git commit -m "test(equities): lock balances reconciliation failures"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Restore IBKR shared-account refresh reliability and freshness semantics

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/reference_balances.py`
- Modify: `systems/flux/flux/common/account_projection.py`
- Modify: `systems/flux/flux/runners/shared/portfolio_runner.py`
- Modify: `nautilus_trader/adapters/interactive_brokers/factories.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`
- Test: `tests/unit_tests/adapters/interactive_brokers/test_gateway.py`
- Test: `tests/unit_tests/flux/common/test_account_projection.py`
- Test: `tests/unit_tests/flux/common/test_portfolio_snapshot.py`

**Dependencies:** `Task 1: Lock the failing reconciliation contract in tests`

**Write Scope:** `systems/flux/flux/strategies/makerv4/reference_balances.py`, `systems/flux/flux/common/account_projection.py`, `systems/flux/flux/runners/shared/portfolio_runner.py`, `nautilus_trader/adapters/interactive_brokers/factories.py`, `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`, `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`, `tests/unit_tests/adapters/interactive_brokers/test_gateway.py`, `tests/unit_tests/flux/common/test_account_projection.py`, `tests/unit_tests/flux/common/test_portfolio_snapshot.py`

**Verification Commands:**
- `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`
- `./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -k 'projection or stale or ibkr'`
- `./.venv/bin/pytest -q tests/unit_tests/adapters/interactive_brokers/test_gateway.py`
- `./.venv/bin/pytest -q tests/unit_tests/flux/common/test_account_projection.py tests/unit_tests/flux/common/test_portfolio_snapshot.py`

**Step 1: Fix the IBKR refresh path itself**

Start by proving and fixing the actual recovery path, not just the stale-state wrapper. The implementation must make the shared-account refresh capable of becoming fresh again after a timeout or disconnected cached client.

Concretely:
- identify whether the failure is a stuck cached client, a bad timeout budget, client-id/session contention, or provider-owned event-loop reuse
- prefer the smallest provider-local recovery path first
- on repeated timeout/disconnect, force a clean reconnect path instead of reusing a dead client forever

Acceptable fixes include:
- teaching the provider to detect a dead client and rebuild it before the next refresh
- tightening the timeout/retry loop so a single bad request does not stall refresh for minutes
- only if provider-local recovery cannot restore freshness, invalidating and recreating the cached IBKR client on timeout/disconnect behind new tests in `tests/unit_tests/adapters/interactive_brokers/test_gateway.py`

Do not stop at “mark scope stale.” The provider must be able to recover to fresh state during normal operation.

**Step 2: Implement explicit provider status in the IBKR snapshot provider**

Extend the provider payload to carry status metadata instead of only rows:

```python
{
    "source_scope": "shared_account",
    "rows": [...],
    "totals": {...},
    "projection_status": {
        "healthy": True,
        "last_success_ts_ms": 1774215797831,
        "last_attempt_ts_ms": 1774216519016,
        "last_error_type": None,
        "last_error_message": None,
        "stale_after_ms": 15000,
    },
}
```

On refresh failure:
- do not overwrite `last_success_ts_ms`
- record the exception type and useful message
- make timeout logs include `type(exc).__name__` so `TimeoutError()` does not render blank

**Step 3: Make stale IBKR rows age out conservatively**

When the provider only has an old last-good snapshot and refreshes keep failing:
- keep the last-known rows available in the provider snapshot for diagnostics
- mark the scope unhealthy/stale once `last_success_ts_ms` exceeds the stale budget
- ensure the portfolio publisher can present those rows as explicitly stale diagnostic rows with `include_in_reconciliation=false`

Do not “refresh” row timestamps on failure. The original row `ts_ms` must remain the last true success time.

**Step 4: Publish per-scope status from the portfolio runner**

Update `build_profile_account_snapshot(...)` and `_publish_profile_account_projections(...)` so each projection key includes:
- `scope_status`
- `server_ts_ms`
- last success timestamp
- last failure metadata

Then ensure the portfolio snapshot copies this per-scope metadata into `accounts.scope_status`.

**Step 5: Re-run the IBKR/provider slice**

Run:

```bash
./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py
./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -k 'projection or stale or ibkr'
./.venv/bin/pytest -q tests/unit_tests/adapters/interactive_brokers/test_gateway.py
./.venv/bin/pytest -q tests/unit_tests/flux/common/test_account_projection.py tests/unit_tests/flux/common/test_portfolio_snapshot.py
```

Expected: PASS with explicit stale/healthy semantics, stable last-success timestamps, and a proven refresh-recovery path after timeout/disconnect.

**Step 6: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv4/reference_balances.py \
  systems/flux/flux/common/account_projection.py \
  systems/flux/flux/runners/shared/portfolio_runner.py \
  nautilus_trader/adapters/interactive_brokers/factories.py \
  tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py \
  tests/unit_tests/adapters/interactive_brokers/test_gateway.py \
  tests/unit_tests/flux/common/test_account_projection.py \
  tests/unit_tests/flux/common/test_portfolio_snapshot.py
git commit -m "fix(equities): harden ibkr projection freshness"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Fix Binance PM shared-account projection parsing and publishing

**Files:**
- Modify: `systems/flux/flux/runners/shared/profile_accounts.py`
- Modify: `nautilus_trader/adapters/binance/futures/http/account.py`
- Modify: `nautilus_trader/adapters/binance/futures/schemas/account.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`
- Test: `tests/integration_tests/adapters/binance/test_execution_futures.py`
- Test: `tests/integration_tests/adapters/binance/test_providers.py`

**Dependencies:** `Task 1: Lock the failing reconciliation contract in tests`

**Write Scope:** `systems/flux/flux/runners/shared/profile_accounts.py`, `nautilus_trader/adapters/binance/futures/http/account.py`, `nautilus_trader/adapters/binance/futures/schemas/account.py`, `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`, `tests/integration_tests/adapters/binance/test_execution_futures.py`, `tests/integration_tests/adapters/binance/test_providers.py`

**Verification Commands:**
- `./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -k 'binance and projection'`
- `./.venv/bin/pytest -q tests/integration_tests/adapters/binance/test_execution_futures.py -k 'portfolio_margin or position_risk'`
- `./.venv/bin/pytest -q tests/integration_tests/adapters/binance/test_providers.py -k 'portfolio_margin'`

**Step 1: Reproduce the live PM shared-account shape in tests**

Use fixture payloads or inline objects that omit fields the live PM path does not guarantee:
- `isolatedMargin`
- wallet-balance-only fields
- any PM-specific optional totals that can be absent while the response is still valid

**Step 2: Harden the adapter/provider boundary**

Make the shared-account path tolerant of PM payload drift with the smallest blast radius:
- prefer permissive schema handling for optional PM fields
- normalize missing optional fields in the projection layer only when the balances view actually needs them
- do not mutate adapter semantics for execution-only fields unless a dedicated adapter contract test proves it is required

Only normalize fields the projection logic actually needs:
- symbol / instrument
- signed position quantity
- max notional / leverage only if present
- cash balances and withdrawable totals

Do not require execution-only fields for balances reconciliation.

**Step 3: Publish Binance projection status the same way as IBKR**

Ensure the Binance projection snapshot carries:
- `projection_status.healthy`
- `projection_status.last_success_ts_ms`
- `projection_status.last_error_type`
- `projection_status.last_error_message`

This keeps the API/UI logic consistent across IBKR, Binance, and Hyperliquid shared-account scopes.

**Step 4: Re-run the Binance slice**

Run:

```bash
./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -k 'binance and projection'
./.venv/bin/pytest -q tests/integration_tests/adapters/binance/test_execution_futures.py -k 'portfolio_margin or position_risk'
./.venv/bin/pytest -q tests/integration_tests/adapters/binance/test_providers.py -k 'portfolio_margin'
```

Expected: PASS with PM position-risk refresh succeeding and shared-account projection rows publishing instead of erroring.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/runners/shared/profile_accounts.py \
  nautilus_trader/adapters/binance/futures/http/account.py \
  nautilus_trader/adapters/binance/futures/schemas/account.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py \
  tests/integration_tests/adapters/binance/test_execution_futures.py \
  tests/integration_tests/adapters/binance/test_providers.py
git commit -m "fix(equities): harden binance shared-account projection"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Make the equities balances API fail-safe and per-scope explicit

**Files:**
- Modify: `systems/flux/flux/api/app.py`
- Modify: `systems/flux/flux/common/account_projection.py`
- Modify: `systems/flux/flux/runners/equities/readiness.py`
- Modify: `ops/scripts/deploy/check_equities_live_readiness.sh`
- Test: `tests/unit_tests/flux/api/test_app.py`
- Test: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_readiness.py`
- Test: `tests/unit_tests/flux/common/test_account_projection.py`
- Test: `tests/unit_tests/flux/common/test_portfolio_snapshot.py`

**Dependencies:** `Task 2: Restore IBKR shared-account refresh reliability and freshness semantics`, `Task 3: Fix Binance PM shared-account projection parsing and publishing`

**Write Scope:** `systems/flux/flux/api/app.py`, `systems/flux/flux/common/account_projection.py`, `systems/flux/flux/runners/equities/readiness.py`, `ops/scripts/deploy/check_equities_live_readiness.sh`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`, `tests/unit_tests/examples/strategies/test_equities_readiness.py`, `tests/unit_tests/flux/common/test_account_projection.py`, `tests/unit_tests/flux/common/test_portfolio_snapshot.py`

**Verification Commands:**
- `./.venv/bin/pytest -q tests/unit_tests/flux/api/test_app.py -k 'balances_profile_equities'`
- `./.venv/bin/pytest -q tests/unit_tests/flux/api/test_equities_profile_contract.py -k 'balances'`
- `./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_readiness.py -k 'degraded or balances or projection'`
- `./.venv/bin/pytest -q tests/unit_tests/flux/common/test_account_projection.py tests/unit_tests/flux/common/test_portfolio_snapshot.py`

**Step 1: Decode and aggregate per-scope status in the API store**

Extend the profile-account snapshot decode/load path so `load_profile_account_projection_rows(...)` returns:
- `projection_rows`
- `projection_totals`
- `scope_status`

Each `scope_status` row should include:
- `account_scope_id`
- `provider`
- `required`
- `healthy`
- `stale`
- `last_success_ts_ms`
- `last_attempt_ts_ms`
- `last_error_type`
- `last_error_message`
- `row_count`

Derive `required` from the same profile account bindings used by the portfolio runner for the equities profile. Do not invent a second source of truth in the API layer.

**Step 2: Make `portfolio_snapshot_v2` conservative about stale shared-account rows**

When balances API serves the equities profile:
- keep using `portfolio_snapshot_v2` when the snapshot itself is fresh
- but do not silently bless stale shared-account rows as current reconciliation truth
- if a required shared-account scope is stale, set `degraded=true`, populate `stale_required`, and expose the exact `scope_status`

Stale shared-account rows should stay in the payload for operator visibility, but they must:
- carry `stale=true`
- stay associated with their `account_scope_id`
- carry `include_in_reconciliation=false`
- be excluded from fresh totals, risk-group aggregation, and any “balanced/unbalanced” logic that claims current reconciliation truth

**Step 3: Make the fallback path equally explicit**

The fallback path in `/api/v1/balances` must also expose:
- `source`
- `degraded`
- `scope_status`
- `missing_required`
- `stale_required`
- `stale_after_ms`

Do not let the fallback path hide projection degradation just because strategy snapshot rows arrived recently.

Add at least one contract test where:
- `binance.futures.main` is fresh
- `hyperliquid.xyz.main` is fresh
- `ibkr.reference.main` is stale

and prove the response still shows fresh Binance/Hyperliquid rows as usable while flagging only IBKR rows/scope as stale.

**Step 4: Keep the readiness gate aligned with the new degradation contract**

Update the equities readiness evaluator and its wrapper script so the live gate interprets balances degradation the same way as the API:
- a stale required shared-account scope still fails closed
- healthy non-stale scopes remain visible in diagnostics
- stale diagnostic rows do not count as fresh reconciliation truth

Do not let the balances API and readiness gate diverge on what `degraded`, `stale_required`, or `missing_required` mean.

**Step 5: Re-run the API and readiness slices**

Run:

```bash
./.venv/bin/pytest -q tests/unit_tests/flux/api/test_app.py -k 'balances_profile_equities'
./.venv/bin/pytest -q tests/unit_tests/flux/api/test_equities_profile_contract.py -k 'balances'
./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_readiness.py -k 'degraded or balances or projection'
./.venv/bin/pytest -q tests/unit_tests/flux/common/test_account_projection.py tests/unit_tests/flux/common/test_portfolio_snapshot.py
```

Expected: PASS with explicit per-scope degradation and no silent stale-row promotion.

**Step 6: Commit**

```bash
git add \
  systems/flux/flux/api/app.py \
  systems/flux/flux/common/account_projection.py \
  systems/flux/flux/runners/equities/readiness.py \
  ops/scripts/deploy/check_equities_live_readiness.sh \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/examples/strategies/test_equities_readiness.py \
  tests/unit_tests/flux/common/test_account_projection.py \
  tests/unit_tests/flux/common/test_portfolio_snapshot.py
git commit -m "fix(equities): expose balances scope freshness"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Surface balances freshness and degraded scopes in Fluxboard

**Files:**
- Modify: `fluxboard/api.ts`
- Modify: `fluxboard/types.ts`
- Modify: `fluxboard/stores.ts`
- Modify: `fluxboard/Balances.tsx`
- Modify: `fluxboard/components/panels/BalancesPanel.tsx`
- Modify: `fluxboard/Balances.test.tsx`
- Modify: `fluxboard/api.flux.test.ts`
- Test: `fluxboard/__tests__/store-freshness-contract.test.ts`

**Dependencies:** `Task 4: Make the equities balances API fail-safe and per-scope explicit`

**Write Scope:** `fluxboard/api.ts`, `fluxboard/types.ts`, `fluxboard/stores.ts`, `fluxboard/Balances.tsx`, `fluxboard/components/panels/BalancesPanel.tsx`, `fluxboard/Balances.test.tsx`, `fluxboard/api.flux.test.ts`, `fluxboard/__tests__/store-freshness-contract.test.ts`

**Verification Commands:**
- `cd fluxboard && npm test -- --runInBand store-freshness-contract`
- `cd fluxboard && npm test -- --runInBand Balances.test.tsx api.flux.test.ts`

**Step 1: Extend the balances payload type**

Add typed support for backend freshness/degradation fields:

```ts
type BalancesScopeStatus = {
  account_scope_id: string;
  healthy: boolean;
  stale: boolean;
  last_success_ts_ms?: number | null;
  last_attempt_ts_ms?: number | null;
  last_error_type?: string | null;
  last_error_message?: string | null;
  row_count?: number | null;
};
```

**Step 2: Fix the store freshness selector**

Make `selectBalancesFreshnessTs(...)` prefer actual backend freshness:
- snapshot `server_ts_ms`
- or the newest non-stale backend row timestamp
- not just `lastReceiveTs` / `lastUpdate`

This keeps the panel clock honest when the websocket is healthy but the underlying account data is stale.

**Step 3: Surface degraded scopes in the balances page**

Add a compact operator-facing degraded-state area on the page:
- show when the page is degraded
- list stale/missing scopes
- surface the last error type/message for IBKR/Binance scopes

Do not bury this in hidden debug JSON. It needs to be visible during live reconciliation.

**Step 4: Re-run the Fluxboard slice**

Run:

```bash
cd fluxboard && npm test -- --runInBand store-freshness-contract
cd fluxboard && npm test -- --runInBand Balances.test.tsx api.flux.test.ts
```

Expected: PASS with stale/degraded balances rendered explicitly and panel freshness tied to backend truth.

**Step 5: Commit**

```bash
git add \
  fluxboard/api.ts \
  fluxboard/types.ts \
  fluxboard/stores.ts \
  fluxboard/Balances.tsx \
  fluxboard/components/panels/BalancesPanel.tsx \
  fluxboard/Balances.test.tsx \
  fluxboard/api.flux.test.ts \
  fluxboard/__tests__/store-freshness-contract.test.ts
git commit -m "fix(fluxboard): surface degraded balances freshness"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 6: Verify live reconciliation behavior and rollout guards

**Files:**
- Modify: `docs/plans/2026-03-23-equities-balances-reconciliation.md`

**Dependencies:** `Task 2: Restore IBKR shared-account refresh reliability and freshness semantics`, `Task 3: Fix Binance PM shared-account projection parsing and publishing`, `Task 4: Make the equities balances API fail-safe and per-scope explicit`, `Task 5: Surface balances freshness and degraded scopes in Fluxboard`

**Write Scope:** `docs/plans/2026-03-23-equities-balances-reconciliation.md`

**Verification Commands:**
- `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py tests/unit_tests/examples/strategies/test_equities_run_portfolio.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_equities_profile_contract.py`
- `./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_readiness.py tests/unit_tests/flux/common/test_account_projection.py tests/unit_tests/flux/common/test_portfolio_snapshot.py`
- `./.venv/bin/pytest -q tests/integration_tests/adapters/binance/test_execution_futures.py tests/integration_tests/adapters/binance/test_providers.py`
- `cd fluxboard && npm test -- --runInBand store-freshness-contract Balances.test.tsx api.flux.test.ts`
- `pnpm --dir fluxboard build`
- `systemctl status flux@equities-portfolio.service flux@equities-api.service`
- `journalctl -u flux@equities-portfolio.service -n 200 --no-pager`
- `journalctl -u flux@equities-api.service -n 100 --no-pager`
- `curl -sS 'http://127.0.0.1:5022/api/v1/balances?profile=equities'`
- `curl -fsS 'http://127.0.0.1:5024/equities' | rg '/static/fluxboard/assets/'`

**Step 1: Run the full touched verification bundle**

Run the Python and Fluxboard suites listed above. Record exact pass/fail outputs in the tracker.

**Step 2: Redeploy the affected live services**

Run the exact deploy/build steps for the touched surfaces:

```bash
pnpm --dir fluxboard build
sudo systemctl restart flux@equities-portfolio.service flux@equities-api.service
```

If the `equities-api` command/env contract changed, rerun the service installer first:

```bash
sudo ops/scripts/deploy/install_equities_systemd.sh
sudo systemctl restart flux@equities-portfolio.service flux@equities-api.service
```

Do not restart the full equities universe unless the changed code path requires it.

**Step 3: Verify the live failure modes are gone**

Confirm in live logs and API output that:
- IBKR projection refresh is no longer logging blank `TimeoutError` failures
- Binance shared-account refresh is no longer logging `isolatedMargin` parse failures
- `/api/v1/balances?profile=equities` exposes per-scope `scope_status`
- stale scopes, if any, are clearly marked as degraded instead of silently merged
- `equities.fluxboard.balances` matches the API contract and clearly surfaces degraded state

**Step 4: Perform the operator reconciliation pass**

Check a live sample of known symbols:
- `TSM`
- `INTC`
- `CRCL`
- one stale/unbalanced name from the prior investigation if it still exists

For each, verify whether the balances page is:
- balanced and fresh
- degraded with explicit stale IBKR/Binance scope
- or truly mismatched with fresh data on both sides

Only the third case counts as a remaining reconciliation bug.

Before closing the task, do one explicit end-to-end comparison for at least one symbol that recently traded:
- raw IBKR projection payload from Redis
- raw Binance projection payload from Redis
- raw portfolio snapshot payload from Redis
- `/api/v1/balances?profile=equities`
- Fluxboard balances rendering

All five views must agree on freshness/degraded status and on whether the symbol participates in reconciliation totals.

**Step 5: Update the tracker and commit the verified closeout state**

```bash
git add docs/plans/2026-03-23-equities-balances-reconciliation.md
git commit -m "docs: record balances reconciliation verification"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
