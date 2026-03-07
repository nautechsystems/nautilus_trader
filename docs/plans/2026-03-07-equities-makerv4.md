# Equities MakerV4 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the current equities MakerV3 path with a production-ready MakerV4 strategy that quotes Hyperliquid trade[XYZ] perps off IBKR prices, hedges immediately on IBKR, publishes a dedicated MakerV4 signal surface, and fixes the existing Hyperliquid subaccount/account-state gap.

**Architecture:** Keep `/equities`, `profile=equities`, and `portfolio=equities` stable. Introduce `makerv4` as a first-class strategy spec and runtime param set, reuse the existing shared runners/API/balances/trades infrastructure where it is already family-agnostic, and build only the minimum new abstractions needed for a fee-aware immediate-hedge strategy. Treat Hyperliquid account resolution and fee resolution as foundational infrastructure, not rollout polish, because the current agent-wallet path is wrong for funded subaccounts.

**Tech Stack:** Python (Flux runners/API/strategy code), Nautilus Trader adapters (Hyperliquid, Interactive Brokers), Redis, TOML/systemd deploy config, React/TypeScript Fluxboard, pytest, Vitest, shell deploy scripts.

---

## Research Findings That Drive The Plan

1. The current equities Hyperliquid address is an API wallet / agent wallet, not the funded master account. Live `userRole` resolution maps `0x6B0cEc6d1d273331e1a04dC529E262740fC55d86` to master `0x6ed25f0c7497ccfb5ab429b0f195ba87052b5249`. Current balances are wrong because runtime account-state queries are still pointed at the agent wallet.
2. Hyperliquid fee discovery is usable from the official API. Live `userFees` on the resolved master currently returns account-specific rates, so MakerV4 should not hardcode HL maker/taker fees by default.
3. The current codebase already supports `vault_address` / `account_address` wiring at the Hyperliquid adapter layer in `nautilus_trader/adapters/hyperliquid/*`, but the equities deploy/runtime path does not yet resolve or surface the correct effective user for balances and fees.
4. Chainsaw `maker_v2` is the closest behavior reference, but it is not a literal drop-in. It is fee-aware and immediate-hedge-oriented, but it does not already implement the exact IOC-through-mid behavior requested here. MakerV4 will need a new hedge pricing path built from existing IOC primitives plus new crossing logic.
5. The current Flux profile/socket/balances/trades surfaces are mostly reusable. The hard-coded MakerV3 seams are the strategy registry, node/API runners, runtime params defaults/schema, and parts of the signal payload builder.
6. IBKR hedge fee discovery does not have a clean lightweight runtime path in this repo today. The adapter exposes post-fill commissions and `whatIf` order previews, but there is no existing per-quote fee service suitable for a hot quoting loop. MakerV4 should therefore use:
   - live Hyperliquid fees from API
   - configurable assumed IBKR hedge fee bps with explicit docs and tests
   - optional later `whatIf`/audit tooling as a non-blocking follow-up

## Scope

**In scope**

1. Correct Hyperliquid API-wallet / subaccount handling for balances, positions, and fee resolution.
2. Add `makerv4` as a first-class equities strategy family with dedicated runtime params and signal payloads.
3. Implement default-on immediate hedging with an operator toggle and IOC-through-mid pricing on the hedge leg.
4. Make MakerV4 quote pricing fee-aware using live Hyperliquid fees plus a configured assumed IBKR hedge-fee input.
5. Add a dedicated MakerV4 signal view under the existing `/equities` surface.
6. Show both Hyperliquid and IBKR balances/positions in the equities balances surface.
7. Keep the live equities profile stable and avoid speculative multi-version support beyond the migration work needed for MakerV4.
8. Abstract and reuse the concrete shared utilities that Makerv4 duplicates from Makerv3, but only after the Makerv4 publisher/signal contract is real.

**Out of scope**

1. Residual management / residual sweeper logic. If the immediate hedge fails or partially fills, fail closed, alert, and pause quoting.
2. A generic “all maker families forever” abstraction.
3. Parallel long-term support for `makerv3` and `makerv4` as equal production families inside one equities surface.
4. New portfolio/profile names or route changes outside the existing `/equities` surface.

## Design Decisions

1. **MakerV4 is a new family, not `makerv3` with flags.**
   - Use `strategy_family="maker_v4"`, `strategy_version="v4"`, and `param_set="makerv4"`.
   - Reuse runner/API/profile infrastructure, not MakerV3 state shape.
2. **Immediate hedge is the defining behavior and should default on.**
   - Add a runtime toggle, but default it to `true` for MakerV4.
   - If hedge readiness is false, strategy should become untradeable and keep `bot_off`.
3. **Do not fake IBKR fees.**
   - Hyperliquid fees come from API.
   - IBKR hedge fee is an explicit config/runtime assumption until a real pre-trade fee source exists.
4. **Do not hide venue identity.**
   - Signal must show Hyperliquid maker leg and IBKR hedge/reference leg as distinct venues.
   - Balances must show Hyperliquid wallet/collateral rows and IBKR cash/positions separately.
5. **After-hours is a first-class requirement.**
   - IBKR data should use `use_regular_trading_hours = false`.
   - Hedge route/exchange must be explicit and configurable so `SMART` and `BLUEOCEAN` are both supportable.
