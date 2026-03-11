# Equities HL vs IBKR Go-To-Prod Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Deliver a production-safe equities stack where Hyperliquid is the execution venue, IBKR is the reference and future hedge venue, and `/equities` shows correct shared balances and positions even when those holdings pre-exist any single strategy process.

**Architecture:** Do not solve this by bolting more balance logic onto MakerV3. Introduce a profile-scoped account projection layer and a multi-asset portfolio snapshot that become the canonical source for `profile=equities` balances. Keep strategy-local inventory publication for risk and signal state, but move shared account state, portfolio aggregation, and GUI/API balance provenance out of strategy-family-specific code so MakerV3 and MakerV4 can both sit on the same production control plane.

**Tech Stack:** Python (Flux runners/API/strategy code), Redis, Nautilus Trader adapters (Hyperliquid, IBKR), TOML deploy config, systemd/Pulse deploy scripts, React/TypeScript Fluxboard, pytest, Vitest, shell deploy tooling.

## Research Findings Driving This Plan

1. `profile=equities` balances do not currently use the portfolio snapshot path. `api_balances()` only loads `store.load_portfolio_snapshot(...)` under `profile_normalized == "tokenmm"`, then falls back to merging per-strategy balance snapshots for equities. Files: `systems/flux/flux/api/app.py`, `systems/flux/flux/api/_payloads_balances.py`.
2. The current portfolio snapshot model is not valid for multi-stock equities. `StrategySetPortfolioAggregator.recompute_once()` loops `base_currency` values but writes each snapshot to the same `FluxRedisKeys.portfolio_snapshot(portfolio_id=...)` key, so the last asset overwrites the earlier ones. Files: `systems/flux/flux/runners/shared/portfolio_runner.py`, `systems/flux/flux/common/keys.py`, `systems/flux/flux/common/portfolio_snapshot.py`.
3. Portfolio component keys are derived from the wrong identity for equities. MakerV3 publishes components under the maker-leg base currency (for example `XYZ:AAPL`), while the shared portfolio runner derives base assets from contract symbols like `AAPL`. Those keys do not meet in the middle. Files: `systems/flux/flux/strategies/makerv3/inventory.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/runners/shared/portfolio_runner.py`.
4. IBKR reference balances are currently strategy-owned and MakerV4-only. `run_node.py` only attaches `configure_reference_balance_snapshot_provider(...)` when the strategy exposes that hook; MakerV3 does not. Even worse, the provider is conceptually attached to a strategy even though the IBKR account is shared across the equities profile. Files: `systems/flux/flux/runners/equities/run_node.py`, `systems/flux/flux/strategies/makerv4/reference_balances.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `systems/flux/flux/strategies/makerv3/publisher.py`.
5. The current balances merge rewrites row `strategy_id` to the profile id (`equities`) and only marks shared-account cash opportunistically. That loses provenance and cannot correctly model pre-existing IBKR holdings that are not strategy-owned. Files: `systems/flux/flux/api/_payloads_balances.py`, `fluxboard/docs/equities_contract.md`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`.
6. The current readiness model is too brittle for production balances. `profile=equities` degrades whenever required per-strategy balance snapshots are missing, even if the real shared IBKR account still has valid cash and positions that should remain visible. Files: `systems/flux/flux/api/app.py`, `deploy/equities/equities.live.toml`.

## Design Choice

### Recommended approach

Build a **profile-scoped account and portfolio control plane**, then hang both MakerV3 and MakerV4 off it:

1. Add a canonical `portfolio_asset_id` and account-role manifest for each equities strategy in shared deploy config.
2. Move IBKR and Hyperliquid account-state collection to the profile layer (`run_portfolio` / shared helpers), not the strategy.
3. Replace the current single-base `portfolio_snapshot` with a whole-profile multi-asset snapshot that becomes the canonical source for `profile=equities` balances.
4. Keep strategy-local inventory components for risk gating and quote logic, but key them by canonical asset identity rather than venue-specific base strings.
5. Treat MakerV4 as a consumer of the same control plane, not as the place where account/balance correctness lives.

