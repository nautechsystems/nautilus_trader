# Equities Split Sync With PR 65 Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Sync the completed `equities_maker` / `equities_taker` split branch with PR 65 (`feat(equities): add Binance multivenue equity perps`) so the split architecture can run Hyperliquid maker/taker and Binance maker/taker for the enrolled equities universe without reintroducing legacy `maker_v4` control-plane behavior.

**Architecture:** Treat PR 65 as upstream multivenue functionality that must be translated into the split architecture, not as a cherry-pick target. Use this pass to remove the remaining “equities perp means Hyperliquid vs IBKR” assumptions from the shared equities stack by moving onto route-driven venue roles, shared account scopes, and venue-capability seams. Keep `/equities` on the split family contract, preserve shared asset-level risk across all enrolled venue/family combinations, and translate PR 65’s new Binance routes into split naming instead of restoring `*_makerv4` deploy identity.

**Tech Stack:** Python 3, Flux runners and API, Redis-backed params and readiness state, deploy TOML manifests, Fluxboard React/TypeScript surfaces, pytest, vitest, GitHub PR 65 head `c49951381f8ec6da3de59f0b081ccad35e949662`.

## Sync Principles

1. Keep the active equities families as `equities_maker` and `equities_taker`; do not restore `strategy_class = "maker_v4"` on the shared `/equities` surface.
2. Port PR 65’s widened `strategy_contracts` and `account_scopes` schema because the multivenue runner/account logic depends on them.
3. Preserve the split branch’s mixed-family params contract: `/equities/params` remains strategy-aware and profile-scoped writes stay ambiguous unless a strategy is explicit.
4. Preserve same-symbol shared-risk grouping across all enrolled venue/family combinations while keeping distinct execution positions separate by venue/account/instrument.
5. Translate PR 65’s Binance venue expansion into split naming and split deploy identity, so the target shape can support `*_tradexyz_maker`, `*_tradexyz_taker`, `*_binance_perp_maker`, and `*_binance_perp_taker` for the enrolled universe.

## Abstraction Goals

This sync pass should leave the equities stack with the right seams for additional equities-perp venues beyond Hyperliquid and Binance.

Required abstraction outcomes:

1. Shared strategy contracts describe venue roles and market identity, not one hardcoded venue pair.
2. Shared account projection and balances logic groups by account scope and contract metadata, not by exchange-name special cases.
3. Runner venue resolution is contract-driven and capability-driven, so adding another equities-perp venue does not require another control-plane fork.
4. Shared equities-arb execution helpers separate maker venue behavior, reference/hedge venue behavior, quantity translation, and quote or order-book observation capabilities.
5. Readiness and Fluxboard consume the shared equities contract without assuming one maker venue family owns the page.

Non-goals for this pass:

1. Generalizing beyond equities-perp plus IBKR reference/hedge into unrelated asset classes.
2. Adding cross-strategy arbitration between venue or family variants.
3. Designing a full plugin system for venue behavior; lightweight shared capability seams are enough.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | in_progress | main | none | `systems/flux/flux/common`, `systems/flux/flux/runners/equities`, `systems/flux/flux/runners/shared`, `systems/flux/flux/api`, `systems/flux/flux/strategies`, `deploy/equities`, `fluxboard`, `tests/unit_tests`, `docs/plans` | `shared` | `shared` | none | not_run | Execution started via `subagent-driven-development`; Task 1 completed after the updated upstream PR 65 worktree widened the live universe; Task 2 is now the next ready lane |
| Task 1: Expand Shared Equities Route And Account Contracts | completed | main | none | `systems/flux/flux/common/account_scopes.py`, `systems/flux/flux/common/strategy_contracts.py`, `deploy/equities/equities.live.toml`, `tests/unit_tests/examples/strategies/test_equities_run_api.py`, `tests/unit_tests/examples/strategies/test_equities_run_node.py`, `tests/unit_tests/examples/strategies/test_equities_stack_contract.py` | `shared` | `shared` | `849bf583ef..73c020dcfd` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_stack_contract.py tests/unit_tests/examples/strategies/test_equities_run_api.py tests/unit_tests/examples/strategies/test_equities_run_node.py -q` -> PASS (`98 passed in 1.49s`) | 2026-03-19 UTC Task 1 completed after spec pass and quality pass on `849bf583ef..73c020dcfd`; final route contract commits are `760a36cd44` then `73c020dcfd`; upstream sync reference is `.worktrees/equities-binance-perps-multivenue@a181328b7f3c39b7a0451e26e999360f075b8989` |
| Task 2: Port Shared Account Projection And Portfolio Balance Changes | not_started | unassigned | Task 1: Expand Shared Equities Route And Account Contracts | `systems/flux/flux/runners/shared/profile_accounts.py`, `systems/flux/flux/api/app.py`, `systems/flux/flux/api/_payloads_balances.py`, `tests/unit_tests/flux/common/test_portfolio_inventory.py`, `tests/unit_tests/flux/api/test_balances_merge_dedupe.py`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`, `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py` | `shared` | `shared` | none | not_run | Plan created |
| Task 3: Port Route-Authoritative Runner, API, And Readiness Behavior | not_started | unassigned | Task 1: Expand Shared Equities Route And Account Contracts | `systems/flux/flux/runners/equities/run_node.py`, `systems/flux/flux/runners/equities/run_api.py`, `systems/flux/flux/runners/equities/readiness.py`, `systems/flux/flux/api/app.py`, `systems/flux/flux/api/_payloads_signals.py`, `tests/unit_tests/examples/strategies/test_equities_run_node.py`, `tests/unit_tests/examples/strategies/test_equities_run_api.py`, `tests/unit_tests/examples/strategies/test_equities_readiness.py`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_payloads.py` | `shared` | `shared` | none | not_run | Plan created |
| Task 4: Port Multivenue Split Execution Into Shared Equities-Arb Core | not_started | unassigned | Task 1: Expand Shared Equities Route And Account Contracts | `systems/flux/flux/strategies/shared/equities_arb`, `systems/flux/flux/strategies/equities_maker`, `systems/flux/flux/strategies/equities_taker`, `systems/flux/flux/strategies/makerv4`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`, `tests/unit_tests/flux/strategies/equities_maker`, `tests/unit_tests/flux/strategies/equities_taker`, `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`, `tests/unit_tests/flux/strategies/makerv4/test_pricing.py` | `shared` | `shared` | none | not_run | Plan created |
| Task 5: Translate Multivenue Deploy Strategy And Stack Assets Into Split Naming | not_started | unassigned | Task 1: Expand Shared Equities Route And Account Contracts, Task 3: Port Route-Authoritative Runner, API, And Readiness Behavior, Task 4: Port Multivenue Split Execution Into Shared Equities-Arb Core | `deploy/equities/strategies`, `deploy/equities/equities_stack.env.example`, `deploy/equities/systemd`, `ops/scripts/deploy/install_equities_systemd.sh`, `ops/scripts/deploy/binance_equities_universe.py`, `tests/unit_tests/examples/strategies/test_equities_stack_contract.py` | `shared` | `shared` | none | not_run | Added after upstream PR 65 worktree update confirmed checked-in Binance strategy configs and stack assets |
| Task 6: Port Portable Fluxboard And Public Contract Changes | not_started | unassigned | Task 2: Port Shared Account Projection And Portfolio Balance Changes, Task 3: Port Route-Authoritative Runner, API, And Readiness Behavior, Task 5: Translate Multivenue Deploy Strategy And Stack Assets Into Split Naming | `fluxboard/components/domain/signal`, `fluxboard/config/paramsProfiles.ts`, `fluxboard/types.ts`, `fluxboard/tests/signal`, `fluxboard/__tests__`, `fluxboard/docs/equities_contract.md` | `shared` | `shared` | none | not_run | Renumbered after adding deploy translation task |
| Task 7: Final Verification And PR 64 Sync Update | not_started | unassigned | Task 2: Port Shared Account Projection And Portfolio Balance Changes, Task 3: Port Route-Authoritative Runner, API, And Readiness Behavior, Task 4: Port Multivenue Split Execution Into Shared Equities-Arb Core, Task 5: Translate Multivenue Deploy Strategy And Stack Assets Into Split Naming, Task 6: Port Portable Fluxboard And Public Contract Changes | `docs/plans/2026-03-19-equities-pr65-sync.md`, `GitHub PR #64 body/title if needed` | `shared` | `shared` | none | not_run | Renumbered after adding deploy translation task |

---

### Task 1: Expand Shared Equities Route And Account Contracts