6. **Naming must be explicit rather than coincidental.**
   - Maintain one canonical mapping for strategy id, family, version, param set, and Fluxboard profile key.
7. **Shared Makerv3 utilities should be abstracted only after duplication is concrete.**
   - The first Makerv4 strategy/publisher implementation can be local.
   - Once the Makerv4 publisher and signal contract are stable, extract the shared pieces and switch both MakerV3 and MakerV4 to them.

## Acceptance Criteria

1. A `makerv4` strategy can be loaded through the equities runner without hard-coded MakerV3 metadata.
2. Hyperliquid balances and fee lookups resolve against the funded master/vault target when the configured signer is an agent wallet.
3. MakerV4 quote prices incorporate live Hyperliquid fees and configured assumed IBKR hedge fees.
4. On maker fill, MakerV4 immediately sends an IOC hedge order priced through mid according to the configured crossing rules.
5. If hedge placement fails or hedge readiness is false, quoting is paused and an alert is emitted; no residual logic is attempted.
6. `/api/v1/signals?profile=equities` publishes a MakerV4 row with two visible venue legs and effective spread fields.
7. Fluxboard `/equities` renders a dedicated MakerV4 signal table rather than forcing the existing MakerV2/V3 table semantics.
8. `/api/v1/balances?profile=equities` shows both Hyperliquid and IBKR balances/positions for the allowlisted strategy set.
9. Deploy/docs/tests are updated so the live contract is explicit about subaccount resolution, IBKR fee config, after-hours routing, and MakerV4 defaults.
10. There is one explicit naming-map test proving `makerv4` ↔ `maker_v4` ↔ `v4` ↔ Fluxboard `maker_v4` profile mapping.
11. Quantity, tick-size, and instrument mapping tests cover Hyperliquid-to-IBKR size translation, rounding, and zero-rounding edge cases.
12. Signal payloads include the required MakerV4 observability fields:
    - `effective_account_source`
    - `hedge_disabled_reason`
    - `ibkr_quote_age_ms`
    - `fee_snapshot_age_s`
    - `hedge_latency_ms`
    - `hedge_slippage_bps_vs_mid`
13. Shared-account balance semantics are operator-safe: shared IBKR cash is not double-counted across strategies.
14. After-hours rollout validation confirms:
    - outside-RTH order attributes are enabled as intended
    - the route is the expected one (`SMART` or `BLUEOCEAN`)
    - the required instrument permissions are present

## Execution Notes

1. Follow `@superpowers:test-driven-development` for each task.
2. Keep changes in this worktree/PR only.
3. Do not touch unrelated dirty files.
4. Use the user-facing name `assumed_hedge_fee_bps` everywhere for the IBKR fee assumption.
5. Extract shared utilities from Makerv3 only after the first concrete Makerv4 publisher/signal contract exists, then switch both families to the shared helpers.
6. Use `@superpowers:requesting-code-review` before final rollout signoff.

## Progress Tracker

| Task | Status | Owner | Notes |
| --- | --- | --- | --- |
| 1 | completed | controller | Hyperliquid effective-account resolution landed and verified. |
| 2 | completed | controller | `makerv4` registry/runtime identity landed and verified. |
| 3 | completed | controller | Instrument/rounding/pricing guardrail slice landed and verified. |
| 4 | completed | controller | Makerv4 runtime params and fee rules landed and verified. |
| 5 | completed | controller | Makerv4 immediate-hedge core landed and verified. |
| 6 | completed | controller | Makerv4 signal payload publisher landed and verified. |
| 7 | completed | controller | Shared Makerv3/Makerv4 publisher utilities extracted and verified. |
| 8 | completed | controller | Dedicated Fluxboard MakerV4 signal view implemented and locally approved; Fluxboard Task 8 verification passed. |
| 9 | completed | controller | Dual-venue equities balances/positions slice verified locally (`11 passed`). |
| 10 | completed | controller | Deploy/runtime contract is approved; route alignment and shared-contract consistency are enforced (`27 passed`). |
| 11 | in_progress | controller | Verification matrix is green and the Saturday canary now has the correct MakerV4 row shape plus a non-degraded balances endpoint, but live IBKR quote flow and dual-venue live balances are not yet fully proven on the paper weekend host. |

## Task 1: Fix Hyperliquid effective-account resolution for balances and fees

**Files:**
- Create: `systems/flux/flux/runners/live/hyperliquid_account.py`
- Modify: `systems/flux/flux/runners/live/venues.py`
- Modify: `nautilus_trader/adapters/hyperliquid/config.py`
- Modify: `nautilus_trader/adapters/hyperliquid/factories.py`
- Modify: `nautilus_trader/adapters/hyperliquid/execution.py`
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Modify: `deploy/equities/equities.live.toml`
- Modify: `deploy/equities/strategies/equities.strategy.template.toml`
- Modify: `deploy/equities/strategies/README.md`
- Test: `tests/unit_tests/examples/strategies/test_live_venue_registry.py`
- Test: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Test: `tests/integration_tests/adapters/hyperliquid/test_trade_xyz_adapter_contract.py`

**Step 1: Write the failing tests**

Add tests that prove:
1. explicit funded `account_address` keeps precedence when provided
2. explicit `vault_address` keeps precedence when provided
3. agent wallets resolve to the correct master for balance and fee queries
4. WS subscription identity is the same effective account target, not a separate guess
5. signer identity stays unchanged for execution