This is the cleanest design because it fixes the actual architectural bug: shared account state is currently modeled as a side effect of a strategy process. That is wrong for balances, wrong for provenance, and wrong for operational resilience.

### Alternatives considered

1. **Patch MakerV3 only by porting the MakerV4 balance-provider hook.**
   - Pros: smallest code diff.
   - Cons: still strategy-owned, still duplicates IBKR account state across many nodes, still fails to show pre-existing holdings when strategies are down, and still leaves the multi-asset portfolio snapshot broken.
2. **Switch directly to MakerV4 for all equities now.**
   - Pros: faster route to HL/IBKR hedge behavior.
   - Cons: hedging improves, but shared balances, portfolio identity, and API/GUI provenance remain wrong. That is not a production-safe migration.
3. **Recommended: profile-scoped control plane first, then strategy-family rollout.**
   - Pros: correct abstraction boundary, supports current MakerV3 live stack, supports MakerV4 canaries later, removes tokenmm-only API assumptions, and gives the GUI one stable balance source.

## Redesigns Required Before Production Signoff

1. Replace the single-key portfolio snapshot contract with a whole-profile multi-asset snapshot.
2. Introduce canonical equities asset identity (`portfolio_asset_id`) rather than inferring identity from venue strings such as `XYZ:AAPL`.
3. Move shared venue-account snapshots out of strategy-family code and into profile-level orchestration.
4. Stop using `strategy_id` as the identity for shared-account balance rows; add explicit provenance fields.
5. Remove the tokenmm-only special case in `api_balances()` and make profile-scoped balances uniformly portfolio-snapshot-driven.
6. Add strategy-spec capabilities so MakerV3 and MakerV4 can coexist operationally behind `/equities` during canaries without implicit drift.

## Acceptance Criteria

1. `GET /api/v1/balances?profile=equities` is sourced from a profile portfolio snapshot, not from ad hoc per-strategy balance merges.
2. The equities portfolio snapshot contains `inventory_by_asset` for all enrolled stocks at once and is not overwritten asset-by-asset.
3. Existing IBKR cash and stock positions appear in `profile=equities` balances even if an individual strategy node is down.
4. Shared-account rows expose provenance with explicit fields such as `source_scope`, `account_scope_id`, and `source_strategy_ids`; they are not mislabeled as owned by a single strategy.
5. MakerV3 inventory components publish under canonical `portfolio_asset_id` values and aggregate cleanly with Hyperliquid and IBKR rows.
6. MakerV3 and MakerV4 both resolve strategy metadata and capabilities through one shared registry contract.
7. Fluxboard `/equities/balances` renders shared-account and strategy-local rows with freshness and provenance, without breaking the stable route surface.
8. Live operational readiness no longer depends on every strategy emitting an identical IBKR account snapshot.
9. The deploy contract clearly separates:
   - shared profile account scopes
   - per-strategy local inventory publishers
   - canary strategy-family selection
10. There is a canary path for MakerV4 hedge execution that reuses the same account projection and portfolio snapshot model.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | in_progress | main | Executing in subagent-driven mode from the isolated equities worktree. |
| Task 1: Define Canonical Equities Contract And Identity Model | completed | main | Completed after spec review and final quality review with no findings; verified by `46 passed`, targeted `ruff` checks, and clean `git diff --check`. |
| Task 2: Build Profile-Scoped Account Projection Infrastructure | not_started | unassigned | Plan created |
| Task 3: Replace Single-Base Portfolio Snapshot With Multi-Asset Snapshot V2 | not_started | unassigned | Plan created |
| Task 4: Move Equities Balances API To Portfolio Snapshot V2 | not_started | unassigned | Plan created |
| Task 5: Migrate MakerV3 Inventory And Balance Publishing To The Canonical Model | not_started | unassigned | Plan created |
| Task 6: Add Strategy Capabilities And Mixed-Family Equities Metadata | not_started | unassigned | Plan created |
| Task 7: Update Fluxboard And Operator Contracts For Shared Equities Balances | not_started | unassigned | Plan created |
| Task 8: Harden Deploy/Runtime Orchestration And Readiness Gates | not_started | unassigned | Plan created |
| Task 9: Canary MakerV4 HL-vs-IBKR Trading On The New Control Plane | not_started | unassigned | Plan created |

