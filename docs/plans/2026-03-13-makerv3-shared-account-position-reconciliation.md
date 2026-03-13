# Makerv3 Shared-Account Position Reconciliation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make Makerv3 strategy inventory and Signal output reconcile the strategy's matching shared-account maker position so live Hyperliquid quoting can be tested without hiding real inventory.

**Architecture:** Reuse the existing profile-owned `profile_account_projection` Redis contract instead of reading balances/API rows. Makerv3 gets one small generic runtime hook for its execution account scope and a projection feed, then reconciles only the exact `maker_instrument_id` into its existing local maker-position snapshot path when direct strategy-owned execution state is absent.

**Tech Stack:** Python, Redis, Flux strategy/runners, Nautilus Trader strategy config, pytest, Flux API/Signal payloads.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | completed | main | Tasks 1-4 completed. Live Makerv3 equities nodes were restarted from the worktree, and matching shared Hyperliquid maker positions now flow into Signal truth for retained names without polluting unrelated strategies. |
| Task 1: Wire Makerv3 Execution Scope Contract | completed | main | Spec review passed, quality review approved, and exact Task 1 tests passed locally: `4 passed in 0.28s`. |
| Task 2: Add Shared-Account Projection Reader | completed | main | Spec review passed, quality review approved, and commit `75f54fbf65` verified the focused pytest slice: `2 passed in 0.27s`. |
| Task 3: Reconcile Exact Maker Positions In Makerv3 | completed | main | Focused and broader Task 3 pytest slices are green after preserving projection-provided base/conversion metadata and adding precedence coverage for a fresh flat local snapshot over an older shared-account row. Local quality review found no remaining issues on the current diff. |
| Task 4: Verify Live Signal Truth And Update Trackers | completed | main | Restarted all 10 retained equities Makerv3 nodes with `sudo systemctl restart ...`. Live `/api/v1/signals?profile=equities` now shows `googl_tradexyz_makerv3 local_qty_base=-6`, `nvda_tradexyz_makerv3 local_qty_base=-9.111`, both with `local_inventory_source=shared_account_projection`, while `tsla_tradexyz_makerv3` stays flat at `0`. `/api/v1/balances?profile=equities` still serves the shared HL position rows for `NVDA`, `COIN`, and `GOOGL`, and no equities node units are failed. |

---