```python
def test_resolve_hyperliquid_effective_user_prefers_explicit_vault_address() -> None:
    resolved = resolve_hyperliquid_effective_user(
        signer_address="0xagent",
        vault_address="0xvault",
        info_client=DummyInfoClient(user_role_user="0xmaster"),
    )
    assert resolved.execution_signer == "0xagent"
    assert resolved.account_query_address == "0xvault"
    assert resolved.fee_query_address == "0xvault"
```

```python
def test_resolve_hyperliquid_effective_user_uses_user_role_master_for_agent_wallet() -> None:
    resolved = resolve_hyperliquid_effective_user(
        signer_address="0xagent",
        vault_address=None,
        info_client=DummyInfoClient(user_role_user="0xmaster"),
    )
    assert resolved.account_query_address == "0xmaster"
```

```python
def test_resolve_hyperliquid_effective_user_preserves_ws_subscription_address() -> None:
    resolved = resolve_hyperliquid_effective_user(
        signer_address="0xagent",
        account_address="0xsubaccount",
        vault_address=None,
        info_client=DummyInfoClient(user_role_user="0xmaster"),
    )
    assert resolved.account_query_address == "0xsubaccount"
    assert resolved.ws_subscription_address == "0xsubaccount"
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_live_venue_registry.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/integration_tests/adapters/hyperliquid/test_trade_xyz_adapter_contract.py
```

Expected: FAIL because there is no shared effective-user resolver at the adapter boundary and balances still assume the configured account address is the query target.

**Step 3: Write the minimal implementation**

Create a helper that returns both execution signer identity and effective account/fee query identity.

```python
@dataclass(frozen=True, slots=True)
class ResolvedHyperliquidUser:
    execution_signer: str
    account_query_address: str
    fee_query_address: str
    ws_subscription_address: str
    source: str
```

Wire this into:
- venue resolution for Hyperliquid config
- Hyperliquid adapter config/factory construction so signer/query/ws identity is authoritative before clients are built
- balance publishing
- future fee lookup for MakerV4

Document new env/config precedence:
1. `vault_address_env`
2. explicit `account_address_env` if it is already the funded user
3. `userRole`-resolved master for agent wallets
4. resolved effective address must be reused for both REST queries and WS subscriptions

**Step 4: Run tests to verify they pass**

Run the same pytest command, then smoke-check the helper on the live box:

```bash
python - <<'PY'
from flux.runners.live.hyperliquid_account import resolve_hyperliquid_effective_user
print(resolve_hyperliquid_effective_user)
PY

uv run --group test pytest -q tests/integration_tests/adapters/hyperliquid/test_trade_xyz_adapter_contract.py
```

Expected: PASS and helper import succeeds.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/runners/live/hyperliquid_account.py \
  systems/flux/flux/runners/live/venues.py \
  nautilus_trader/adapters/hyperliquid/config.py \
  nautilus_trader/adapters/hyperliquid/factories.py \
  nautilus_trader/adapters/hyperliquid/execution.py \
  systems/flux/flux/strategies/makerv3/publisher.py \
  deploy/equities/equities.live.toml \
  deploy/equities/strategies/equities.strategy.template.toml \
  deploy/equities/strategies/README.md \
  tests/unit_tests/examples/strategies/test_live_venue_registry.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/integration_tests/adapters/hyperliquid/test_trade_xyz_adapter_contract.py
git commit -m "fix: resolve hyperliquid funded account for equities"
```

## Task 2: Introduce `makerv4` as a first-class strategy spec and param set

**Files:**
- Create: `systems/flux/flux/strategies/makerv4/__init__.py`
- Create: `systems/flux/flux/strategies/makerv4/constants.py`
- Create: `systems/flux/flux/strategies/makerv4/strategy.py`
- Create: `systems/flux/flux/strategies/makerv4/runtime_params.py`
- Modify: `systems/flux/flux/strategies/registry.py`
- Modify: `systems/flux/flux/strategies/__init__.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `systems/flux/flux/runners/equities/run_api.py`
- Modify: `systems/flux/flux/api/app.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_identity_map.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_api.py`
- Test: `tests/unit_tests/flux/common/test_params.py`
- Test: `fluxboard/__tests__/config/paramsProfiles.test.ts`

**Step 1: Write the failing tests**

```python
def test_registry_exports_makerv4_spec() -> None:
    spec = get_strategy_spec("makerv4")
    assert spec.strategy_id == "makerv4"
    assert spec.strategy_family == "maker_v4"
    assert spec.param_set == "makerv4"
```

```python
def test_equities_run_api_uses_makerv4_metadata_when_strategy_spec_is_makerv4() -> None:
    metadata = build_strategy_metadata_for_test("makerv4")
    assert metadata.strategy_family == "maker_v4"
    assert metadata.strategy_version == "v4"
```

```python
def test_makerv4_identity_map_is_explicit() -> None:
    identity = get_strategy_identity("makerv4")
    assert identity.strategy_id == "makerv4"
    assert identity.strategy_family == "maker_v4"
    assert identity.param_set == "makerv4"
    assert identity.profile_key == "maker_v4"
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/makerv4/test_identity_map.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/flux/common/test_params.py

cd fluxboard && pnpm vitest run __tests__/config/paramsProfiles.test.ts
```

