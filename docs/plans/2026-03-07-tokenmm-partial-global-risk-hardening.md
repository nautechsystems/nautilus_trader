# TokenMM Partial Global Risk Hardening Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Allow TokenMM strategies to consume a consistent partial shared `global_qty` during degraded periods, while hardening the portfolio/risk contract so it can cleanly converge with base-unit normalization work.

**Architecture:** Introduce an explicit portfolio aggregation mode and completeness metadata instead of overloading `global_qty = null` to mean every kind of failure. `run_portfolio` must become the single source of truth for shared TokenMM portfolio state: contributor health, shared inventory, merged balances rows, and portfolio totals. In the long term, this same portfolio pipeline upgrades from ambiguous `local_qty` / `global_qty` to normalized base-exposure fields from the base-unit normalization plan.

**Tech Stack:** Flux MakerV3 strategy and publisher, Flux shared portfolio runner, Flux API payload builders, Fluxboard Signal/Balances contracts, TokenMM deploy config, pytest, vitest.

---

## Decision Summary

Use a temporary `partial` aggregation mode for TokenMM portfolio inventory with these rules:

1. `global_qty` may be a partial sum of fresh contributors.
2. Completeness is reported explicitly via metadata, not inferred from `global_qty == null`.
3. Strategy quoting may use partial `global_qty` only when policy allows it.
4. Missing, stale, and unknown contributors remain visible in Redis, API, and UI.
5. `run_portfolio` owns the canonical shared portfolio snapshot consumed by strategy risk, Flux API, and Fluxboard.
6. The base-unit normalization rollout must replace the quantity source, not re-decide the partial-vs-strict policy.

### Task 1: Freeze Temporary Semantics And Config Surface

**Files:**
- Create: `docs/architecture/tokenmm-portfolio-inventory-semantics.md`
- Modify: `deploy/tokenmm/README.md`
- Modify: `deploy/tokenmm/tokenmm.live.toml`
- Modify: `fluxboard/docs/tokenmm_contract.md`
- Modify: `fluxboard/docs/tokenmm_socket_contract.md`

**Step 1: Write the failing doc/contract checklist**

Document these invariants:

- `global_qty` is a usable shared inventory estimate, not automatically a complete one.
- completeness is expressed by explicit metadata fields, not by nullability alone.
- `partial` and `strict` are the only supported aggregation modes.
- `partial` mode keeps missing/stale contributor diagnostics visible.
- the eventual normalized fields will be `local_qty_base` and `global_qty_base`.

Example contract snippet:

```json
{
  "global_qty": "129016.69578451",
  "aggregation_mode": "partial",
  "global_qty_complete": false,
  "usable_component_count": 4,
  "expected_component_count": 7,
  "missing_required": ["plumeusdt_binance_perp_makerv3"],
  "stale_required": ["plumeusdt_bitget_spot_makerv3"]
}
```

**Step 2: Add config knobs to the deploy contract**

Plan for these config fields in `deploy/tokenmm/tokenmm.live.toml`:

```toml
[portfolio]
portfolio_id = "tokenmm"
inventory_stale_after_ms = 3000
inventory_aggregation_mode = "partial"
allow_partial_global_risk = true
```

Keep `tokenmm_required_strategy_ids` for contributor expectations and diagnostics only until a later split is introduced.

**Step 3: Verify docs are internally consistent**

Run:
```bash
rg -n "aggregation_mode|global_qty_complete|allow_partial_global_risk|local_qty_base|global_qty_base" docs fluxboard/docs deploy/tokenmm
```

Expected: docs and deploy contract all describe the same temporary semantics.

**Step 4: Commit**

```bash
git add docs/architecture/tokenmm-portfolio-inventory-semantics.md deploy/tokenmm/README.md deploy/tokenmm/tokenmm.live.toml fluxboard/docs/tokenmm_contract.md fluxboard/docs/tokenmm_socket_contract.md
git commit -m "docs: define partial tokenmm global risk semantics"
```

### Task 2: Add Explicit Portfolio Aggregate Metadata And Partial-Sum Logic

**Files:**
- Modify: `systems/flux/flux/common/keys.py`
- Modify: `systems/flux/flux/common/portfolio_inventory.py`
- Create: `systems/flux/flux/common/portfolio_snapshot.py`
- Modify: `systems/flux/flux/runners/shared/portfolio_runner.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_portfolio.py`
- Test: `tests/unit_tests/flux/common/test_portfolio_snapshot.py`
- Test: `tests/unit_tests/flux/common/test_portfolio_inventory.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py`