---

### Task 1: Define Canonical Equities Contract And Identity Model

**Files:**
- Create: `docs/architecture/equities_hl_ibkr_prod_model.md`
- Create: `systems/flux/flux/common/strategy_contracts.py`
- Modify: `deploy/equities/equities.live.toml`
- Modify: `deploy/equities/README.md`
- Modify: `fluxboard/docs/equities_contract.md`
- Test: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`
- Test: `tests/unit_tests/flux/api/test_equities_profile_contract.py`

**Step 1: Write the failing contract tests**

Add tests that pin the new shared manifest and provenance contract:

```python
def test_equities_live_config_declares_strategy_contracts_with_portfolio_asset_ids() -> None:
    config = _load_toml(_repo_root() / "deploy/equities/equities.live.toml")
    contracts = config["strategy_contracts"]
    aapl = next(item for item in contracts if item["strategy_id"] == "aapl_tradexyz_makerv3")
    assert aapl["portfolio_asset_id"] == "AAPL"
    assert aapl["execution_account_scope_id"] == "hyperliquid.xyz.main"
    assert aapl["reference_account_scope_id"] == "ibkr.reference.main"
```

```python
def test_equities_contract_docs_define_shared_account_row_provenance() -> None:
    contract = _read(_repo_root() / "fluxboard/docs/equities_contract.md")
    assert "source_scope" in contract
    assert "account_scope_id" in contract
    assert "source_strategy_ids" in contract
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py
```

Expected: FAIL because `strategy_contracts` and the new provenance fields do not exist yet.

**Step 3: Write the minimal implementation**

Create one shared manifest type instead of inferring identity from strategy ids and venue strings:

```python
@dataclass(frozen=True, slots=True)
class StrategyContractEntry:
    strategy_id: str
    portfolio_asset_id: str
    maker_instrument_id: str
    reference_instrument_id: str
    execution_account_scope_id: str
    reference_account_scope_id: str
    hedge_account_scope_id: str | None = None
```

Use `[[strategy_contracts]]` in `deploy/equities/equities.live.toml` as the single source of truth for:
- canonical asset identity
- venue leg mapping
- shared account scopes
- future MakerV4 canary family metadata

Document that `strategy_id` is strategy-local, while shared-account rows are portfolio-scoped and must use provenance fields rather than pretending to belong to one strategy.

**Step 4: Run tests to verify they pass**

Run the same pytest command plus:

```bash
rg -n "strategy_contracts|portfolio_asset_id|source_scope|account_scope_id|source_strategy_ids" \
  deploy/equities/equities.live.toml \
  deploy/equities/README.md \
  fluxboard/docs/equities_contract.md
```

Expected: PASS and the docs/config now describe one explicit production contract.

**Step 5: Commit**

```bash
git add \
  docs/architecture/equities_hl_ibkr_prod_model.md \
  systems/flux/flux/common/strategy_contracts.py \
  deploy/equities/equities.live.toml \
  deploy/equities/README.md \
  fluxboard/docs/equities_contract.md \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py
git commit -m "design: define canonical equities hl-ibkr contract"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Build Profile-Scoped Account Projection Infrastructure

