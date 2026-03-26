# Equities MakerV4 Split Design

## Goal

Replace the current single `MakerV4` equities strategy family with two explicit, concurrently deployable Nautilus strategy families:

- `maker`: quote the Hyperliquid strategy market and hedge/take against IBKR after fills
- `taker`: trade outside arb bounds by taking on both venues

Both variants must remain on the shared equities control plane:

- shared `portfolio=equities`
- shared per-asset risk/book view
- shared `/equities` Fluxboard surface
- shared deploy stack and readiness model

The live equities control-plane contract is the split `equities_maker` / `equities_taker` family surface. The refreshed implementation still keeps narrow legacy `maker_v4` / `makerv4` compatibility shims in API metadata resolution, readiness parsing, and Fluxboard types so stale persisted state and mixed-rollout artifacts can be read during the finish pass, but new deploy/API/UI surfaces are expected to emit the split-family contract.

## Recommendation

Use two explicit strategy families with a shared equities-arb core:

- `equities_maker`
- `equities_taker`

Keep them as separate strategy IDs, separate node configs, and separate runtime param registries, but make them read the same shared portfolio/book for asset-level risk. Do not add cross-strategy arbitration in this wave. If both variants on `AAPL` are live, both may trade as long as the shared asset-level risk gate permits it.

This gives the cleanest operator model:

- the variant is visible in the strategy ID and family metadata
- runtime params can stay explicitly separate for `maker` and `taker`
- shared logic moves into reusable modules instead of remaining hidden behind `execution_mode`

## Naming

### Strategy IDs

Recommended live strategy ID pattern:

- `<symbol>_tradexyz_maker`
- `<symbol>_tradexyz_taker`

Examples:

- `aapl_tradexyz_maker`
- `aapl_tradexyz_taker`

### Strategy Metadata

Recommended strategy metadata:

- `strategy_family = "equities_maker"` / `"equities_taker"`
- `strategy_version = "v1"`
- `param_set = "equities_maker"` / `"equities_taker"`
- `strategy_class = "equities_maker"` / `"equities_taker"`

### Operator Params Profiles

`/equities/params` stays one shared route, but the params contract should be separate for the two strategy families and keyed off the selected strategy in the operator dropdown:

- `params_profile = "equities_maker"` for a selected maker row
- `params_profile = "equities_taker"` for a selected taker row
- `GET /api/v1/param-schema` and related Fluxboard schema caching must become strategy-aware, for example via a `strategy=<strategy_id>` selector, rather than assuming one page-global schema
- the generic params API must resolve schema/defaults/validation per strategy across `GET /api/v1/params`, bulk `POST/PATCH /api/v1/params`, and `GET/POST/PATCH /api/v1/strategies/<id>/parameters`; one app-global schema/default bundle is not sufficient for the split
- the live `run_api.main()` path must bind per-strategy family and asset metadata from merged config plus `strategy_contracts`, not only helper-unit tests that inject contracts into an isolated `[api]` table
- row metadata still carries family-specific `param_set` / `strategy_family`
- persisted Fluxboard `maker_v4` params preferences should migrate into the new split profiles
- legacy `maker_v4` / `makerv4` selectors may remain accepted as fallback aliases while old persisted state is drained, but active equities rows and new operator actions should resolve to `equities_maker` / `equities_taker`

### Operator Labels

Use operator-facing labels exactly as:

- `Maker`
- `Taker`

That keeps the GUI aligned with the trading behavior the desk already uses in conversation.

## Strategy Semantics

### Shared Semantics

Both strategies:

- trade the same underlying HL-vs-IBKR arb
- are fee-aware
- publish fee and pricing assumptions into signal/operator payloads
- preserve the existing `makerv4` RTH vs outside-RTH hedge semantics exactly, including the current `outside_rth_hedge_enabled` behavior and the existing deploy contract that keeps IBKR reference data available outside regular-session hours
- use the same shared quote-health, hedge-policy, pending-hedge, and hedge-backlog concepts
- read the same shared equities portfolio/book for asset-level risk and inventory checks

Strategy-local state remains limited to execution mechanics:

- managed maker/taker orders
- cooldowns
- pending hedge state
- hedge backlog / retry state

Strategy-local inventory ownership is not introduced in this split, and the split families should not expose legacy local inventory/risk knobs such as `des_qty_local`, `max_qty_local`, and `max_skew_bps_local` or new equivalents that imply strategy-owned asset risk.

### Maker

`maker` is the inside-bounds strategy:

- quotes the Hyperliquid strategy market
- publishes maker-side quote targets and maker-specific observability
- hedges against IBKR after fills using the shared hedge path
- keeps the existing immediate-hedge and outside-RTH hedge behavior unchanged in this wave

### Taker

`taker` is the outside-bounds strategy:

- computes fee-aware outside-band opportunities
- is defined as a taker-on-both-venues strategy rather than a maker-quote strategy with a different threshold
- owns aggressive entry logic on the strategy market and aggressive hedge/take logic on the IBKR side using the shared hedge path
- does not reuse the maker quote lifecycle as its primary execution model
- keeps the current backlog/fail-closed behavior when the hedge side is not usable

## Shared Core Design

Split the current `makerv4` code into:

1. Shared equities-arb core under `systems/flux/flux/strategies/shared/equities_arb/`
2. Family-specific strategy modules under:
   - `systems/flux/flux/strategies/equities_maker/`
   - `systems/flux/flux/strategies/equities_taker/`

Recommended shared-core responsibilities:

- shared quote-health evaluation and quote snapshot assembly
- shared fee assumptions and fee-aware observability fields
- shared hedge/take order intent and pending-hedge/backlog lifecycle
- shared portfolio-risk reads against the `portfolio=equities` aggregate
- shared signal/operator payload helpers
- shared fee-rule and pricing helpers that currently live in `makerv4.fees` and `makerv4.pricing`
- shared runner capability helpers so `run_node.py` no longer branches on `param_set == "makerv4"` for runtime params, immediate-hedge behavior, venue promotion, or allowed instruments

Family-specific responsibilities:

- `maker`: quote generation and quote lifecycle
- `taker`: outside-band opportunity detection and two-sided taker execution logic
- family-specific runtime params and defaults

Implementation note:

- `maker` can remain relatively thin after the shared-core extraction
- `taker` is expected to be a more substantial extraction because its current taker behavior is interleaved into the `makerv4` state machine

## Fluxboard Design

Replace the dedicated `MakerV4SignalTable` contract with a shared equities-arb signal table, for example `EquitiesArbSignalTable`.

That table should:

- show one row per strategy instance, not one row per symbol
- include a visible `Variant` column (`Maker` / `Taker`)
- sort naturally by symbol then variant
- keep the shared maker/hedge/ref leg presentation where relevant
- show fee assumptions, hedge policy, and quote-health with consistent semantics across both families

Shared equities operator model:

- same `/equities/signal`
- same `/equities/params`
- same `/equities/balances`
- same `/equities/trades`
- no separate app surface for each family
- params schema/profile switches by the selected strategy/family on the shared `/equities/params` page

Params UX should expose:

- common controls first
- session and shared-risk controls before family-specific execution knobs
- family-specific fields grouped after the common block
- rows ordered in an operator-friendly sequence by symbol then variant
- no local-inventory/local-risk ownership controls
- migrated stored `maker_v4` params preferences into the new split params profiles

## Deploy And Portfolio Design

Keep the current “one strategy instance per node” deploy shape for this wave, but allow two strategy instances per symbol.

Implications:

- `strategy_contracts` may now contain two rows with the same `portfolio_asset_id`
- `api.equities_strategy_ids` and `api.equities_required_strategy_ids` must allow both rows
- the existing tuple-based asset grouping in `run_portfolio.py` should be reused rather than replaced; only patch portfolio-runner logic if deploy/readiness tests expose a real remaining gap
- readiness must validate shared asset-level state while still expecting both strategy processes to be present when enrolled

The shared portfolio/book remains canonical for:

- max long / max short per asset
- shared inventory exposure per asset
- any later shared asset gate added to equities

## Error Handling

Wave-1 policy:

- no new arbitration or stand-down rules between `maker` and `taker`
- no new “inside regime owns the symbol” behavior
- preserve fail-closed hedge behavior when the hedge side is invalid
- preserve retryable hedge backlog behavior where already present

If concurrent variants exhibit undesirable interaction in live trading, add an explicit cross-strategy coordination layer later rather than hiding it in this split.

## Testing Strategy

The split needs contract coverage at four levels:

1. Strategy-family registry and runner wiring
2. Shared portfolio/deploy/readiness contracts for dual strategies per asset
3. API payload and Fluxboard family-aware rendering
4. Shared-core hedge, fee, and quote-health behavior reused by both families

Execution sequencing should keep the shared lane green:

- local fail-first testing inside a task is fine
- committed red shared-branch contract tests are not part of the plan
- generic `flux.api.app` mixed-family params tests and Fluxboard websocket delta-merge tests are required coverage, not optional polish

The migration is complete when:

- no active equities deploy or API contract depends on `makerv4`
- both families can be represented under the same equities stack
- the shared UI and params surfaces remain operator-friendly

## Open Decisions Closed By This Design

The design below assumes:

- both variants can run concurrently for the same stock
- both share the same equities portfolio/book and shared asset-level risk view
- node topology is not redesigned in this wave
- narrow legacy `maker_v4` / `makerv4` compatibility shims remain in place during the finish pass, but the split-family contract is the only active equities deploy surface
- no cross-strategy arbitration is added in this wave
