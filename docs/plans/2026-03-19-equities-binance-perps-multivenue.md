# Equities Binance Perps Multi-Venue Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Add Binance USD-M equity perps to the shared `equities` profile so multiple maker venues can trade the same stock while sharing one stock-netted portfolio and risk surface with IBKR as the canonical hedge / FV market.

**Architecture:** Reuse the existing shared portfolio pipeline used by TokenMM instead of adding an equities-only fork. Widen `[[strategy_contracts]]` from “one stock row” to “one strategy route row,” allow duplicate `portfolio_asset_id` values across different maker venues, remove Hyperliquid-specific runner assumptions, and make MakerV4 hedge sizing flow through canonical stock quantity rather than venue-specific fill assumptions. Shared account projections must also grow beyond `hyperliquid` and `ibkr` so Binance futures account rows can participate in the same profile-owned balances and position truth.

**Tech Stack:** Python, Redis, Nautilus Trader live venue adapters (Binance Futures, Hyperliquid, IBKR), TOML deploy config, pytest, shell deploy tooling, Flux API / readiness runners, shared portfolio snapshot pipeline.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | completed | codex | All six tasks completed; Task 6 reviews passed, full verification bundle green (`321 passed`), and Binance discovery helper output captured |
| Task 1: Lock The Multi-Venue Equities Route Contract | completed | codex | Route-level contract landed; declared Binance scope row added; `151 passed` on Task 1 verification slice |
| Task 2: Generalize Shared Account Scopes And Profile Account Projection For Binance Futures | completed | codex | Spec review passed, quality review found no actionable defects, and `157 passed` on verification slice |
| Task 3: Remove Hyperliquid-Specific Equities Runner Assumptions | completed | codex | Spec review passed, quality review found no actionable defects, and `176 passed` on verification slice |
| Task 4: Generalize MakerV4 Quantity Translation And Hedge Sizing | completed | codex | Spec review passed, quality review approved, Task 4 targeted slice green (`9 passed, 95 deselected`), and full touched MakerV4 suite green (`93 passed`) |
| Task 5: Add Binance Equity-Perp Discovery, Enrollment, And Deploy Surfaces | completed | codex | Spec review passed, quality review approved, Task 5 slice green (`4 passed, 33 deselected`), and broader contract/file suite green (`37 passed`) |
| Task 6: Prove Multi-Venue Portfolio, API, And Readiness Behavior | completed | codex | Spec review passed, quality review approved, targeted slice green (`11 passed, 112 deselected`), broader verification bundle green (`321 passed`), and discovery helper output captured via `python3` |

---

### Task 1: Lock The Multi-Venue Equities Route Contract

**Files:**
- Modify: `systems/flux/flux/common/strategy_contracts.py`
- Modify: `deploy/equities/equities.live.toml`
- Modify: `deploy/equities/README.md`
- Modify: `deploy/equities/strategies/README.md`
- Modify: `docs/architecture/equities_hl_ibkr_prod_model.md`
- Modify: `fluxboard/docs/equities_contract.md`
- Test: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`

**Step 1: Write the failing contract tests**

Add tests that pin the new route-level semantics:

```python
def test_equities_live_config_allows_multiple_routes_for_same_portfolio_asset() -> None:
    config = _load_toml(_repo_root() / "deploy/equities/equities.live.toml")
    rows = config["strategy_contracts"]
    pltr_rows = [row for row in rows if row["portfolio_asset_id"] == "PLTR"]
    assert {row["maker_venue"] for row in pltr_rows} == {"HYPERLIQUID", "BINANCE_PERP"}
    assert len({row["strategy_id"] for row in pltr_rows}) == len(pltr_rows)