**Files:**
- Create: `systems/flux/flux/common/account_projection.py`
- Create: `systems/flux/flux/runners/shared/profile_accounts.py`
- Modify: `systems/flux/flux/strategies/makerv4/reference_balances.py`
- Modify: `systems/flux/flux/runners/shared/portfolio_runner.py`
- Modify: `systems/flux/flux/runners/equities/run_portfolio.py`
- Modify: `systems/flux/flux/common/keys.py`
- Test: `tests/unit_tests/flux/common/test_account_projection.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`

**Step 1: Write the failing tests**

Cover the profile-owned account projection behavior:

```python
def test_profile_account_projection_publishes_ibkr_positions_without_strategy_snapshots() -> None:
    provider = FakeAccountProjectionProvider(
        rows=[{"exchange": "ibkr", "account": "U1234567", "asset": "AAPL", "kind": "position", "signed_qty": "25"}],
    )
    snapshot = build_profile_account_snapshot(profile_id="equities", providers=[provider], ts_ms=1_700_000_000_000)
    assert snapshot["rows"][0]["exchange"] == "ibkr"
    assert snapshot["rows"][0]["source_scope"] == "shared_account"
```

```python
def test_equities_portfolio_runner_collects_shared_account_snapshots_once_per_scope() -> None:
    aggregator = build_equities_aggregator_with_account_scopes(["ibkr.reference.main"])
    assert aggregator.account_scope_ids == ["ibkr.reference.main"]
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/common/test_account_projection.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py
```

Expected: FAIL because no profile-owned account projection layer exists.

**Step 3: Write the minimal implementation**

Create a family-agnostic provider contract:

```python
class AccountProjectionProvider(Protocol):
    def snapshot(self) -> dict[str, Any] | None: ...
```

Refactor the current IBKR reference balance provider into this shape and move ownership to the profile runner, not the strategy. `run_portfolio` should instantiate account providers by `account_scope_id` from `strategy_contracts`, deduplicate shared scopes, and refresh them once per profile process.

Do not fetch the same IBKR account summary from every node. The profile runner owns shared account polling; strategies own only local execution state.

**Step 4: Run tests to verify they pass**

Run the same pytest command plus a focused contract slice:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/common/test_account_projection.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -k "account"
```

Expected: PASS with one shared IBKR account projection per scope.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/common/account_projection.py \
  systems/flux/flux/runners/shared/profile_accounts.py \
  systems/flux/flux/strategies/makerv4/reference_balances.py \
  systems/flux/flux/runners/shared/portfolio_runner.py \
  systems/flux/flux/runners/equities/run_portfolio.py \
  systems/flux/flux/common/keys.py \
  tests/unit_tests/flux/common/test_account_projection.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py
git commit -m "feat: add profile-scoped equities account projections"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Replace Single-Base Portfolio Snapshot With Multi-Asset Snapshot V2

**Files:**
- Modify: `systems/flux/flux/common/keys.py`
- Modify: `systems/flux/flux/common/portfolio_snapshot.py`
- Modify: `systems/flux/flux/common/portfolio_inventory.py`
- Modify: `systems/flux/flux/runners/shared/portfolio_runner.py`
- Test: `tests/unit_tests/flux/common/test_keys.py`
- Test: `tests/unit_tests/flux/common/test_portfolio_snapshot.py`
- Test: `tests/unit_tests/flux/common/test_portfolio_inventory.py`

**Step 1: Write the failing tests**

Add tests that prove the snapshot can hold all 24 equities at once:

```python
def test_build_portfolio_snapshot_v2_keeps_inventory_for_multiple_assets() -> None:
    snapshot = build_portfolio_snapshot_v2(
        portfolio_id="equities",
        inventory_by_asset={
            "AAPL": {"global_qty_base": "10"},
            "MSFT": {"global_qty_base": "5"},
        },
        balance_rows=[],
        account_rows=[],
        now_ms_value=1_700_000_000_000,
    )
    assert snapshot["inventory_by_asset"]["AAPL"]["global_qty_base"] == "10"
    assert snapshot["inventory_by_asset"]["MSFT"]["global_qty_base"] == "5"