**Files:**
- Modify: `systems/flux/flux/common/account_scopes.py`
- Modify: `systems/flux/flux/common/strategy_contracts.py`
- Modify: `deploy/equities/equities.live.toml`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_api.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Dependencies:** `none`

**Write Scope:** `systems/flux/flux/common/account_scopes.py`, `systems/flux/flux/common/strategy_contracts.py`, `deploy/equities/equities.live.toml`, `tests/unit_tests/examples/strategies/test_equities_run_api.py`, `tests/unit_tests/examples/strategies/test_equities_run_node.py`, `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_stack_contract.py tests/unit_tests/examples/strategies/test_equities_run_api.py tests/unit_tests/examples/strategies/test_equities_run_node.py -q`

**Step 1: Write the failing contract tests**

Add or extend tests so the split branch requires all PR 65 route metadata and account-scope fields:

- `maker_venue`
- `maker_symbol`
- `market_type`
- Binance account scope auth/config fields (`api_key_env`, `api_secret_env`, `account_type`, `private_api_family`, `base_url_http`, `recv_window_ms`)
- translated split deploy naming for new routes instead of `*_makerv4`

Cover two route shapes explicitly:

- same-stock split pair sharing one execution route (`aapl_tradexyz_maker` + `aapl_tradexyz_taker`)
- same-stock multivenue split routes staying distinct while sharing asset-level risk (`aapl_tradexyz_maker` + `aapl_tradexyz_taker` + `aapl_binance_perp_maker` + `aapl_binance_perp_taker`)

**Step 2: Run the targeted contract tests and confirm failure**

Run:

```bash
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py -q
```

Expected: FAIL because the current split branch does not yet decode PR 65’s widened route/account schema.

**Step 3: Implement the widened contract**

Update the shared contract layer and live manifest so the split branch can express PR 65’s multivenue topology:

- extend `AccountScopeConfig` and its decoder with the Binance shared-account fields
- extend `StrategyContractEntry` and its decoder with `maker_venue`, `maker_symbol`, and `market_type`
- add those fields to every existing split `[[strategy_contracts]]` row
- add the Binance shared account scope from PR 65
- translate PR 65’s new Binance routes into split naming in `deploy/equities/equities.live.toml`
- keep the shared API bootstrap as `equities_maker` / `equities_taker`, not `maker_v4`

Add the split Binance strategy identities needed for the target venue/family matrix.
Do not add new contract fields that are really derivable from existing route/account metadata unless a real runner or readiness seam needs them.

**Step 4: Run the targeted contract tests and confirm pass**

Rerun the verification command from Step 2 and expect PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/common/account_scopes.py \
  systems/flux/flux/common/strategy_contracts.py \
  deploy/equities/equities.live.toml \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py
git commit -m "feat: sync equities multivenue route contract"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Port Shared Account Projection And Portfolio Balance Changes

**Files:**
- Modify: `systems/flux/flux/runners/shared/profile_accounts.py`
- Modify: `systems/flux/flux/api/app.py`
- Modify: `systems/flux/flux/api/_payloads_balances.py`
- Test: `tests/unit_tests/flux/common/test_portfolio_inventory.py`
- Test: `tests/unit_tests/flux/api/test_balances_merge_dedupe.py`
- Test: `tests/unit_tests/flux/api/test_app.py`
- Test: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`

**Dependencies:** `Task 1: Expand Shared Equities Route And Account Contracts`

**Write Scope:** `systems/flux/flux/runners/shared/profile_accounts.py`, `systems/flux/flux/api/app.py`, `systems/flux/flux/api/_payloads_balances.py`, `tests/unit_tests/flux/common/test_portfolio_inventory.py`, `tests/unit_tests/flux/api/test_balances_merge_dedupe.py`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`, `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/common/test_portfolio_inventory.py tests/unit_tests/flux/api/test_balances_merge_dedupe.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -q`

**Step 1: Write the failing balance/account tests**

Add tests that pin the combined contract:

- Binance futures shared-account projections build and publish rows/totals for the `equities` profile
- profile account providers group strategies by all referenced shared scopes, not just Hyperliquid/IBKR assumptions
- equities balances keep same-symbol maker+taker dedupe when the pair shares one execution instrument
- same-stock multivenue split routes do not collapse into one execution position
- same-venue maker+taker pairs on Binance dedupe the same way the split branch already dedupes same-venue Hyperliquid maker+taker pairs
- portfolio snapshot fallback merges projection rows/totals without losing split-family same-asset behavior