Expected: FAIL because `makerv4` does not exist in the registry and equities runners still hard-code `makerv3`.

**Step 3: Write the minimal implementation**

Add a new registry spec and drive runner/API metadata from the spec instead of hard-coded MakerV3 constants.

```python
MAKERV4_STRATEGY_SPEC = FluxStrategySpec(
    strategy_id="makerv4",
    strategy_family="maker_v4",
    strategy_version="v4",
    param_set="makerv4",
    strategy_cls=MakerV4Strategy,
    config_cls=MakerV4StrategyConfig,
)
```

Create a placeholder `MakerV4Strategy` / config stub so the registry and runner wiring are valid before the strategy core is implemented.

Also add one explicit identity map shared by backend and frontend metadata:
- registry strategy id = `makerv4`
- family = `maker_v4`
- version = `v4`
- param set = `makerv4`
- Fluxboard profile key = `maker_v4`

Keep one-param-set-per-process behavior for the equities API. Do not add a generalized mixed-version API surface.

**Step 4: Run tests to verify they pass**

Run the same pytest and Vitest commands.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv4/__init__.py \
  systems/flux/flux/strategies/makerv4/constants.py \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/strategies/makerv4/runtime_params.py \
  systems/flux/flux/strategies/registry.py \
  systems/flux/flux/strategies/__init__.py \
  systems/flux/flux/runners/equities/run_node.py \
  systems/flux/flux/runners/equities/run_api.py \
  systems/flux/flux/api/app.py \
  tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/makerv4/test_identity_map.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/flux/common/test_params.py \
  fluxboard/__tests__/config/paramsProfiles.test.ts
git commit -m "feat: add makerv4 strategy registry and api metadata"
```

## Task 3: Add quantity, tick-size, instrument-mapping, and pricing guardrails

**Files:**
- Create: `systems/flux/flux/strategies/makerv4/instruments.py`
- Create: `systems/flux/flux/strategies/makerv4/rounding.py`
- Create: `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`
- Modify: `systems/flux/flux/strategies/makerv4/pricing.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_pricing.py`

**Step 1: Write the failing tests**

Add pure tests for:
1. Hyperliquid perp quantity -> IBKR share quantity mapping
2. integer-share / min-lot constraints
3. Hyperliquid quote rounding on perp tick size
4. IBKR hedge-limit rounding on stock tick size
5. tiny fills that round to zero on the hedge side
6. locked/crossed IBKR market
7. one-sided IBKR quote
8. missing midpoint
9. very wide spread
10. stale quote
11. buy/sell hedge prices that would cross beyond the touch after rounding

```python
def test_translate_hyperliquid_fill_to_ibkr_shares_rounds_down_to_int() -> None:
    shares = translate_hyperliquid_fill_to_ibkr_shares(
        fill_qty=Decimal("1.87"),
        min_share_increment=Decimal("1"),
    )
    assert shares == Decimal("1")
```

```python
def test_build_hedge_limit_caps_buy_at_best_ask_after_rounding() -> None:
    limit_price = build_ibkr_ioc_limit(
        side="BUY",
        bid=Decimal("190.00"),
        ask=Decimal("190.04"),
        cross_mid_bps=Decimal("5"),
        tick_size=Decimal("0.01"),
    )
    assert limit_price <= Decimal("190.04")
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py \
  tests/unit_tests/flux/strategies/makerv4/test_pricing.py
```

Expected: FAIL because MakerV4 does not yet have dedicated mapping/rounding helpers.

**Step 3: Write the minimal implementation**

Add pure helpers for:
- venue/instrument mapping between Hyperliquid perp fills and IBKR stock hedges
- quantity rounding and minimum-share enforcement
- price rounding for both venues
- quote-validity checks before midpoint-based pricing

These helpers should be deterministic and side-effect free so the strategy core can rely on them without re-implementing math inline.

**Step 4: Run tests to verify they pass**

Run the same pytest command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv4/instruments.py \
  systems/flux/flux/strategies/makerv4/rounding.py \
  systems/flux/flux/strategies/makerv4/pricing.py \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py \
  tests/unit_tests/flux/strategies/makerv4/test_pricing.py
git commit -m "feat: add makerv4 mapping and rounding guards"
```

## Task 4: Add MakerV4 runtime params and fee-resolution rules

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/runtime_params.py`
- Create: `systems/flux/flux/strategies/makerv4/fees.py`
- Modify: `systems/flux/flux/api/app.py`
- Modify: `fluxboard/config/paramsProfiles.ts`
- Modify: `fluxboard/api.ts`
- Modify: `fluxboard/Params.tsx`
- Modify: `fluxboard/stores.ts`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py`
- Test: `tests/unit_tests/flux/params/test_manager.py`
- Test: `fluxboard/__tests__/config/paramsProfiles.test.ts`

**Step 1: Write the failing tests**

```python
def test_makerv4_defaults_enable_instant_hedge_and_live_hyperliquid_fees() -> None:
    params = MAKERV4_RUNTIME_PARAM_DEFAULTS
    assert params["instant_hedge_enabled"] is True
    assert params["maker_fee_source"] == "hyperliquid_api"
    assert params["hedge_fee_source"] == "config"
```