**Step 1: Write failing tests for strict vs partial behavior and canonical shared snapshot output**

Add tests that prove:

```python
def test_build_portfolio_snapshot_partial_mode_keeps_sum_and_marks_incomplete():
    snapshot = build_portfolio_snapshot(
        portfolio_id="tokenmm",
        base_currency="PLUME",
        inventory_components={...},
        balance_rows_by_strategy={...},
        required_strategy_ids={"strategy_a", "strategy_b"},
        aggregation_mode="partial",
        now_ms_value=2_000,
    )
    assert snapshot["inventory"]["global_qty"] == "10"
    assert snapshot["inventory"]["aggregation_mode"] == "partial"
    assert snapshot["inventory"]["global_qty_complete"] is False
    assert snapshot["inventory"]["missing_required"] == ["strategy_b"]
    assert snapshot["balances"]["rows"][0]["strategy_id"] == "tokenmm"
```

```python
def test_build_portfolio_snapshot_strict_mode_nulls_global_qty_when_required_missing():
    ...
    assert snapshot["inventory"]["global_qty"] is None
    assert snapshot["inventory"]["global_qty_complete"] is False
```

**Step 2: Run the targeted tests to verify they fail**

Run:
```bash
pytest tests/unit_tests/flux/common/test_portfolio_snapshot.py -v
pytest tests/unit_tests/flux/common/test_portfolio_inventory.py -k "partial_mode or strict_mode" -v
pytest tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py -k "partial_mode or strict_mode" -v
```

Expected: FAIL because the canonical portfolio snapshot and partial-sum metadata do not exist yet.

**Step 3: Implement minimal canonical shared snapshot model**

Add Redis keys for a shared portfolio snapshot owned by `run_portfolio`, for example:

```python
FluxRedisKeys.portfolio_snapshot(...)
FluxRedisKeys.portfolio_snapshot_channel(...)
```

Create a snapshot builder that publishes one canonical payload containing:

- `inventory`
- `balances.rows`
- `balances.totals`
- `components`
- `server_ts_ms`
- `stale_after_ms`

The `inventory` section must extend the payload produced by `aggregate_components(...)` with:

- `aggregation_mode`
- `global_qty_complete`
- `usable_component_count`
- `expected_component_count`
- `missing_required`
- `stale_required`
- `null_qty_required`

Implementation rule:

```python
if aggregation_mode == "strict":
    global_qty = total if fresh_any and not missing_required and not stale_required and not null_qty_required else None
else:
    global_qty = total if fresh_any else None
```

**Step 4: Thread config and snapshot publication into the portfolio runner**

Read `portfolio.inventory_aggregation_mode` in `StrategySetPortfolioAggregator`, aggregate the inventory components plus merged balance rows once, and publish the canonical portfolio snapshot from the portfolio service.

**Step 5: Run tests to verify they pass**

Run:
```bash
pytest tests/unit_tests/flux/common/test_portfolio_snapshot.py -v
pytest tests/unit_tests/flux/common/test_portfolio_inventory.py -v
pytest tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py -v
```

Expected: PASS with coverage for both `strict` and `partial` modes and a single published portfolio snapshot contract.

**Step 6: Commit**

```bash
git add systems/flux/flux/common/keys.py systems/flux/flux/common/portfolio_inventory.py systems/flux/flux/common/portfolio_snapshot.py systems/flux/flux/runners/shared/portfolio_runner.py systems/flux/flux/runners/tokenmm/run_portfolio.py tests/unit_tests/flux/common/test_portfolio_snapshot.py tests/unit_tests/flux/common/test_portfolio_inventory.py tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py
git commit -m "feat: publish canonical tokenmm portfolio snapshot"
```

### Task 3: Make Strategy Risk Consume Partial Global Qty Deliberately

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/quote_engine.py`
- Modify: `systems/flux/flux/strategies/makerv3/inventory.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`

**Step 1: Write failing strategy tests**

Add tests that prove:

```python
def test_compute_inventory_skew_uses_partial_portfolio_global_qty_when_enabled():
    payload = {
        "global_qty": "129016.69578451",
        "aggregation_mode": "partial",
        "global_qty_complete": False,
        "missing_required": ["strategy_02"],
    }
    ...
    assert skew["global_inventory_qty"] == Decimal("129016.69578451")
    assert skew["global_inventory_source"] == "portfolio_component_partial_sum"