**Step 2: Run the targeted balance/account tests and confirm failure**

Run:

```bash
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest \
  tests/unit_tests/flux/common/test_portfolio_inventory.py \
  tests/unit_tests/flux/api/test_balances_merge_dedupe.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -q
```

Expected: FAIL because the split branch does not yet expose PR 65’s shared Binance account projection flow.

**Step 3: Implement the shared-account port**

Port the reusable PR 65 logic into the split branch:

- add Binance futures shared-account providers to `profile_accounts.py`
- keep the shared reference-balance import on `shared.equities_arb.reference_balances`
- load profile account projection rows/totals in `app.py`
- merge those rows/totals into equities balances without regressing existing shared-position grouping
- preserve same-asset maker+taker grouping by reusing `shared_observation_group_by_strategy_id`

Do not collapse cross-venue maker routes together unless they truly share the same execution venue/account/instrument.
Make the grouping and provider-selection logic generic over account-scope metadata so later equities-perp venues can reuse the same path.

**Step 4: Run the targeted balance/account tests and confirm pass**

Rerun the verification command from Step 2 and expect PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/runners/shared/profile_accounts.py \
  systems/flux/flux/api/app.py \
  systems/flux/flux/api/_payloads_balances.py \
  tests/unit_tests/flux/common/test_portfolio_inventory.py \
  tests/unit_tests/flux/api/test_balances_merge_dedupe.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py
git commit -m "feat: sync equities shared account projections"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Port Route-Authoritative Runner, API, And Readiness Behavior

**Files:**
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `systems/flux/flux/runners/equities/run_api.py`
- Modify: `systems/flux/flux/runners/equities/readiness.py`
- Modify: `systems/flux/flux/api/app.py`
- Modify: `systems/flux/flux/api/_payloads_signals.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_api.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_readiness.py`
- Test: `tests/unit_tests/flux/api/test_app.py`
- Test: `tests/unit_tests/flux/api/test_payloads.py`

**Dependencies:** `Task 1: Expand Shared Equities Route And Account Contracts`

**Write Scope:** `systems/flux/flux/runners/equities/run_node.py`, `systems/flux/flux/runners/equities/run_api.py`, `systems/flux/flux/runners/equities/readiness.py`, `systems/flux/flux/api/app.py`, `systems/flux/flux/api/_payloads_signals.py`, `tests/unit_tests/examples/strategies/test_equities_run_node.py`, `tests/unit_tests/examples/strategies/test_equities_run_api.py`, `tests/unit_tests/examples/strategies/test_equities_readiness.py`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_payloads.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_run_node.py tests/unit_tests/examples/strategies/test_equities_run_api.py tests/unit_tests/examples/strategies/test_equities_readiness.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_payloads.py -q`

**Step 1: Write the failing runner/API tests**

Add tests for the split-aware version of PR 65’s contract-authoritative behavior:

- `run_node.py` rewrites execution venue, maker instrument, and IBKR scope settings from the selected strategy contract
- `run_api.main()` publishes per-route metadata for same-stock multivenue routes
- readiness keeps using the shared equities contract and profile-required strategy set
- signal payloads carry the portable PR 65 additions such as `max_ibkr_quote_age_ms` and paused/stale-state handling without breaking the split branch’s existing `hl_*` fee field contract
- params endpoints remain split-family aware and do not become ambiguous profile-wide `maker_v4` endpoints

**Step 2: Run the targeted runner/API tests and confirm failure**

Run:

```bash
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_readiness.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_payloads.py -q
```

Expected: FAIL because the split branch does not yet express PR 65’s multivenue route resolution and shared signal additions.

**Step 3: Implement the runner/API port**

Port the reusable runner/API behavior while keeping the split contract intact:

- extend the shared venue-resolution path to honor `maker_venue`, `maker_symbol`, and route-owned IBKR scope overrides
- move any remaining “Hyperliquid is the maker venue” assumptions behind shared equities-arb capability helpers
- keep strategy-spec resolution routed through the split/shared equities-arb helpers
- ensure `run_api.py` binds metadata from real merged config and widened `strategy_contracts`
- keep readiness on the shared `equities` contract and shared portfolio view
- port only the signal payload changes that are family-neutral

Do not reintroduce `maker_v4` as the shared `/equities` family or default params contract.

**Step 4: Run the targeted runner/API tests and confirm pass**

Rerun the verification command from Step 2 and expect PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/runners/equities/run_node.py \
  systems/flux/flux/runners/equities/run_api.py \
  systems/flux/flux/runners/equities/readiness.py \
  systems/flux/flux/api/app.py \
  systems/flux/flux/api/_payloads_signals.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_readiness.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_payloads.py
git commit -m "feat: sync equities multivenue runner contract"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Port Multivenue Split Execution Into Shared Equities-Arb Core

**Files:**
- Modify: `systems/flux/flux/strategies/shared/equities_arb/core.py`
- Modify: `systems/flux/flux/strategies/shared/equities_arb/instruments.py`
- Modify: `systems/flux/flux/strategies/shared/equities_arb/hedging.py`
- Modify: `systems/flux/flux/strategies/shared/equities_arb/observability.py`
- Modify: `systems/flux/flux/strategies/equities_maker/strategy.py`
- Modify: `systems/flux/flux/strategies/equities_taker/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/instruments.py`
- Modify: `systems/flux/flux/strategies/makerv4/pricing.py`
- Modify: `systems/flux/flux/strategies/makerv4/fees.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Test: `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`
- Test: `tests/unit_tests/flux/strategies/equities_maker/test_strategy.py`
- Test: `tests/unit_tests/flux/strategies/equities_taker/test_strategy.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_pricing.py`