```python
def test_makerv4_params_profile_exposes_hedge_and_fee_controls() -> None:
    profile = getProfileSchema("maker_v4")
    assert "instant_hedge_enabled" in profile
    assert "assumed_hedge_fee_bps" in profile
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py \
  tests/unit_tests/flux/params/test_manager.py

cd fluxboard && pnpm vitest run __tests__/config/paramsProfiles.test.ts
```

Expected: FAIL because MakerV4 params/schema do not exist.

**Step 3: Write the minimal implementation**

Add a MakerV4 schema with:

```python
{
    "instant_hedge_enabled": True,
    "hedge_style": "ioc_through_mid",
    "hedge_ioc_cross_mid_bps": 2.0,
    "hedge_ioc_max_cross_bps": 10.0,
    "maker_fee_source": "hyperliquid_api",
    "hedge_fee_source": "config",
    "assumed_hedge_fee_bps": 1.0,
}
```

Also add the quoting defaults you actually want operators to use:
- inventory/risk defaults appropriate for equities
- `bot_on = false`
- `force_bot_off_on_start = true`

Make the Params profile first-class as `maker_v4`, with short headers/tooltips consistent with the operator-facing style already used for MakerV3 where keys overlap.

**Step 4: Run tests to verify they pass**

Run the same pytest and Vitest commands.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv4/runtime_params.py \
  systems/flux/flux/strategies/makerv4/fees.py \
  systems/flux/flux/api/app.py \
  fluxboard/config/paramsProfiles.ts \
  fluxboard/api.ts \
  fluxboard/Params.tsx \
  fluxboard/stores.ts \
  tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py \
  tests/unit_tests/flux/params/test_manager.py \
  fluxboard/__tests__/config/paramsProfiles.test.ts
git commit -m "feat: add makerv4 runtime params and fee controls"
```

## Task 5: Implement the MakerV4 strategy core with default-on immediate hedge

**Files:**
- Create: `systems/flux/flux/strategies/makerv4/strategy.py`
- Create: `systems/flux/flux/strategies/makerv4/pricing.py`
- Create: `systems/flux/flux/strategies/makerv4/market_data.py`
- Create: `systems/flux/flux/strategies/makerv4/wire.py`
- Create: `systems/flux/flux/strategies/makerv4/managed_orders.py`
- Modify: `systems/flux/flux/runners/live/venues.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_pricing.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_node.py`

**Step 1: Write the failing tests**

Add tests for:
1. fee-adjusted quote construction
2. immediate hedge enabled by default
3. IOC-through-mid hedge price selection
4. fail-closed behavior on hedge rejection or stale IBKR data
5. duplicate fill-event idempotency
6. partial maker fills
7. partial hedge fills
8. restart recovery with in-flight hedge state

```python
def test_makerv4_buy_hedge_prices_ioc_limit_through_mid() -> None:
    order = build_buy_hedge_order(
        ibkr_bid=Decimal("190.00"),
        ibkr_ask=Decimal("190.04"),
        cross_mid_bps=Decimal("2"),
    )
    assert order.time_in_force == "IOC"
    assert order.limit_price > Decimal("190.02")
    assert order.limit_price <= Decimal("190.04")
```

```python
def test_makerv4_pauses_quoting_when_hedge_fails() -> None:
    state = run_failed_hedge_cycle_for_test()
    assert state.tradeable is False
    assert state.bot_status == "bot_off"
```

```python
def test_makerv4_duplicate_fill_event_does_not_double_hedge() -> None:
    state = replay_duplicate_maker_fill_for_test()
    assert state.hedge_order_count == 1
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_pricing.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py
```

Expected: FAIL because MakerV4 strategy logic does not exist.

**Step 3: Write the minimal implementation**

Implement MakerV4 around these rules:
1. maker venue = Hyperliquid trade[XYZ] perp
2. hedge/reference venue = IBKR
3. quote price = IBKR reference price minus/plus required gross edge, expected HL fee, assumed IBKR hedge fee, and configured offsets
4. on fill, submit a hedge IOC limit order priced through mid using configurable cross-mid and max-cross caps
5. if hedge is rejected, timed out, or partially filled and not flat, emit critical alert and stop quoting
6. maintain idempotent hedge state so duplicate fill events and restart replay do not double-hedge
7. set the IBKR outside-RTH order attribute when the strategy/deploy config enables after-hours hedging
8. do not add residual management in this task

Use existing IOC/order helpers where possible. If IBKR needs a dedicated IOC builder, keep it local to Makerv4.

**Step 4: Run tests to verify they pass**

Run the same pytest command, then a focused paper-mode smoke:

```bash
uv run --group test pytest -q tests/unit_tests/flux/strategies/makerv4
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/strategies/makerv4/pricing.py \
  systems/flux/flux/strategies/makerv4/market_data.py \
  systems/flux/flux/strategies/makerv4/wire.py \
  systems/flux/flux/strategies/makerv4/managed_orders.py \
  systems/flux/flux/runners/live/venues.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_pricing.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py