```

```python
def test_refresh_quotes_does_not_block_on_partial_portfolio_inventory_when_policy_enabled():
    ...
    assert cancels == []
    assert alerts == []
```

```python
def test_refresh_quotes_blocks_when_global_qty_missing_even_in_partial_mode():
    ...
    assert states == ["blocked_portfolio_inventory"]
```

**Step 2: Run the targeted tests to verify they fail**

Run:
```bash
pytest tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py -k "partial_portfolio" -v
pytest tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py -k "partial_portfolio" -v
pytest tests/unit_tests/flux/strategies/makerv3/test_inventory.py -k "partial_portfolio" -v
```

Expected: FAIL because the strategy currently blocks whenever `missing_required` is non-empty.

**Step 3: Implement policy-aware shared inventory reads**

Change `_shared_portfolio_inventory_qty_and_block_reason(...)` so it:

- still rejects stale aggregate payloads
- still rejects `global_qty is None`
- accepts `global_qty_complete = false` when `allow_partial_global_risk` is enabled
- returns a diagnostic source string such as `portfolio_component_partial_sum`

Do not silently ignore the incompleteness; cache it into pricing debug and state payloads.

**Step 4: Preserve observability in pricing debug**

Add these fields to strategy-side `pricing_debug.skew`:

- `global_inventory_qty_complete`
- `global_inventory_aggregation_mode`
- `global_inventory_missing_required`
- `global_inventory_stale_required`

**Step 5: Run tests to verify they pass**

Run:
```bash
pytest tests/unit_tests/flux/strategies/makerv3/test_inventory.py -v
pytest tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py -v
pytest tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py -v
```

Expected: PASS with clear separation between unusable inventory and partial-but-usable inventory.

**Step 6: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/strategy.py systems/flux/flux/strategies/makerv3/quote_engine.py systems/flux/flux/strategies/makerv3/inventory.py tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py
git commit -m "feat: allow partial shared portfolio risk for makerv3"
```

### Task 4: Make Flux API And Fluxboard Pure Consumers Of The Shared Portfolio Feed

**Files:**
- Modify: `systems/flux/flux/api/app.py`
- Modify: `systems/flux/flux/api/_payloads_signals.py`
- Modify: `systems/flux/flux/api/payloads.py`
- Modify: `fluxboard/types.ts`
- Modify: `fluxboard/components/domain/signal/SignalTable.tsx`
- Test: `tests/unit_tests/flux/api/test_app.py`
- Test: `tests/unit_tests/flux/api/test_payloads.py`
- Test: `fluxboard/tests/signal/MakerV2Overlay.test.tsx`

**Step 1: Write failing API and UI tests**

Add tests that prove:

```python
def test_balances_profile_tokenmm_uses_partial_shared_inventory_metadata():
    assert body["data"]["degraded"] is True
    assert body["data"]["global_qty"] == "129016.69578451"
    assert body["data"]["aggregation_mode"] == "partial"
    assert body["data"]["global_qty_complete"] is False
    assert body["data"]["source"] == "portfolio_snapshot"
```

```python
def test_build_signals_payload_preserves_partial_global_qty_metadata():
    assert payload["pricing_adjustments"][0]["global_qty"] == 129016.69578451
    assert payload["state"]["pricing_debug"]["skew"]["global_inventory_qty_complete"] is False
```

```tsx
it('renders global qty with partial-state annotation instead of blanking it', async () => {
  expect(screen.getByText('129,016.6958')).toBeInTheDocument();
  expect(screen.getByText(/partial/i)).toBeInTheDocument();
});
```

**Step 2: Run the targeted tests to verify they fail**

Run:
```bash
pytest tests/unit_tests/flux/api/test_app.py -k "partial_shared_inventory" -v
pytest tests/unit_tests/flux/api/test_payloads.py -k "partial_global_qty" -v
pnpm --dir fluxboard exec vitest run fluxboard/tests/signal/MakerV2Overlay.test.tsx
```

Expected: FAIL because balances and signals do not yet consume the shared portfolio snapshot consistently.

**Step 3: Remove API-side shared portfolio recomputation**

Required implementation:

- load the canonical shared portfolio snapshot from Redis for `profile=tokenmm`
- return `balances.rows`, `balances.totals`, `components`, and `inventory` metadata directly from that snapshot
- add `source = "portfolio_snapshot"` to make the ownership explicit
- stop deriving `missing_required`, `degraded`, and shared totals independently inside `app.py`