**Dependencies:** `Task 1: Expand Shared Equities Route And Account Contracts`

**Write Scope:** `systems/flux/flux/strategies/shared/equities_arb`, `systems/flux/flux/strategies/equities_maker/strategy.py`, `systems/flux/flux/strategies/equities_taker/strategy.py`, `systems/flux/flux/strategies/makerv4/instruments.py`, `systems/flux/flux/strategies/makerv4/pricing.py`, `systems/flux/flux/strategies/makerv4/fees.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`, `tests/unit_tests/flux/strategies/equities_maker/test_strategy.py`, `tests/unit_tests/flux/strategies/equities_taker/test_strategy.py`, `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`, `tests/unit_tests/flux/strategies/makerv4/test_pricing.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py tests/unit_tests/flux/strategies/equities_maker/test_strategy.py tests/unit_tests/flux/strategies/equities_taker/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_instruments.py tests/unit_tests/flux/strategies/makerv4/test_pricing.py -q`

**Step 1: Write the failing strategy-layer tests**

Add or extend tests for the portable PR 65 strategy behavior plus the clarified four-strategy venue/family matrix:

- base-qty-aware maker fill translation and any needed compatibility mapping from PR 65 fee snapshots into the split branch’s existing `hl_*` operator fields
- base-quantity-aware fill translation instead of assuming venue qty equals stock qty
- order-book/BBO handling for maker venues that publish book deltas
- shared quote payload propagation of `max_ibkr_quote_age_ms`
- split-family reuse of the shared venue translation helpers on both Hyperliquid and Binance
- Binance taker path coverage where the current split taker path still assumes Hyperliquid-specific maker venue details

**Step 2: Run the targeted strategy tests and confirm failure**

Run:

```bash
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest \
  tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py \
  tests/unit_tests/flux/strategies/equities_maker/test_strategy.py \
  tests/unit_tests/flux/strategies/equities_taker/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py \
  tests/unit_tests/flux/strategies/makerv4/test_pricing.py -q
```

Expected: FAIL because the split branch does not yet include the full multivenue split execution refinements required by PR 65 plus the clarified four-strategy target shape.

**Step 3: Implement the shared-core port**

Move the reusable behavior into the shared/split seam:

- add base-qty-aware maker fill translation helpers to the shared equities-arb layer
- let `equities_maker` consume those helpers directly
- extend `equities_taker` onto the same shared venue/instrument helpers where its current path still assumes Hyperliquid-specific maker details
- introduce lightweight venue-capability seams for maker quote source, book/BBO support, quantity translation, and route normalization instead of scattering venue-name branches through strategy code
- retain `makerv4` compatibility only as a thin compatibility wrapper for legacy tests
- port the quote payload and conversion additions without restoring a `maker_v4`-owned signal contract or breaking the existing `hl_*` fee contract