git commit -m "feat: implement equities makerv4 immediate hedge core"
```

## Task 6: Publish MakerV4 signal/trade payloads and effective-spread data

**Files:**
- Create: `systems/flux/flux/strategies/makerv4/publisher.py`
- Modify: `systems/flux/flux/api/payloads.py`
- Modify: `systems/flux/flux/api/app.py`
- Modify: `systems/flux/flux/api/socketio.py`
- Test: `tests/unit_tests/flux/api/test_payloads.py`
- Test: `tests/unit_tests/flux/api/test_equities_profile_contract.py`

**Step 1: Write the failing tests**

```python
def test_equities_signal_payload_emits_makerv4_quote_snapshot() -> None:
    row = load_signal_row_for_test("aapl_tradexyz_makerv4")
    assert row["strategy_family"] == "maker_v4"
    assert row["maker_v4"]["quote_snapshot"]["maker_leg"]["venue"] == "HYPERLIQUID"
    assert row["maker_v4"]["quote_snapshot"]["hedge_leg"]["venue"] == "IBKR"
    assert "effective_spread_bps" in row["maker_v4"]["quote_snapshot"]
    assert "effective_account_source" in row["maker_v4"]["quote_snapshot"]
    assert "assumed_hedge_fee_bps" in row["maker_v4"]["quote_snapshot"]
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py
```

Expected: FAIL because signal payloads do not know about `maker_v4`.

**Step 3: Write the minimal implementation**

Publish a `maker_v4.quote_snapshot` payload with:
- `maker_leg`
- `hedge_leg`
- `ref_leg`
- `effective_spread_bps`
- `quoted_spread_bps`
- `expected_maker_fee_bps`
- `assumed_hedge_fee_bps`
- `hedge_ready`
- `hedge_route`
- `effective_account_source`
- `hedge_disabled_reason`
- `ibkr_quote_age_ms`
- `fee_snapshot_age_s`
- `hedge_latency_ms`
- `hedge_slippage_bps_vs_mid`

Do not overload the existing midpoint-vs-FV “spread” field. Add new explicit effective-spread fields instead.

Make trades/alerts preserve both leg identities so Fluxboard can show maker fill vs hedge fill provenance.

**Step 4: Run tests to verify they pass**

Run the same pytest command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv4/publisher.py \
  systems/flux/flux/api/payloads.py \
  systems/flux/flux/api/app.py \
  systems/flux/flux/api/socketio.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py
git commit -m "feat: publish maker v4 signals and effective spread"
```

## Task 7: Extract concrete shared MakerV3 utilities and switch Makerv4 to them

**Files:**
- Create: `systems/flux/flux/strategies/shared/quote_snapshot.py`
- Create: `systems/flux/flux/strategies/shared/publisher_common.py`
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Modify: `systems/flux/flux/strategies/makerv4/publisher.py`
- Test: `tests/unit_tests/flux/api/test_payloads.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py`

**Step 1: Write the failing tests**

```python
def test_makerv3_and_makerv4_use_shared_quote_snapshot_contract() -> None:
    v3 = build_makerv3_quote_snapshot_for_test()
    v4 = build_makerv4_quote_snapshot_for_test()
    assert v3["maker_leg"]["venue"]
    assert v4["maker_leg"]["venue"]
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py
```

Expected: FAIL because the shared helpers do not exist yet.

**Step 3: Write the minimal implementation**

Now that the Makerv4 publisher contract is concrete, extract only the duplicated shared utilities from Makerv3 and switch both families to them:
- quote snapshot normalization
- leg labeling / role map helpers
- venue/account observability row assembly
- balances row normalization that is strategy-family agnostic

Do **not** create a generic strategy base class.

**Step 4: Run tests to verify they pass**

Run the same pytest command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/shared/quote_snapshot.py \
  systems/flux/flux/strategies/shared/publisher_common.py \
  systems/flux/flux/strategies/makerv3/publisher.py \
  systems/flux/flux/strategies/makerv4/publisher.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py
git commit -m "refactor: share maker publisher utilities between v3 and v4"
```

## Task 8: Add a dedicated MakerV4 signal view in Fluxboard

**Files:**
- Create: `fluxboard/components/domain/signal/MakerV4SignalTable.tsx`
- Create: `fluxboard/tests/signal/MakerV4SignalTable.test.tsx`
- Modify: `fluxboard/components/domain/signal/SignalTable.tsx`
- Modify: `fluxboard/api.ts`
- Modify: `fluxboard/types.ts`
- Modify: `fluxboard/config/uiProfiles.ts`
- Modify: `fluxboard/App.tsx`
- Test: `fluxboard/tests/signal/MakerV4SignalTable.test.tsx`
- Test: `fluxboard/__tests__/panels/signal.test.tsx`

**Step 1: Write the failing tests**

```tsx
it('renders a dedicated maker v4 signal table with both venue legs and effective spread', async () => {
  render(<MakerV4SignalTable strategies={[buildMakerV4Strategy()]} />);
  expect(screen.getByText('Maker Market')).toBeInTheDocument();
  expect(screen.getByText('Hedge Market')).toBeInTheDocument();
  expect(screen.getByText('Effective Spread')).toBeInTheDocument();
  expect(screen.getByText(/Hyperliquid/i)).toBeInTheDocument();
  expect(screen.getByText(/IBKR/i)).toBeInTheDocument();
});
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cd fluxboard && pnpm vitest run \
  tests/signal/MakerV4SignalTable.test.tsx \
  __tests__/panels/signal.test.tsx