```

```python
def test_portfolio_snapshot_key_is_profile_scoped_not_last_asset_wins() -> None:
    key = FluxRedisKeys.portfolio_snapshot(portfolio_id="equities")
    assert key == "flux:v1:portfolio:snapshot:equities"
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/common/test_keys.py \
  tests/unit_tests/flux/common/test_portfolio_snapshot.py \
  tests/unit_tests/flux/common/test_portfolio_inventory.py
```

Expected: FAIL because the current snapshot builder only models one asset payload at a time.

**Step 3: Write the minimal implementation**

Introduce `portfolio_snapshot_v2` semantics:

```python
{
    "portfolio_id": "equities",
    "inventory_by_asset": {
        "AAPL": {...},
        "MSFT": {...},
    },
    "balances": {"rows": [...]},
    "accounts": {"rows": [...]},
    "server_ts_ms": 1700000000000,
}
```

Keep per-asset inventory keys for strategy risk (`portfolio_inventory(portfolio_id, base_currency)`), but make the profile snapshot the whole-book object that the API reads. The snapshot must carry all assets, not the last loop iteration.

**Step 4: Run tests to verify they pass**

Run the same pytest command and verify the old overwrite assumption is gone.

Expected: PASS with multi-asset inventory preserved in one snapshot.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/common/keys.py \
  systems/flux/flux/common/portfolio_snapshot.py \
  systems/flux/flux/common/portfolio_inventory.py \
  systems/flux/flux/runners/shared/portfolio_runner.py \
  tests/unit_tests/flux/common/test_keys.py \
  tests/unit_tests/flux/common/test_portfolio_snapshot.py \
  tests/unit_tests/flux/common/test_portfolio_inventory.py
git commit -m "feat: replace equities portfolio snapshot with multi-asset snapshot v2"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Move Equities Balances API To Portfolio Snapshot V2

**Files:**
- Modify: `systems/flux/flux/api/app.py`
- Modify: `systems/flux/flux/api/_payloads_balances.py`
- Modify: `systems/flux/flux/api/_payloads_common.py`
- Test: `tests/unit_tests/flux/api/test_app.py`
- Test: `tests/unit_tests/flux/api/test_equities_profile_contract.py`

**Step 1: Write the failing tests**

Cover the profile-level source and provenance behavior:

```python
def test_balances_profile_equities_prefers_portfolio_snapshot_v2() -> None:
    body = client.get("/api/v1/balances", query_string={"profile": "equities"}).get_json()
    assert body["data"]["source"] == "portfolio_snapshot_v2"
```

```python
def test_balances_profile_equities_preserves_shared_account_provenance_fields() -> None:
    row = body["data"]["rows"][0]
    assert row["source_scope"] == "shared_account"
    assert row["account_scope_id"] == "ibkr.reference.main"
    assert row["source_strategy_ids"] == ["aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"]
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py -k "balances"
```

Expected: FAIL because equities still uses per-strategy snapshot merge and rewrites row ownership.

**Step 3: Write the minimal implementation**

Remove the tokenmm-only special case. `api_balances()` should load the profile snapshot for any profile that has one, then degrade to per-strategy merge only as an explicit fallback.

Introduce explicit balance provenance fields:

```python
row["source_scope"] = "strategy" | "shared_account" | "portfolio"
row["account_scope_id"] = "ibkr.reference.main"
row["source_strategy_ids"] = ["aapl_tradexyz_makerv3"]
```

Do not overload `strategy_id` for shared account rows. Keep `strategy_id` only when the row is truly strategy-local.

**Step 4: Run tests to verify they pass**

Run the same pytest command plus:

```bash
uv run --group test pytest -q tests/unit_tests/flux/api/test_equities_profile_contract.py -k "portfolio_snapshot_v2 or shared_account"
```

Expected: PASS with `source = "portfolio_snapshot_v2"` and explicit row provenance.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/api/app.py \
  systems/flux/flux/api/_payloads_balances.py \
  systems/flux/flux/api/_payloads_common.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py
git commit -m "feat: source equities balances from portfolio snapshot v2"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Migrate MakerV3 Inventory And Balance Publishing To The Canonical Model

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/inventory.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_node.py`