This task is not limited to PR 65’s maker-only proof points anymore. The clarified target requires the split strategy layer to support both venues for both families while staying on one shared equities risk/book model.

**Step 4: Run the targeted strategy tests and confirm pass**

Rerun the verification command from Step 2 and expect PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/shared/equities_arb \
  systems/flux/flux/strategies/equities_maker/strategy.py \
  systems/flux/flux/strategies/equities_taker/strategy.py \
  systems/flux/flux/strategies/makerv4/instruments.py \
  systems/flux/flux/strategies/makerv4/pricing.py \
  systems/flux/flux/strategies/makerv4/fees.py \
  systems/flux/flux/strategies/makerv4/strategy.py \
  tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py \
  tests/unit_tests/flux/strategies/equities_maker/test_strategy.py \
  tests/unit_tests/flux/strategies/equities_taker/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py \
  tests/unit_tests/flux/strategies/makerv4/test_pricing.py
git commit -m "feat: sync equities multivenue split execution"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Translate Multivenue Deploy Strategy And Stack Assets Into Split Naming

**Files:**
- Modify: `deploy/equities/strategies`
- Modify: `deploy/equities/strategies/equities.strategy.template.toml`
- Modify: `deploy/equities/equities_stack.env.example`
- Modify: `deploy/equities/systemd/common.env.example`
- Modify: `deploy/equities/systemd/flux-equities.target`
- Modify: `deploy/equities/systemd/flux-pulse.sudoers`
- Modify: `ops/scripts/deploy/install_equities_systemd.sh`
- Add or Modify: `ops/scripts/deploy/binance_equities_universe.py`
- Modify: `deploy/equities/README.md`
- Modify: `deploy/equities/strategies/README.md`
- Test: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Dependencies:** `Task 1: Expand Shared Equities Route And Account Contracts`, `Task 3: Port Route-Authoritative Runner, API, And Readiness Behavior`, `Task 4: Port Multivenue Split Execution Into Shared Equities-Arb Core`

**Write Scope:** `deploy/equities/strategies`, `deploy/equities/equities_stack.env.example`, `deploy/equities/systemd/common.env.example`, `deploy/equities/systemd/flux-equities.target`, `deploy/equities/systemd/flux-pulse.sudoers`, `ops/scripts/deploy/install_equities_systemd.sh`, `ops/scripts/deploy/binance_equities_universe.py`, `deploy/equities/README.md`, `deploy/equities/strategies/README.md`, `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_stack_contract.py -q`

**Step 1: Write the failing deploy and stack-contract tests**

Add or extend tests so the split branch requires all of the following:

- checked-in split Binance strategy TOMLs exist for the enrolled multivenue symbols and use split IDs such as `pltr_binance_perp_maker` and `pltr_binance_perp_taker`
- translated Hyperliquid and Binance strategy TOMLs keep the split family contract (`equities_maker`, `equities_taker`) and do not restore local inventory or risk knobs
- the strategy template and env examples document Binance credentials and shared account-scope ownership
- systemd and installer assets discover, install, and authorize the translated split multivenue node service names
- any upstream helper needed to audit enrolled Binance coverage, such as `binance_equities_universe.py`, reads the shared split manifest rather than legacy `*_makerv4` IDs

**Step 2: Run the targeted deploy tests and confirm failure**

Run:

```bash
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py -q
```

Expected: FAIL because the split branch does not yet translate PR 65's checked-in Binance deploy configs and stack assets.

**Step 3: Implement the translated deploy surface**

Translate the updated PR 65 deploy lane into the split branch:

- convert the upstream `*_makerv4` strategy TOMLs into split `*_maker` and `*_taker` strategy configs for both Hyperliquid and Binance-enrolled routes
- keep the split family naming, params, and outside-RTH semantics intact
- wire venue-specific node config only where required by the translated route
- update shared env examples, systemd assets, and installer generation for the expanded split node matrix
- update deploy docs to describe the translated multivenue split stack rather than reviving `maker_v4`

Do not widen the active `/equities` control plane away from `equities_maker` and `equities_taker`.

**Step 4: Run the targeted deploy tests and confirm pass**

Rerun the verification command from Step 2 and expect PASS.

**Step 5: Commit**