```

Expected: FAIL because there is no MakerV4 table or route selection yet.

**Step 3: Write the minimal implementation**

Do **not** keep branching the existing `SignalTable.tsx` monolith more deeply. Instead:
1. extract shared cells/helpers as needed
2. add `MakerV4SignalTable`
3. keep `/equities` stable and switch to the MakerV4 table when the family is `maker_v4` or when the strategy set is MakerV4-only

Columns should include:
- strategy id / status
- Hyperliquid maker market
- IBKR hedge/reference market
- effective spread
- hedge readiness / tradeability
- last update / staleness

**Step 4: Run tests to verify they pass**

Run the same Vitest command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  fluxboard/components/domain/signal/MakerV4SignalTable.tsx \
  fluxboard/tests/signal/MakerV4SignalTable.test.tsx \
  fluxboard/components/domain/signal/SignalTable.tsx \
  fluxboard/api.ts \
  fluxboard/types.ts \
  fluxboard/config/uiProfiles.ts \
  fluxboard/App.tsx \
  fluxboard/__tests__/panels/signal.test.tsx
git commit -m "feat: add makerv4 equities signal view"
```

## Task 9: Surface both Hyperliquid and IBKR balances/positions

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/publisher.py`
- Modify: `systems/flux/flux/api/payloads.py`
- Modify: `systems/flux/flux/api/app.py`
- Modify: `fluxboard/Balances.tsx`
- Test: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Test: `fluxboard/Balances.test.tsx`

**Step 1: Write the failing tests**

```python
def test_equities_balances_include_hyperliquid_and_ibkr_rows() -> None:
    rows = load_balances_rows_for_test("aapl_tradexyz_makerv4")
    venues = {row["venue"] for row in rows}
    assert venues == {"hyperliquid", "ibkr"}
```

```python
def test_shared_ibkr_cash_balance_is_not_duplicated_across_strategies() -> None:
    rows = load_balances_rows_for_test(["aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"])
    ibkr_cash_rows = [row for row in rows if row["venue"] == "ibkr" and row["asset"] == "USD"]
    assert len(ibkr_cash_rows) == 1
    assert ibkr_cash_rows[0]["scope"] == "shared_account"
```

```tsx
it('renders both hyperliquid collateral and ibkr cash/positions in balances', async () => {
  render(<Balances initialRows={buildMakerV4BalanceRows()} />);
  expect(screen.getByText(/Hyperliquid/i)).toBeInTheDocument();
  expect(screen.getByText(/IBKR/i)).toBeInTheDocument();
});
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/flux/api/test_equities_profile_contract.py

cd fluxboard && pnpm vitest run Balances.test.tsx
```

Expected: FAIL because equities balances currently only materialize the execution account rows.

**Step 3: Write the minimal implementation**

Make MakerV4 publish both venues:
1. Hyperliquid collateral / positions from the corrected effective account target
2. IBKR cash / positions from the IBKR account stream used by the hedge venue

Keep rows venue-separated. Do not net IBKR and Hyperliquid together into a fake single wallet.

For shared accounts, do not repeat the same IBKR cash balance once per strategy. Either dedupe by `(venue, account, asset)` or clearly label rows as shared account state. The preferred implementation is:
- dedupe shared cash rows by `(venue, account, asset)`
- keep position rows distinct where the position attribution is actually strategy-specific
- add a visible `scope="shared_account"` marker to the deduped cash rows

If Fluxboard already renders the returned rows cleanly, avoid gratuitous UI churn.

**Step 4: Run tests to verify they pass**

Run the same pytest and Vitest commands.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv4/publisher.py \
  systems/flux/flux/api/payloads.py \
  systems/flux/flux/api/app.py \
  fluxboard/Balances.tsx \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  fluxboard/Balances.test.tsx
git commit -m "feat: surface hyperliquid and ibkr balances for equities"
```

## Task 10: Update deploy/runtime config for MakerV4 and after-hours IBKR

**Files:**
- Modify: `deploy/equities/equities.live.toml`
- Modify: `deploy/equities/strategies/equities.strategy.template.toml`
- Create: `deploy/equities/strategies/aapl_tradexyz_makerv4.toml`
- Modify: `deploy/equities/systemd/common.env.example`
- Modify: `deploy/equities/README.md`
- Modify: `deploy/equities/strategies/README.md`
- Modify: `ops/scripts/deploy/install_equities_systemd.sh`
- Test: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Step 1: Write the failing tests**

```python
def test_equities_stack_contract_documents_makerv4_and_after_hours_ibkr() -> None:
    contract = load_equities_contract_for_test()
    assert contract["strategy_family"] == "maker_v4"
    assert contract["ibkr_use_regular_trading_hours"] is False
    assert contract["ibkr_route_exchange"] in {"SMART", "BLUEOCEAN"}
    assert contract["ibkr_outside_rth_orders"] is True
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/examples/strategies/test_equities_stack_contract.py
```

Expected: FAIL because the current deploy contract is still MakerV3-oriented and does not document after-hours routing or fee envs.

**Step 3: Write the minimal implementation**

Update the deploy contract to include:
- `makerv4` strategy files
- `vault_address_env` / effective Hyperliquid account guidance
- IBKR config with `use_regular_trading_hours = false`
- explicit `route_exchange` / `primary_exchange` notes for `SMART` vs `BLUEOCEAN`
- explicit outside-RTH hedge-order attribute/config, not just data-session config
- explicit `IBKR_ASSUMED_HEDGE_FEE_BPS` or equivalent env/config field

Do not leave the docs implying that IBKR fees are dynamically discovered if they are not.