No fallback API-side recomputation path should remain for the shared TokenMM profile view.

**Step 4: Update Signal contract and Fluxboard rendering**

Expose and render:

- `aggregation_mode`
- `global_qty_complete`
- `missing_required`
- `stale_required`

UI rule:

- show `global_qty` when present
- annotate as `partial` when `global_qty_complete == false`
- never fall back to `risk_delta` when an explicit inventory adjustment exists

**Step 5: Run tests to verify they pass**

Run:
```bash
pytest tests/unit_tests/flux/api/test_app.py -v
pytest tests/unit_tests/flux/api/test_payloads.py -v
pnpm --dir fluxboard exec vitest run fluxboard/tests/signal/MakerV2Overlay.test.tsx
```

Expected: PASS with consistent semantics across shared risk and UI surfaces.

**Step 6: Commit**

```bash
git add systems/flux/flux/api/app.py systems/flux/flux/api/_payloads_signals.py systems/flux/flux/api/payloads.py fluxboard/types.ts fluxboard/components/domain/signal/SignalTable.tsx tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_payloads.py fluxboard/tests/signal/MakerV2Overlay.test.tsx
git commit -m "refactor: make tokenmm api consume portfolio snapshot"
```

### Task 5: Remove Remaining Producer-Level Observability Lies

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Test: `tests/unit_tests/flux/api/test_payloads.py`

**Step 1: Write failing tests for `bot_off` quote visibility**

Add tests that prove:

```python
def test_publish_state_preserves_live_quote_status_when_bot_off():
    strategy._publish_state("bot_off")
    assert state_payload["maker_quote_status"]["bid_open"] == 1
    assert state_payload["maker_quote_status"]["ask_open"] == 2
```

**Step 2: Run the targeted tests to verify they fail**

Run:
```bash
pytest tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -k "bot_off" -v
pytest tests/unit_tests/flux/api/test_payloads.py -k "maker_quote_status" -v
```

Expected: FAIL because the producer currently zeroes quote counts on `bot_off`.

**Step 3: Implement the minimal publisher fix**

Remove the special-case zero payload in `_maker_quote_status_payload(...)` so state snapshots reflect actual managed-order state regardless of `bot_on`.

**Step 4: Run tests to verify they pass**

Run:
```bash
pytest tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -v
pytest tests/unit_tests/flux/api/test_payloads.py -v
```

Expected: PASS with producer truth matching the UI contract.

**Step 5: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/publisher.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/api/test_payloads.py
git commit -m "fix: preserve quote status while bot is off"
```

### Task 6: Converge Partial-Sum Risk Onto Base-Unit Normalization

**Files:**
- Modify: `docs/plans/2026-03-07-base-unit-risk-and-balance-normalization.md` only if the owner requests an update
- Modify: `systems/flux/flux/common/portfolio_inventory.py`
- Modify: `systems/flux/flux/strategies/makerv3/inventory.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/api/_payloads_signals.py`
- Modify: `systems/flux/flux/api/app.py`
- Modify: `fluxboard/docs/tokenmm_contract.md`
- Test: `tests/unit_tests/flux/common/test_quantity_units.py`
- Test: `tests/unit_tests/flux/common/test_portfolio_inventory.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`
- Test: `tests/unit_tests/flux/api/test_payloads.py`
- Test: `tests/unit_tests/flux/api/test_app.py`

**Step 1: Write failing normalization-bridge tests**

Add tests that prove:

```python
def test_partial_global_qty_uses_local_qty_base_not_venue_qty():
    ...
    assert payload["global_qty_base"] == "1000"
    assert payload["global_qty"] == "1000"  # temporary alias
```

```python
def test_component_with_unsupported_qty_conversion_is_reported_not_silently_summed():
    ...
    assert payload["components"][0]["qty_conversion_status"] == "unsupported"
    assert payload["null_qty_required"] == ["strategy_02"]