```bash
git add \
  deploy/equities/strategies \
  deploy/equities/equities_stack.env.example \
  deploy/equities/systemd/common.env.example \
  deploy/equities/systemd/flux-equities.target \
  deploy/equities/systemd/flux-pulse.sudoers \
  ops/scripts/deploy/install_equities_systemd.sh \
  ops/scripts/deploy/binance_equities_universe.py \
  deploy/equities/README.md \
  deploy/equities/strategies/README.md \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py
git commit -m "feat: sync equities multivenue deploy surface"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 6: Port Portable Fluxboard And Public Contract Changes

**Files:**
- Modify: `fluxboard/components/domain/signal/EquitiesArbSignalTable.tsx`
- Modify: `fluxboard/components/domain/signal/MakerV4SignalTable.tsx`
- Modify: `fluxboard/config/paramsProfiles.ts`
- Modify: `fluxboard/types.ts`
- Test: `fluxboard/tests/signal/EquitiesArbSignalTable.test.tsx`
- Test: `fluxboard/tests/signal/SignalFamilyFilter.test.tsx`
- Test: `fluxboard/__tests__/config/paramsProfiles.test.ts`
- Test: `fluxboard/api.flux.test.ts`
- Test: `fluxboard/Signal.delta-pass-through.test.tsx`
- Test: `fluxboard/components/domain/signal/SignalTable.store.test.ts`
- Modify: `fluxboard/docs/equities_contract.md`

**Dependencies:** `Task 2: Port Shared Account Projection And Portfolio Balance Changes`, `Task 3: Port Route-Authoritative Runner, API, And Readiness Behavior`, `Task 5: Translate Multivenue Deploy Strategy And Stack Assets Into Split Naming`

**Write Scope:** `fluxboard/components/domain/signal/EquitiesArbSignalTable.tsx`, `fluxboard/components/domain/signal/MakerV4SignalTable.tsx`, `fluxboard/config/paramsProfiles.ts`, `fluxboard/types.ts`, `fluxboard/tests/signal/EquitiesArbSignalTable.test.tsx`, `fluxboard/tests/signal/SignalFamilyFilter.test.tsx`, `fluxboard/__tests__/config/paramsProfiles.test.ts`, `fluxboard/api.flux.test.ts`, `fluxboard/Signal.delta-pass-through.test.tsx`, `fluxboard/components/domain/signal/SignalTable.store.test.ts`, `fluxboard/docs/equities_contract.md`

**Verification Commands:**
- `pnpm --dir /home/ubuntu/nautilus_trader/.worktrees/makerv4-split-dual-arb-impl-20260317/fluxboard exec vitest run tests/signal/EquitiesArbSignalTable.test.tsx tests/signal/SignalFamilyFilter.test.tsx __tests__/config/paramsProfiles.test.ts api.flux.test.ts Signal.delta-pass-through.test.tsx components/domain/signal/SignalTable.store.test.ts`

**Step 1: Write the failing Fluxboard/doc tests**

Add or extend tests so the split surface requires all of the following:

- `/equities/signal` stays on the shared split-family table and keeps the family filter
- portable PR 65 improvements such as robust undefined-last spread sorting, stale-quote gating, and quote-health display fixes land on the split table
- params profile logic continues to resolve `equities_maker` and `equities_taker`
- websocket/store delta handling accepts the widened shared equities contract
- docs describe the translated multivenue contract instead of reviving `maker_v4` language

**Step 2: Run the targeted Fluxboard/doc tests and confirm failure**

Run:

```bash
pnpm --dir /home/ubuntu/nautilus_trader/.worktrees/makerv4-split-dual-arb-impl-20260317/fluxboard exec vitest run \
  tests/signal/EquitiesArbSignalTable.test.tsx \
  tests/signal/SignalFamilyFilter.test.tsx \
  __tests__/config/paramsProfiles.test.ts \
  api.flux.test.ts \
  Signal.delta-pass-through.test.tsx \
  components/domain/signal/SignalTable.store.test.ts