```

```python
def test_strategy_ids_by_asset_groups_multiple_routes_under_one_stock_bucket() -> None:
    grouped = _strategy_ids_by_asset(
        {
            "strategy_contracts": [
                {
                    "strategy_id": "pltr_tradexyz_makerv4",
                    "portfolio_asset_id": "PLTR",
                    "maker_venue": "HYPERLIQUID",
                    "maker_instrument_id": "xyz:PLTR-USD-PERP.HYPERLIQUID",
                    "reference_instrument_id": "PLTR.NASDAQ",
                    "execution_account_scope_id": "hyperliquid.xyz.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                },
                {
                    "strategy_id": "pltr_binance_perp_makerv4",
                    "portfolio_asset_id": "PLTR",
                    "maker_venue": "BINANCE_PERP",
                    "maker_instrument_id": "PLTRUSDT-PERP.BINANCE_PERP",
                    "reference_instrument_id": "PLTR.NASDAQ",
                    "execution_account_scope_id": "binance.futures.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                },
            ],
        },
        allowlist=["pltr_tradexyz_makerv4", "pltr_binance_perp_makerv4"],
    )
    assert grouped["PLTR"] == ("pltr_tradexyz_makerv4", "pltr_binance_perp_makerv4")
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py
```

Expected: FAIL because the checked-in equities contract still assumes unique `portfolio_asset_id` rows and does not define explicit maker-venue metadata per route.

**Step 3: Implement the route-level contract**

Extend `StrategyContractEntry` to represent one strategy route, not one stock:

```python
@dataclass(frozen=True, slots=True)
class StrategyContractEntry:
    strategy_id: str
    portfolio_asset_id: str
    maker_venue: str
    maker_symbol: str
    market_type: str
    maker_instrument_id: str
    reference_instrument_id: str
    execution_account_scope_id: str
    reference_account_scope_id: str
    hedge_account_scope_id: str | None = None
```

Update docs so:
- duplicate `portfolio_asset_id` across different maker routes is valid
- `strategy_id` stays unique
- portfolio / risk stays stock-netted by `portfolio_asset_id`
- local maker inventory stays strategy-local by route

**Step 4: Run the contract slice again**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py
```

Expected: PASS with duplicate-stock multi-route rows treated as valid.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/common/strategy_contracts.py \
  deploy/equities/equities.live.toml \
  deploy/equities/README.md \
  deploy/equities/strategies/README.md \
  docs/architecture/equities_hl_ibkr_prod_model.md \
  fluxboard/docs/equities_contract.md \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py
git commit -m "feat(equities): allow multi-venue routes per stock"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Generalize Shared Account Scopes And Profile Account Projection For Binance Futures

**Files:**
- Modify: `systems/flux/flux/common/account_scopes.py`
- Modify: `systems/flux/flux/runners/shared/profile_accounts.py`
- Modify: `deploy/equities/equities.live.toml`
- Modify: `deploy/equities/README.md`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Step 1: Write the failing shared-account tests**

Add tests that pin Binance futures as a supported profile-owned account scope:

```python
def test_build_profile_account_provider_bindings_supports_binance_futures_scope() -> None:
    bindings = build_profile_account_provider_bindings(
        config={
            "strategy_contracts": [
                {
                    "strategy_id": "pltr_binance_perp_makerv4",
                    "portfolio_asset_id": "PLTR",
                    "maker_venue": "BINANCE_PERP",
                    "maker_symbol": "PLTRUSDT",
                    "market_type": "perp",
                    "maker_instrument_id": "PLTRUSDT-PERP.BINANCE_PERP",
                    "reference_instrument_id": "PLTR.NASDAQ",
                    "execution_account_scope_id": "binance.futures.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                },
            ],
            "account_scopes": [
                {
                    "scope_id": "binance.futures.main",
                    "provider": "binance",
                    "venue": "BINANCE_PERP",
                    "api_key_env": "EQUITIES_BINANCE_API_KEY",
                    "api_secret_env": "EQUITIES_BINANCE_API_SECRET",
                    "account_type": "USDT_FUTURES",
                },
            ],
        },
    )
    assert bindings[0].account_scope_id == "binance.futures.main"
    assert bindings[0].provider is not None
```

```python
def test_equities_live_config_declares_binance_futures_account_scope_contract() -> None:
    config = _load_toml(_repo_root() / "deploy/equities/equities.live.toml")
    scopes = {row["scope_id"]: row for row in config["account_scopes"]}
    assert scopes["binance.futures.main"]["provider"] == "binance"
    assert scopes["binance.futures.main"]["venue"] == "BINANCE_PERP"
    assert scopes["binance.futures.main"]["api_key_env"] == "EQUITIES_BINANCE_API_KEY"
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  -k "binance or account_scope or profile_account"
```

Expected: FAIL because account scopes currently only expose `ibkr` and `hyperliquid` provider fields and the profile-account provider factory has no Binance path.

**Step 3: Add Binance futures shared-account provider support**

Extend `AccountScopeConfig` minimally with Binance fields:

```python
api_key_env: str | None = None
api_secret_env: str | None = None
account_type: str | None = None
base_url_http: str | None = None
recv_window_ms: int | None = None
```

Implement a `BinanceFuturesAccountProjectionProvider` in `profile_accounts.py` that:
- loads credentials from env vars
- uses `BinanceFuturesAccountHttpAPI`
- queries account info + position risk
- normalizes output into the existing shared account snapshot row format
- publishes `source_scope = "shared_account"` rows under the configured `account_scope_id`

Do not make strategies own Binance account state. Keep it profile-owned like the equities IBKR and Hyperliquid paths.

**Step 4: Re-run the shared-account slice**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  -k "binance or account_scope or profile_account"
```

Expected: PASS with Binance futures account scopes decoded and provider bindings built.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/common/account_scopes.py \
  systems/flux/flux/runners/shared/profile_accounts.py \
  deploy/equities/equities.live.toml \
  deploy/equities/README.md \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py
git commit -m "feat(equities): add binance futures shared account scopes"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Remove Hyperliquid-Specific Equities Runner Assumptions

**Files:**
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `systems/flux/flux/runners/equities/run_api.py`
- Modify: `systems/flux/flux/runners/equities/readiness.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_api.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_readiness.py`
- Test: `tests/unit_tests/examples/strategies/test_live_venue_registry.py`

**Step 1: Write the failing runner tests**

Add regressions that prove equities nodes can use `BINANCE_PERP` maker legs with explicit IBKR references:

```python
def test_build_node_uses_explicit_contract_reference_for_binance_perp_route(monkeypatch) -> None:
    # config row explicitly binds PLTRUSDT-PERP.BINANCE_PERP -> PLTR.NASDAQ
    # build_node must preserve that binding and not call the Hyperliquid mapper
    ...
```

```python
def test_equities_readiness_groups_same_stock_routes_without_treating_them_as_manifest_drift() -> None:
    # PLTR on HL and BINANCE_PERP both contribute to PLTR without contract failure
    ...
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_live_venue_registry.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_readiness.py \
  -k "binance or multivenue or duplicate"
```

Expected: FAIL because the equities runner still has Hyperliquid-specific reference derivation and equities-specific tests/docs still assume one route per stock.

**Step 3: Implement the runner generalization**

Change `run_node.py` so:
- explicit `reference_instrument_id` from `strategy_contracts` wins
- Hyperliquid-only derivation is a fallback for legacy rows only, not the canonical path
- `maker_venue` / `market_type` metadata from the route contract is threaded into strategy config / API metadata where needed

Keep using the generic live venue resolver for `BINANCE_PERP`; do not create a new equities-only Binance client stack.

**Step 4: Run the runner slice again**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_live_venue_registry.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_readiness.py \
  -k "binance or multivenue or duplicate"
```

Expected: PASS with explicit route metadata honored end-to-end.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/runners/equities/run_node.py \
  systems/flux/flux/runners/equities/run_api.py \
  systems/flux/flux/runners/equities/readiness.py \
  tests/unit_tests/examples/strategies/test_live_venue_registry.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_readiness.py
git commit -m "refactor(equities): honor explicit multivenue route contracts"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Generalize MakerV4 Quantity Translation And Hedge Sizing

**Files:**
- Modify: `systems/flux/flux/common/quantity_units.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/instruments.py`
- Modify: `systems/flux/flux/strategies/makerv4/publisher.py`
- Test: `tests/unit_tests/flux/common/test_quantity_units.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py`

**Step 1: Write the failing quantity / hedge tests**

Add tests that prove hedge sizing uses canonical stock quantity, not venue-specific assumptions:

```python
def test_makerv4_binance_equity_perp_fill_converts_to_canonical_stock_qty_before_ibkr_rounding() -> None:
    # maker fill qty from PLTRUSDT-PERP.BINANCE_PERP should map to PLTR stock qty first
    ...
```

```python
def test_makerv4_disables_hedging_when_maker_qty_conversion_is_unsupported() -> None:
    ...
```

```python
def test_publisher_contract_reports_actual_maker_venue_for_binance_perp_route() -> None:
    ...
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/common/test_quantity_units.py \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py \
  -k "binance or hedge_qty or conversion or maker_venue"
```

Expected: FAIL because the current hedge path still reasons in Hyperliquid-specific fill-to-share translation.

**Step 3: Replace the venue-specific hedge path**

Move the conversion contract to:

```python
maker_fill_venue_qty -> canonical stock base_qty -> rounded IBKR hedge shares
```

Implementation rules:
- use the generic quantity conversion helpers where possible
- fail closed when the maker venue instrument cannot produce canonical stock quantity safely
- preserve existing `hedge_min_share_increment` rounding only after canonical stock qty is known
- publish actual maker venue / route metadata in state and operator payloads

Do not special-case Binance in strategy logic beyond what the instrument metadata already says.

**Step 4: Run the targeted strategy slice**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/common/test_quantity_units.py \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py \
  -k "binance or hedge_qty or conversion or maker_venue"
```

Expected: PASS with fail-closed conversion behavior and venue-accurate observability.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/common/quantity_units.py \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/strategies/makerv4/instruments.py \
  systems/flux/flux/strategies/makerv4/publisher.py \
  tests/unit_tests/flux/common/test_quantity_units.py \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py
git commit -m "refactor(makerv4): use canonical stock qty for multivenue hedging"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Add Binance Equity-Perp Discovery, Enrollment, And Deploy Surfaces

**Files:**
- Create: `ops/scripts/deploy/binance_equities_universe.py`
- Create: `tests/unit_tests/ops/test_binance_equities_universe.py`
- Modify: `deploy/equities/strategies/equities.strategy.template.toml`
- Modify: `deploy/equities/equities_stack.env.example`
- Modify: `deploy/equities/systemd/common.env.example`
- Modify: `deploy/equities/README.md`
- Modify: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Step 1: Write the failing discovery / deploy tests**

Add tests that pin:

```python
def test_binance_equities_universe_filters_live_equity_tradfi_perps_only() -> None:
    symbols = load_binance_equity_perps(sample_exchange_info_payload())
    assert "MSTRUSDT" in symbols
    assert "XAUUSDT" not in symbols
    assert "EWJUSDT" not in active_symbols  # pending only
```

```python
def test_equities_template_supports_binance_perp_execution_route() -> None:
    template = _read(_repo_root() / "deploy/equities/strategies/equities.strategy.template.toml")
    assert 'execution_venue = "BINANCE_PERP"' in template or "BINANCE_PERP" in template
    assert "EQUITIES_BINANCE_API_KEY" in _read(_repo_root() / "deploy/equities/systemd/common.env.example")
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/ops/test_binance_equities_universe.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  -k "binance"
```

Expected: FAIL because no discovery helper exists and deploy docs/env examples do not yet define Binance equities credentials.

**Step 3: Implement explicit discovery and deploy surfaces**

Create a small ops helper that:
- fetches Binance USD-M `exchangeInfo`
- filters active equity perps (`TRADIFI_PERPETUAL`, `underlyingType = EQUITY`, active status)
- prints discovered symbols and diffs them against enrolled equities route rows

Update deploy surfaces so the required keys are explicit:
- `EQUITIES_BINANCE_API_KEY`
- `EQUITIES_BINANCE_API_SECRET`

Do not auto-enroll discovered names. Enrollment stays explicit through strategy rows and allowlists.

**Step 4: Run the discovery / deploy slice**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/ops/test_binance_equities_universe.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  -k "binance"
```

Expected: PASS with explicit discovery tooling and deploy contract docs.

**Step 5: Commit**

```bash
git add \
  ops/scripts/deploy/binance_equities_universe.py \
  tests/unit_tests/ops/test_binance_equities_universe.py \
  deploy/equities/strategies/equities.strategy.template.toml \
  deploy/equities/equities_stack.env.example \
  deploy/equities/systemd/common.env.example \
  deploy/equities/README.md \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py
git commit -m "feat(equities): add binance equity perp discovery and deploy contract"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 6: Prove Multi-Venue Portfolio, API, And Readiness Behavior

**Files:**
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_api.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_readiness.py`
- Modify: `tests/unit_tests/flux/common/test_portfolio_inventory.py`
- Modify: `tests/unit_tests/flux/common/test_portfolio_snapshot.py`
- Modify: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Create: `docs/runbooks/equities-binance-perp-market-making.md`

**Step 1: Write the failing end-to-end contract tests**

Add tests that prove:
- one stock bucket can contain multiple strategy contributors
- `/equities` balances / portfolio snapshot preserve per-route contributor visibility
- readiness reports node health per strategy while portfolio/risk remains stock-netted

Example test shape:

```python
def test_equities_portfolio_snapshot_nets_same_stock_across_hl_and_binance_routes() -> None:
    snapshot = build_portfolio_snapshot_v2(
        portfolio_id="equities",
        inventory_by_asset={
            "PLTR": {
                "base_currency": "PLTR",
                "global_qty_base": "15",
                "components": [
                    {"strategy_id": "pltr_tradexyz_makerv4", "local_qty_base": "10"},
                    {"strategy_id": "pltr_binance_perp_makerv4", "local_qty_base": "5"},
                ],
            },
        },
        ...
    )
    assert snapshot["inventory_by_asset"]["PLTR"]["global_qty_base"] == "15"
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/common/test_portfolio_inventory.py \
  tests/unit_tests/flux/common/test_portfolio_snapshot.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_readiness.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py
```

Expected: FAIL until the multi-route same-stock contract is reflected consistently in portfolio snapshots, readiness summaries, and API payload tests.

**Step 3: Implement the end-to-end contract updates**

Make the full equities control plane prove these behaviors:
- node / signal health remains per strategy route
- profile inventory and risk remain per canonical stock
- balances and API payloads keep enough per-route provenance for operators to see where stock exposure lives

Document the canary sequence in the runbook:
1. overlap-name canary (`PLTR` or `TSLA`) to prove same-stock multi-venue netting
2. Binance-only name canary (`MSTR`) to prove explicit enrollment of newly discovered Binance-only routes

**Step 4: Run the full targeted verification bundle**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_live_venue_registry.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_readiness.py \
  tests/unit_tests/flux/common/test_quantity_units.py \
  tests/unit_tests/flux/common/test_portfolio_inventory.py \
  tests/unit_tests/flux/common/test_portfolio_snapshot.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py \
  tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/ops/test_binance_equities_universe.py
```

Expected: PASS.

Then run the live discovery helper without enrolling any new route yet:

```bash
python ops/scripts/deploy/binance_equities_universe.py --show-diff --config deploy/equities/equities.live.toml
```

Expected: prints current Binance equity-perp universe, enrolled Binance route rows, and missing-but-not-enrolled names without changing live config.

**Step 5: Commit**

```bash
git add \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_readiness.py \
  tests/unit_tests/flux/common/test_portfolio_inventory.py \
  tests/unit_tests/flux/common/test_portfolio_snapshot.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  docs/runbooks/equities-binance-perp-market-making.md
git commit -m "test(equities): prove multivenue stock-netted portfolio contract"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Notes For Execution

- Execute this plan in a dedicated git worktree via `@superpowers:using-git-worktrees`.
- Use `@superpowers:test-driven-development` before each implementation task.
- Use `@superpowers:verification-before-completion` before claiming the integration is ready.
- Required live credentials for the new subaccount are:
  - `EQUITIES_BINANCE_API_KEY`
  - `EQUITIES_BINANCE_API_SECRET`
- The first real concurrent multi-venue stock canary should use an overlap name such as `PLTR` or `TSLA`, not `MSTR`. `MSTR` should be the second canary to validate Binance-only enrollment against the discovery tool.