**Step 1: Write the failing tests**

Pin canonical asset-id behavior and the removal of strategy-owned IBKR account injection:

```python
def test_publish_portfolio_inventory_component_uses_portfolio_asset_id_not_xyz_base() -> None:
    component = decode_component(redis_client.get(component_key))
    assert component.base_currency == "AAPL"
```

```python
def test_makerv3_balances_snapshot_contains_local_execution_rows_only() -> None:
    payload = published_balances_payload()
    assert all(row.get("exchange") != "ibkr" for row in payload["positions"])
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/strategies/makerv3/test_inventory.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py
```

Expected: FAIL because MakerV3 still uses maker-leg base identity and its balances payload shape is not aligned with the new profile model.

**Step 3: Write the minimal implementation**

Teach `run_node.py` to inject canonical `portfolio_asset_id` and contract metadata from the shared `strategy_contracts` manifest. MakerV3 should publish local inventory under that canonical asset id and stop pretending to own shared IBKR account rows.

Keep MakerV3 responsible for:
- local maker exposure
- signal state
- strategy-local balance rows

Move shared IBKR account state to the profile layer only.

**Step 4: Run tests to verify they pass**

Run the same pytest command and confirm the existing quote-cycle tests still pass.

Expected: PASS with canonical asset-keyed inventory and no strategy-owned shared-account duplication.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv3/inventory.py \
  systems/flux/flux/strategies/makerv3/strategy.py \
  systems/flux/flux/strategies/makerv3/publisher.py \
  systems/flux/flux/runners/equities/run_node.py \
  tests/unit_tests/flux/strategies/makerv3/test_inventory.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py
git commit -m "refactor: align makerv3 equities publishing with canonical portfolio model"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 6: Add Strategy Capabilities And Mixed-Family Equities Metadata

**Files:**
- Modify: `systems/flux/flux/strategies/registry.py`
- Create: `systems/flux/flux/strategies/shared/capabilities.py`
- Modify: `systems/flux/flux/runners/equities/run_api.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `systems/flux/flux/api/app.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_api.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Test: `tests/unit_tests/flux/api/test_equities_profile_contract.py`

**Step 1: Write the failing tests**

Cover mixed-family readiness without route churn:

```python
def test_equities_run_api_can_publish_per_strategy_family_metadata() -> None:
    metadata = build_equities_strategy_metadata_map(...)
    assert metadata["aapl_tradexyz_makerv3"].strategy_family == "maker_v3"
    assert metadata["aapl_tradexyz_makerv4"].strategy_family == "maker_v4"
```

```python
def test_strategy_spec_capabilities_expose_shared_account_projection_support() -> None:
    spec = get_strategy_spec("makerv4")
    assert spec.capabilities.uses_profile_account_projection is True
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py -k "strategy_family or metadata"
```

Expected: FAIL because equities API metadata is still effectively profile-global.

**Step 3: Write the minimal implementation**

Extend `FluxStrategySpec` with explicit capabilities:

```python
@dataclass(frozen=True, slots=True)
class FluxStrategyCapabilities:
    publishes_local_inventory: bool
    uses_profile_account_projection: bool
    supports_immediate_hedge: bool
```

Build per-strategy metadata from the shared manifest plus strategy spec, so MakerV3 and MakerV4 can coexist during canaries under the stable `equities` profile.

Do not build a generic plugin framework. Add only the capabilities needed for equities production rollout.

**Step 4: Run tests to verify they pass**

Run the same pytest command plus:

```bash
uv run --group test pytest -q tests/unit_tests/examples/strategies/test_equities_run_api.py -k "per_strategy_family"
```