```

Expected: FAIL because the split Fluxboard surface does not yet include the portable PR 65 improvements.

**Step 3: Implement the portable UI/docs port**

Apply only the PR 65 pieces that are compatible with the split control plane:

- keep `EquitiesArbSignalTable` as the active `/equities` table
- keep `equities_maker` / `equities_taker` params profiles and labels
- port the stale-quote, observability, and sorting improvements while keeping the split branch’s existing `hl_*` fee labels/types unless a separate contract migration is approved
- update deploy and Fluxboard docs to describe the widened multivenue split contract for Hyperliquid maker/taker plus Binance maker/taker

Do not port PR 65’s implicit `/equities -> maker_v4` lock or any UI assumption that removes split-family visibility.

**Step 4: Run the targeted Fluxboard/doc tests and confirm pass**

Rerun the verification command from Step 2 and expect PASS.

**Step 5: Commit**

```bash
git add \
  fluxboard/components/domain/signal/EquitiesArbSignalTable.tsx \
  fluxboard/components/domain/signal/MakerV4SignalTable.tsx \
  fluxboard/config/paramsProfiles.ts \
  fluxboard/types.ts \
  fluxboard/tests/signal/EquitiesArbSignalTable.test.tsx \
  fluxboard/tests/signal/SignalFamilyFilter.test.tsx \
  fluxboard/__tests__/config/paramsProfiles.test.ts \
  fluxboard/api.flux.test.ts \
  fluxboard/Signal.delta-pass-through.test.tsx \
  fluxboard/components/domain/signal/SignalTable.store.test.ts \
  fluxboard/docs/equities_contract.md
git commit -m "feat: sync equities multivenue operator surface"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 7: Final Verification And PR 64 Sync Update

**Files:**
- Modify: `docs/plans/2026-03-19-equities-pr65-sync.md`
- Modify: `GitHub PR #64 body/title if needed`

**Dependencies:** `Task 2: Port Shared Account Projection And Portfolio Balance Changes`, `Task 3: Port Route-Authoritative Runner, API, And Readiness Behavior`, `Task 4: Port Multivenue Split Execution Into Shared Equities-Arb Core`, `Task 5: Translate Multivenue Deploy Strategy And Stack Assets Into Split Naming`, `Task 6: Port Portable Fluxboard And Public Contract Changes`

**Write Scope:** `docs/plans/2026-03-19-equities-pr65-sync.md`, `GitHub PR #64 body/title if needed`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest --import-mode=importlib tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py tests/unit_tests/flux/strategies/equities_maker/test_runtime_params.py tests/unit_tests/flux/strategies/equities_maker/test_strategy.py tests/unit_tests/flux/strategies/equities_taker/test_runtime_params.py tests/unit_tests/flux/strategies/equities_taker/test_strategy.py tests/unit_tests/examples/strategies/test_equities_run_api.py tests/unit_tests/examples/strategies/test_equities_run_node.py tests/unit_tests/examples/strategies/test_equities_run_portfolio.py tests/unit_tests/examples/strategies/test_equities_stack_contract.py tests/unit_tests/examples/strategies/test_equities_readiness.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_balances_merge_dedupe.py tests/unit_tests/flux/common/test_portfolio_inventory.py -q`
- `pnpm --dir /home/ubuntu/nautilus_trader/.worktrees/makerv4-split-dual-arb-impl-20260317/fluxboard exec vitest run tests/signal/EquitiesArbSignalTable.test.tsx tests/signal/SignalFamilyFilter.test.tsx __tests__/config/paramsProfiles.test.ts api.flux.test.ts Signal.delta-pass-through.test.tsx components/domain/signal/SignalTable.store.test.ts`
- `git -C /home/ubuntu/nautilus_trader/.worktrees/makerv4-split-dual-arb-impl-20260317 diff --check`

**Step 1: Run the full regression bundle**

Run every verification command above from the shared worktree after the sync tasks land. Use `--import-mode=importlib` for the combined pytest bundle to avoid duplicate-basename collection collisions.

**Step 2: Fix any residual integration gaps**

If the full bundle exposes drift between the ported PR 65 logic and the split branch, fix the regression in the owning task’s write scope before moving on. Update the tracker with the exact diff and rerun the relevant targeted command plus the full bundle.

**Step 3: Update PR 64**

Refresh PR 64 so it explicitly calls out:

- synced with PR 65 / multivenue equities changes
- split contract preserved (`equities_maker`, `equities_taker`)
- Binance multivenue/account updates translated into the split branch
- exact final verification commands and results

**Step 4: Commit the tracker/docs update**

```bash
git add docs/plans/2026-03-19-equities-pr65-sync.md
git commit -m "docs: record equities pr65 sync completion"
```

**Step 5: Update the Progress Tracker**

Mark Task 7 and `Overall` complete only after the full verification bundle passes and PR 64 is updated.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