Document the production validation checklist for after-hours:
- outside-RTH fills are actually available
- the route is the one you expect (`SMART` or `BLUEOCEAN`)
- the instrument permissions are present

**Step 4: Run tests to verify they pass**

Run the same pytest command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  deploy/equities/equities.live.toml \
  deploy/equities/strategies/equities.strategy.template.toml \
  deploy/equities/strategies/aapl_tradexyz_makerv4.toml \
  deploy/equities/systemd/common.env.example \
  deploy/equities/README.md \
  deploy/equities/strategies/README.md \
  ops/scripts/deploy/install_equities_systemd.sh \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py
git commit -m "docs: wire makerv4 equities deploy contract"
```

## Task 11: Live verification, review, and rollout record

**Files:**
- Create: `docs/reviews/2026-03-07-equities-makerv4-review.md`
- Modify: `docs/plans/2026-03-07-equities-makerv4.md`

**Step 1: Run the verification matrix**

Run:

```bash
uv run --group test pytest -q \
  tests/integration_tests/adapters/hyperliquid/test_trade_xyz_adapter_contract.py \
  tests/unit_tests/examples/strategies/test_live_venue_registry.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/makerv4/test_identity_map.py \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py \
  tests/unit_tests/flux/strategies/makerv4

cd fluxboard && pnpm vitest run \
  tests/signal/MakerV4SignalTable.test.tsx \
  Balances.test.tsx \
  __tests__/config/paramsProfiles.test.ts \
  __tests__/panels/signal.test.tsx
```

Expected: PASS.

**Step 2: Run live smoke checks**

Run:

```bash
curl -fsS 'http://127.0.0.1:5022/api/v1/signals?profile=equities' | jq .
curl -fsS 'http://127.0.0.1:5022/api/v1/balances?profile=equities' | jq .
journalctl -u flux@equities-node-aapl_tradexyz_makerv4.service -n 200 --no-pager
journalctl -u flux@equities-bridge.service -n 200 --no-pager
```

Check:
1. signal row shows `strategy_family=maker_v4`
2. maker leg is Hyperliquid, hedge/ref leg is IBKR
3. effective spread fields are present
4. balances include both venues
5. shared-account IBKR cash is not duplicated across strategies
6. no recurring bridge handler errors

**Step 2a: Run a one-symbol canary rollout before widening the allowlist**

Canary policy:
1. enable only `aapl_tradexyz_makerv4`
2. confirm signal, balances, and alerts behavior first
3. confirm outside-RTH route/permissions when the session permits
4. only then expand to more symbols

**Step 3: Request code review**

Use `@superpowers:requesting-code-review`, with explicit asks for:
- strategy-core correctness
- balances/trades payload correctness
- Fluxboard MakerV4 surface regressions
- deploy/runtime contract completeness

**Step 4: Update plan and review record**

Document:
- what passed
- any residual operational follow-ups
- whether IBKR fee remains config-driven
- whether after-hours `BLUEOCEAN` route was tested live or only configured
- the one-symbol canary result
- the shared-account balance smoke-check result

Include one rollback paragraph:
- how to disable MakerV4 cleanly
- whether the prior MakerV3 config remains available for emergency re-enable
- what `/equities` data/API expectations change during rollback

**Execution record (2026-03-07 UTC)**

- Verification matrix passed:
  - Python Task 11 slice: `162 passed in 3.73s`
  - Fluxboard Task 11 slice: `21 passed`
- Additional focused reruns passed:
  - `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`: `9 passed`
  - focused runner slice: `3 passed`
  - focused payload slice: `1 passed`
- Live canary status:
  - `flux@equities-node-aapl_tradexyz_makerv4.service` is `active (running)`
  - current `signals` row shows `strategy_family=maker_v4`, Hyperliquid maker identity, IBKR hedge/ref identity, and `balances_ok=true`
  - current `balances` endpoint is non-degraded and returns one Hyperliquid `USDC` row
  - current Makerv4 bridge tail shows `.balances`, `.state`, and `.market_bbo` discovery without a new Makerv4 handler traceback
- Remaining live follow-up:
  - Saturday paper canary did not yet prove stable live IBKR quote flow
  - current live balances proof is Hyperliquid-only; dual-venue live balances remain to be verified on a weekday/live-account path

**Step 5: Commit**

```bash
git add \
  docs/reviews/2026-03-07-equities-makerv4-review.md \
  docs/plans/2026-03-07-equities-makerv4.md
git commit -m "docs: record makerv4 rollout verification"
```

## Rollout Risks To Watch During Execution

1. **Hyperliquid account confusion:** if signer/master/vault precedence is wrong, balances and fees will silently drift again.
2. **IBKR market-session drift:** after-hours routing and `use_regular_trading_hours=false` must be explicit, or the strategy will look dead outside RTH.
3. **Immediate hedge semantics:** if IOC-through-mid crosses too aggressively, you will buy/sell edge away; if it is too conservative, fills will strand exposure.
4. **Signal contract drift:** do not overload existing MakerV2/V3 fields in ways that make Fluxboard ambiguous.
5. **Fake fee confidence:** do not present configured IBKR fee assumptions as if they were exchange-sourced live fees.

## External References Used For Research

1. Hyperliquid API wallets / nonce model: `https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/nonces-and-api-wallets`
2. Hyperliquid API info endpoints (used conceptually for `userRole` and `userFees`): `https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint`