### Task 1: Wire Makerv3 Execution Scope Contract

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`

**Step 1: Write the failing tests**

Add a runner regression proving equities Makerv3 receives the execution account scope from the existing `strategy_contracts` table:

```python
def test_optional_strategy_config_kwargs_injects_execution_account_scope_id() -> None:
    kwargs = run_node._optional_strategy_config_kwargs(
        config={
            "strategy_contracts": [
                {
                    "strategy_id": "googl_tradexyz_makerv3",
                    "portfolio_asset_id": "GOOGL",
                    "maker_instrument_id": "XYZ:GOOGL-USD-PERP.HYPERLIQUID",
                    "reference_instrument_id": "GOOGL.NASDAQ",
                    "execution_account_scope_id": "hyperliquid.xyz.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                },
            ],
        },
        external_strategy_id="googl_tradexyz_makerv3",
        strategy_spec=_MAKERV3_SPEC,
        strategy_cfg={},
    )

    assert kwargs["execution_account_scope_id"] == "hyperliquid.xyz.main"
```

Add a Makerv3 config acceptance regression so the strategy factory can build with the new field:

```python
def test_strategy_factory_accepts_execution_account_scope_id(strategy_factory) -> None:
    strategy = strategy_factory(execution_account_scope_id="hyperliquid.xyz.main")
    assert strategy.config.execution_account_scope_id == "hyperliquid.xyz.main"
```

**Step 2: Run tests to verify they fail**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py \
  -k "execution_account_scope_id" -p no:rerunfailures
```

Expected: FAIL because `MakerV3StrategyConfig` and `_optional_strategy_config_kwargs(...)` do not yet carry the execution scope.

**Step 3: Write minimal implementation**

Add one optional field to `MakerV3StrategyConfig`:

```python
execution_account_scope_id: str | None = None
```

Extend `run_node._optional_strategy_config_kwargs(...)` so when the strategy contract matches `external_strategy_id`, it injects both:

```python
candidates["portfolio_asset_id"] = contract.portfolio_asset_id
candidates["execution_account_scope_id"] = contract.execution_account_scope_id
```

Keep this generic: no equities-only names in the strategy config itself.

Add an equities-runner attachment helper that passes the existing shared Redis/profile contract into strategies that expose a projection-feed hook:

```python
def _attach_profile_account_projection_feed(...):
    strategy.configure_profile_account_projection_feed(
        redis_client=redis_client,
        profile_id=EQUITIES_DESCRIPTOR.profile,
        account_scope_id=strategy.config.execution_account_scope_id,
        namespace=namespace,
        schema_version=schema_version,
    )
```

Call it from `build_node(...)` right after the existing portfolio-inventory feed wiring.

**Step 4: Run tests to verify they pass**

Run the same pytest command. Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv3/strategy.py \
  systems/flux/flux/runners/equities/run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py
git commit -m "feat(makerv3): carry shared execution scope into strategy config"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Add Shared-Account Projection Reader

**Files:**
- Create: `systems/flux/flux/strategies/shared/account_projection_positions.py`
- Test: `tests/unit_tests/flux/strategies/shared/test_account_projection_positions.py`

**Step 1: Write the failing tests**

Create a focused helper test module that proves the projection reader:
- loads a Redis `profile_account_projection` payload by `profile_id + account_scope_id`
- ignores non-position rows
- ignores rows from other instruments
- prefers the exact `instrument_id` match
- preserves the freshest matching row only

Example failing test:

```python
def test_read_matching_position_row_returns_exact_instrument_match() -> None:
    redis_client = _FakeRedis(
        {
            FluxRedisKeys.profile_account_projection(
                profile_id="equities",
                account_scope_id="hyperliquid.xyz.main",
            ): encode_profile_account_snapshot(
                {
                    "profile_id": "equities",
                    "account_scope_ids": ["hyperliquid.xyz.main"],
                    "rows": [
                        {
                            "source_scope": "shared_account",
                            "account_scope_id": "hyperliquid.xyz.main",
                            "kind": "position",
                            "instrument_id": "XYZ:GOOGL-USD-PERP.HYPERLIQUID",
                            "signed_qty_venue": "-6",
                            "mark_px": "306.5",
                            "ts_ms": 1700000000123,
                        },
                    ],
                }
            ),
        }
    )

    row = read_matching_shared_account_position_row(
        redis_client=redis_client,
        profile_id="equities",
        account_scope_id="hyperliquid.xyz.main",
        instrument_id="XYZ:GOOGL-USD-PERP.HYPERLIQUID",
        namespace="flux",
        schema_version="v1",
    )

    assert row is not None
    assert row["signed_qty_venue"] == "-6"
```

**Step 2: Run tests to verify they fail**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/strategies/shared/test_account_projection_positions.py \
  -p no:rerunfailures
```

Expected: FAIL because the helper does not exist yet.

**Step 3: Write minimal implementation**

Create a small shared helper module with functions along these lines:

```python
def read_matching_shared_account_position_row(
    *,
    redis_client: Any,
    profile_id: str,
    account_scope_id: str,
    instrument_id: str,
    namespace: str,
    schema_version: str,
) -> dict[str, Any] | None: ...
```

Implementation rules:
- read only the existing `FluxRedisKeys.profile_account_projection(...)` key
- decode via `decode_profile_account_snapshot(...)`
- inspect `rows`
- require `kind == "position"`
- require exact normalized `instrument_id` equality
- if multiple rows match, pick the freshest `ts_ms`

No balances/API coupling. No Hyperliquid-specific parsing in this helper.

**Step 4: Run tests to verify they pass**

Run the same pytest command. Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/shared/account_projection_positions.py \
  tests/unit_tests/flux/strategies/shared/test_account_projection_positions.py
git commit -m "feat(makerv3): add shared account projection position reader"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Reconcile Exact Maker Positions In Makerv3

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Step 1: Write the failing tests**

Add one lifecycle regression proving Makerv3 uses the shared-account maker position when direct strategy-owned position state is absent:

```python
def test_maker_local_position_summary_falls_back_to_shared_account_projection_for_exact_instrument(
    clocked_strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("XYZ:GOOGL-USD-PERP.HYPERLIQUID")
    strategy = clocked_strategy_factory(
        [2_000_000_000],
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=InstrumentId.from_str("GOOGL.NASDAQ"),
        portfolio_asset_id="GOOGL",
        execution_account_scope_id="hyperliquid.xyz.main",
    )
    strategy._maker_instrument = _identity_exposure_instrument(
        maker_instrument_id,
        base_currency="GOOGL",
    )
    strategy._instruments = {maker_instrument_id: strategy._maker_instrument}
    strategy._cache = SimpleNamespace(
        positions_open=lambda: [],
        accounts=lambda: [],
        account_for_venue=lambda venue: None,
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )
    fake_redis = _FakeRedis(...)
    strategy.configure_portfolio_inventory_feed(...)
    strategy.configure_profile_account_projection_feed(
        redis_client=fake_redis,
        profile_id="equities",
        account_scope_id="hyperliquid.xyz.main",
        namespace="flux",
        schema_version="v1",
    )

    summary = strategy._maker_local_position_summary("GOOGL")

    assert summary.venue_qty == Decimal("-6")
    assert summary.base_qty == Decimal("-6")
```

Add a second regression that inventory publication and state payloads surface the reconciled local qty:

```python
def test_publish_portfolio_inventory_component_uses_shared_account_projection_when_cache_is_flat(...) -> None:
    ...
    assert component.local_qty_base == Decimal("-6")
```

```python
def test_publish_state_emits_reconciled_local_inventory_source(...) -> None:
    ...
    assert state_payload["pricing_debug"]["skew"]["local_inventory_source"] == "shared_account_projection"
```

**Step 2: Run tests to verify they fail**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  -k "shared_account_projection or execution_account_scope_id" \
  -p no:rerunfailures
```

Expected: FAIL because Makerv3 has no projection feed and no fallback reconciliation path.

**Step 3: Write minimal implementation**

Add one generic Makerv3 feed configuration method:

```python
def configure_profile_account_projection_feed(
    self,
    *,
    redis_client: Any,
    profile_id: str,
    account_scope_id: str,
    namespace: str,
    schema_version: str,
) -> None: ...
```

Store those settings on the strategy, then add a fallback path in `_maker_local_position_summary(...)`:
1. fresh maker position snapshot
2. direct cache/open-position summary
3. exact-instrument shared-account projection row
4. flat/unavailable

Convert the shared row into the existing `_build_maker_position_report_snapshot(...)` shape so the rest of Makerv3 remains unchanged.

Set a distinct source string such as:

```python
"shared_account_projection"
```

for the local inventory source when this fallback is used.

**Step 4: Run tests to verify they pass**

Run the same pytest command plus the focused runner regression from Task 1:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/flux/strategies/shared/test_account_projection_positions.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  -k "execution_account_scope_id or shared_account_projection" \
  -p no:rerunfailures
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv3/strategy.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py
git commit -m "fix(makerv3): reconcile maker inventory from shared account projections"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Verify Live Signal Truth And Update Trackers

**Files:**
- Modify: `docs/plans/2026-03-12-equities-live-trading-readiness.md`
- Modify: `docs/plans/2026-03-13-equities-prod-hardening-universe-pruning.md`
- Modify: `docs/plans/2026-03-13-makerv3-shared-account-position-reconciliation.md`

**Step 1: Run focused strategy verification**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/flux/strategies/shared/test_account_projection_positions.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  -k "execution_account_scope_id or shared_account_projection" \
  -p no:rerunfailures
```

Expected: PASS.

**Step 2: Run broader no-regression verification**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  -p no:rerunfailures
git diff --check
```

Expected: PASS and clean diff formatting.

**Step 3: Verify live Signal truth on the box**

After rebuilding/restarting from the worktree if needed, run:

```bash
curl -fsS http://127.0.0.1:5024/api/v1/signals?profile=equities | \
  jq '.data.strategies[] | select(.id=="googl_tradexyz_makerv3" or .id=="nvda_tradexyz_makerv3" or .id=="coin_tradexyz_makerv3") | {id, local_qty_base, global_qty_base, position_qty_base, inventory_source, local_inventory_source}'
```

Expected:
- `googl_tradexyz_makerv3` reflects the shared `-6` maker position
- `nvda_tradexyz_makerv3` reflects the shared `-9.111` maker position
- `coin_tradexyz_makerv3` reflects the shared `-22.715` maker position if that strategy is in the active set; otherwise document it as out of retained scope

**Step 4: Update trackers**

Record:
- the exact tests run
- whether live Signal/local inventory now matches shared Hyperliquid maker positions
- whether this is sufficient to start the first live Makerv3 quoting test

in both tracker docs and in this plan’s Progress Tracker.

**Step 5: Commit**

```bash
git add \
  docs/plans/2026-03-12-equities-live-trading-readiness.md \
  docs/plans/2026-03-13-equities-prod-hardening-universe-pruning.md \
  docs/plans/2026-03-13-makerv3-shared-account-position-reconciliation.md
git commit -m "docs(equities): track makerv3 shared-account reconciliation rollout"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.