```

**Step 2: Run the targeted tests to verify they fail**

Run:
```bash
pytest tests/unit_tests/flux/common/test_quantity_units.py -v
pytest tests/unit_tests/flux/common/test_portfolio_inventory.py -k "base" -v
pytest tests/unit_tests/flux/strategies/makerv3/test_inventory.py -k "base" -v
```

Expected: FAIL until the normalized quantity path is wired through.

**Step 3: Replace ambiguous quantity sources with normalized base exposure**

When the normalization work lands, move the partial-sum path to:

- component input: `local_qty_base`
- aggregate output: `global_qty_base`
- compatibility alias: `global_qty`
- explicit conversion diagnostics: `qty_conversion_status`, `qty_conversion_source`

Do not silently sum venue-native derivative contract counts.

**Step 4: Run the cross-surface verification suite**

Run:
```bash
pytest tests/unit_tests/flux/common/test_portfolio_inventory.py -v
pytest tests/unit_tests/flux/strategies/makerv3/test_inventory.py -v
pytest tests/unit_tests/flux/api/test_payloads.py -v
pytest tests/unit_tests/flux/api/test_app.py -v
```

Expected: PASS with partial-vs-complete semantics unchanged and only the unit source upgraded.

**Step 5: Commit**

```bash
git add systems/flux/flux/common/portfolio_inventory.py systems/flux/flux/strategies/makerv3/inventory.py systems/flux/flux/strategies/makerv3/strategy.py systems/flux/flux/api/_payloads_signals.py systems/flux/flux/api/app.py fluxboard/docs/tokenmm_contract.md tests/unit_tests/flux/common/test_quantity_units.py tests/unit_tests/flux/common/test_portfolio_inventory.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_app.py
git commit -m "refactor: move partial shared risk to base-exposure quantities"
```

### Task 7: Rollout, Verification, And Backout

**Files:**
- Modify: `deploy/tokenmm/README.md`
- Modify: `/etc/flux/common.env` or `/etc/flux/tokenmm-portfolio.env` only during live rollout

**Step 1: Add rollout checklist docs**

Document these operator checks:

1. portfolio Redis payload shows `aggregation_mode = "partial"`
2. missing/stale contributors are named explicitly
3. Signal shows the same `global_qty` for all live PLUME strategies
4. Signal marks partial state visibly
5. `balances?profile=tokenmm` and `signals?profile=tokenmm` report the same completeness metadata
6. strategies block only when `global_qty` is absent or aggregate feed is stale

**Step 2: Run pre-rollout verification**

Run:
```bash
pytest tests/unit_tests/flux/common/test_portfolio_inventory.py -v
pytest tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py -v
pytest tests/unit_tests/flux/api/test_app.py -v
pytest tests/unit_tests/flux/api/test_payloads.py -v
pnpm --dir fluxboard exec vitest run fluxboard/tests/signal/MakerV2Overlay.test.tsx
```

Expected: PASS.

**Step 3: Roll out to the TokenMM stack**

Run:
```bash
sudo systemctl restart flux@tokenmm-portfolio.service
sudo systemctl restart flux@tokenmm-api.service
sudo systemctl restart flux@tokenmm-node-plumeusdt_bybit_perp_makerv3.service
sudo systemctl restart flux@tokenmm-node-plumeusdt_bybit_spot_makerv3.service
sudo systemctl restart flux@tokenmm-node-plumeusdt_okx_perp_makerv3.service
sudo systemctl restart flux@tokenmm-node-plumeusdt_binance_perp_makerv3.service
sudo systemctl restart flux@tokenmm-node-plumeusdt_binance_spot_makerv3.service
sudo systemctl restart flux@tokenmm-node-plumeusdt_bitget_perp_makerv3.service
sudo systemctl restart flux@tokenmm-node-plumeusdt_bitget_spot_makerv3.service
```

**Step 4: Run live verification**

Run:
```bash
curl -fsS 'http://127.0.0.1:5022/api/v1/signals?profile=tokenmm'
curl -fsS 'http://127.0.0.1:5022/api/v1/balances?profile=tokenmm'
journalctl -u flux@tokenmm-portfolio.service -n 100 --no-pager
```

Expected:

- every PLUME strategy row shows the same `global_qty`
- `global_qty_complete` is `false` while contributors are missing
- missing/stale contributors remain named
- strategies remain runnable if `allow_partial_global_risk = true`

**Step 5: Backout**

If the partial mode causes unsafe behavior:

```bash
sudoedit deploy/tokenmm/tokenmm.live.toml
# set inventory_aggregation_mode = "strict"
# set allow_partial_global_risk = false
sudo systemctl restart flux@tokenmm-portfolio.service
sudo systemctl restart flux@tokenmm-api.service
sudo systemctl restart flux-tokenmm.target
```

**Step 6: Commit**

```bash
git add deploy/tokenmm/README.md
git commit -m "docs: add rollout plan for partial tokenmm global risk"
```