Expected: PASS with explicit mixed-family metadata support.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/registry.py \
  systems/flux/flux/strategies/shared/capabilities.py \
  systems/flux/flux/runners/equities/run_api.py \
  systems/flux/flux/runners/equities/run_node.py \
  systems/flux/flux/api/app.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py
git commit -m "feat: add explicit equities strategy capabilities and metadata"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 7: Update Fluxboard And Operator Contracts For Shared Equities Balances

**Files:**
- Modify: `fluxboard/api.ts`
- Modify: `fluxboard/Balances.tsx`
- Modify: `fluxboard/components/panels/BalancesPanel.tsx`
- Modify: `fluxboard/components/balances/BalanceRow.tsx`
- Modify: `fluxboard/components/balances/BalanceGroup.tsx`
- Modify: `fluxboard/components/balances/RiskTable.tsx`
- Modify: `fluxboard/docs/equities_contract.md`
- Test: `fluxboard/Balances.test.tsx`
- Test: `fluxboard/components/panels/BalancesPanel.test.tsx`
- Test: `fluxboard/components/balances/RiskTable.test.tsx`

**Step 1: Write the failing tests**

Pin the profile-shared balance UX:

```tsx
it("renders shared IBKR account rows with provenance and freshness", async () => {
  render(<BalancesPanel profile="equities" />);
  expect(await screen.findByText("shared_account")).toBeInTheDocument();
  expect(screen.getByText("ibkr.reference.main")).toBeInTheDocument();
});
```

```tsx
it("does not require strategy_id on shared-account rows", async () => {
  expect(screen.getByText("U1234567")).toBeInTheDocument();
});
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cd fluxboard && npm test -- --run \
  Balances.test.tsx \
  components/panels/BalancesPanel.test.tsx \
  components/balances/RiskTable.test.tsx
```

Expected: FAIL because the UI still assumes the old row contract.

**Step 3: Write the minimal implementation**

Render balances by `source_scope` and provenance fields, not by guessed `strategy_id` ownership. Keep `/equities/balances` stable, but make the panel visually distinguish:
- strategy-local rows
- shared-account rows
- degraded/fallback rows

Surface freshness and provenance explicitly so operators can see whether the row came from IBKR shared account state or a strategy-local maker snapshot.

**Step 4: Run tests to verify they pass**

Run the same npm command plus a build smoke:

```bash
cd fluxboard && npm run build
```

Expected: PASS and no contract drift in the shared `/equities` UI.

**Step 5: Commit**

```bash
git add \
  fluxboard/api.ts \
  fluxboard/Balances.tsx \
  fluxboard/components/panels/BalancesPanel.tsx \
  fluxboard/components/balances/BalanceRow.tsx \
  fluxboard/components/balances/BalanceGroup.tsx \
  fluxboard/components/balances/RiskTable.tsx \
  fluxboard/docs/equities_contract.md \
  fluxboard/Balances.test.tsx \
  fluxboard/components/panels/BalancesPanel.test.tsx \
  fluxboard/components/balances/RiskTable.test.tsx
git commit -m "feat: update equities balances ui for shared-account portfolio model"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 8: Harden Deploy/Runtime Orchestration And Readiness Gates

**Files:**
- Modify: `deploy/equities/README.md`
- Modify: `deploy/equities/equities.live.toml`
- Modify: `ops/scripts/deploy/install_equities_systemd.sh`
- Modify: `ops/scripts/deploy/equities_stack.sh`
- Modify: `deploy/equities/systemd/common.env.example`
- Test: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`

**Step 1: Write the failing tests**

Pin the new readiness model:

```python
def test_equities_readiness_does_not_hide_shared_ibkr_balances_when_one_strategy_snapshot_is_missing() -> None:
    snapshot = build_portfolio_snapshot_v2(...)
    assert snapshot["readiness"]["shared_accounts_ready"] is True
    assert snapshot["readiness"]["strategy_snapshots_degraded"] is True
```

```python
def test_equities_installer_writes_profile_account_scope_env_once() -> None:
    env = render_equities_portfolio_env(...)
    assert "EQUITIES_ACCOUNT_SCOPES=ibkr.reference.main,hyperliquid.xyz.main" in env
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py
```

Expected: FAIL because the deploy/runtime contract does not yet model profile account scopes or split readiness states.

**Step 3: Write the minimal implementation**

Update deploy/runtime so the portfolio service is the owner of shared account scopes and readiness gates. Add explicit readiness dimensions:
- `shared_accounts_ready`
- `strategy_snapshots_ready`
- `portfolio_inventory_ready`

This prevents one missing strategy balance snapshot from blanking real IBKR account state in production.

**Step 4: Run tests to verify they pass**

Run the same pytest command and smoke-check generated envs:

```bash
sudo ops/scripts/deploy/install_equities_systemd.sh
sed -n '1,160p' /etc/flux/equities-portfolio.env
```

Expected: PASS locally and the rendered env shows shared account scope ownership in the portfolio service.

**Step 5: Commit**

```bash
git add \
  deploy/equities/README.md \
  deploy/equities/equities.live.toml \
  ops/scripts/deploy/install_equities_systemd.sh \
  ops/scripts/deploy/equities_stack.sh \
  deploy/equities/systemd/common.env.example \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py
git commit -m "ops: harden equities profile readiness and account-scope deploy wiring"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 9: Canary MakerV4 HL-vs-IBKR Trading On The New Control Plane

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `deploy/equities/strategies/aapl_tradexyz_makerv4.toml.disabled`
- Modify: `deploy/equities/equities.live.toml`
- Modify: `deploy/equities/README.md`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Test: `tests/unit_tests/flux/api/test_equities_profile_contract.py`

**Step 1: Write the failing tests**

Pin the new shared-model canary behavior:

```python
def test_makerv4_uses_profile_account_projection_for_reference_balances() -> None:
    strategy = MakerV4Strategy(...)
    assert strategy._supplemental_balance_snapshot() is None
    assert strategy.capabilities.uses_profile_account_projection is True
```

```python
def test_equities_profile_canary_allows_makerv3_and_makerv4_rows_together() -> None:
    body = client.get("/api/v1/signals", query_string={"profile": "equities"}).get_json()
    assert {row["strategy_family"] for row in body["data"]["strategies"]} == {"maker_v3", "maker_v4"}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py -k "makerv4 or mixed_family"
```

Expected: FAIL because MakerV4 still owns its supplemental IBKR balance path and the profile contract is not yet mixed-family-safe.

**Step 3: Write the minimal implementation**

Make MakerV4 consume the same profile-owned account and portfolio surfaces as MakerV3. Then enable one canary strategy (`AAPL`) under the new model, keeping route and profile identity stable.

The canary rollout must fail closed:
- if shared IBKR account projection is stale, hedge readiness is false
- if HL or IBKR venue mapping disagrees with `strategy_contracts`, strategy does not become tradeable
- if profile snapshot freshness is degraded, GUI remains visible and explicit about the degraded dimension

**Step 4: Run tests to verify they pass**

Run the same pytest command, then complete live canary validation:

```bash
curl -fsS http://127.0.0.1:5022/api/v1/signals?profile=equities | jq '.data.strategies[] | {id, strategy_family}'
curl -fsS http://127.0.0.1:5022/api/v1/balances?profile=equities | jq '.data | {source, degraded, count}'
```

Expected: PASS, one MakerV4 canary visible alongside MakerV3 rows, and balances still sourced from the profile snapshot.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/runners/equities/run_node.py \
  deploy/equities/strategies/aapl_tradexyz_makerv4.toml.disabled \
  deploy/equities/equities.live.toml \
  deploy/equities/README.md \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py
git commit -m "feat: canary makerv4 on shared equities control plane"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.
