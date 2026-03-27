# Equities Shared Symbol-Node Design

## Goal

Stabilize the live equities stack by changing the runtime unit from "one node per strategy" to "one node per symbol plus maker venue", while keeping the existing external `equities` operator surface unchanged.

The immediate production problem is not strategy logic. It is topology. The current split maker/taker deploy runs `38` equities node processes, and each process still opens its own IBKR client session on the single usable live gateway at `127.0.0.1:4001`. Shared balances scope health is now green, but live `balances.degraded` and stale signal rows persist because node-level IBKR handshakes fail under that session count.

This design is intentionally scoped to node-owned IBKR session pressure. The stack also depends on a shared IBKR publisher path, and grouped nodes do not by themselves prove that every quote-freshness problem disappears. The goal of this wave is narrower: stop node handshake exhaustion, preserve the existing external contract, and make the remaining market-data issues honest shared-feed problems instead of self-inflicted topology breakage.

## Options

### Option 1: Keep `38` nodes and add more IBKR gateways

This is the lowest code change, but it is not the best immediate design. The host has a second gateway container on `127.0.0.1:4002`, but it is not a clean production-grade capacity lane today. It still depends on separate login state and does not remove the structural duplication of one IBKR session per strategy process.

### Option 2: Keep per-strategy TOMLs, but group them into shared symbol/venue nodes

This is the recommended option.

Keep the existing `38` strategy IDs, strategy TOMLs, API rows, params families, trades, and Fluxboard behavior. Change only the runtime/deploy contract so one node process can host both maker and taker for the same `(portfolio_asset_id, maker_venue)` pair.

Examples:

- `aapl_tradexyz_maker` + `aapl_tradexyz_taker` -> one node `aapl_tradexyz`
- `amzn_binance_perp_maker` + `amzn_binance_perp_taker` -> one node `amzn_binance_perp`

This cuts the current production node count from `38` to `19` without inventing a second config format.

### Option 3: Introduce a new dedicated node manifest and retire per-strategy TOMLs

This is cleaner long-term, but it adds too much churn for the stabilization wave. It would require a new config shape, new installer discovery rules, and a bigger operator migration at exactly the point where prod needs the smallest viable topology fix.

## Recommendation

Implement Option 2.

It preserves the current external strategy contract and deploy metadata while fixing the runtime topology that is breaking prod. It is the smallest change that materially reduces IBKR session pressure and makes balances/readiness recovery plausible on the existing live gateway.

## External Contract

These surfaces must not change in this wave:

- `profile=equities`
- `/equities`
- `38` external strategy IDs
- `equities_maker` / `equities_taker` params and Fluxboard families
- strategy-level `signals`, `params`, and selector behavior
- profile-level `balances` payload shape and readiness metadata
- `trades` / `alerts` attribution on original external strategy IDs when rows are present
- `/equities` realtime behavior and Fluxboard transport compatibility

The API and UI should still behave as if there are `38` independently addressable strategies. The node topology becomes an internal implementation detail.

## Node Contract

Introduce a stable node-group identity derived from the existing split strategy IDs:

- `aapl_tradexyz`
- `amd_tradexyz`
- `amzn_binance_perp`
- `coin_binance_perp`

Each node group owns:

- one `portfolio_asset_id`
- one maker venue
- one maker instrument id
- one reference instrument id
- one execution account scope
- one reference account scope
- optional one hedge account scope
- one or two strategy members

Expected live cardinality for the current prod basket:

- `10` tradexyz node groups
- `9` binance-perp node groups
- total `19` node services
- total `38` strategy instances inside those services

## Runtime Design

The runner refactor is not just “accept two configs.” The current equities runtime is single-strategy end to end, so the grouped-node wave must explicitly change the runner contract.

Required runtime shape:

1. Add a canonical grouping helper that reads `strategy_contracts` plus enrolled strategy TOMLs and returns grouped node definitions.
2. Extend the equities runner so one node process can accept multiple strategy config paths from the same node group.
3. Build one shared `TradingNodeConfig`, one shared venue-client set, and one shared node-scoped identity per node group.
4. Instantiate one strategy object per enrolled member strategy, attach all of them to the same `TradingNode`, and preserve each strategy's existing external identity.
5. Rework the runner hooks that are currently single-strategy assumptions:
   - runtime params attachment
   - portfolio inventory component publication
   - projection / reference-balance feed attachment
   - strategy-local event emission

The important boundary is:

- clients, cache, message bus, reconciliation engine, and risk engine are node-scoped
- strategy IDs, runtime params, events, trades, balances attribution, and observability remain strategy-scoped

## Strategy Config Contract

Do not introduce a new node TOML format in this wave.

Keep the existing per-strategy TOMLs under `deploy/equities/strategies/*.toml` as the source of:

- strategy family (`equities_maker` / `equities_taker`)
- family-specific runtime defaults
- bot state (`bot_on`)
- family-specific execution knobs
- legacy/operator-visible strategy-local config

The grouped node runner should consume multiple strategy TOMLs and one shared `equities.live.toml`. Group membership is derived from shared contract metadata, not from filename conventions alone.

## Deploy Contract

The installer should stop rendering one service per strategy and start rendering one service per node group.

Expected prod service names:

- `equities-node-aapl_tradexyz`
- `equities-node-amzn_binance_perp`
- `equities-node-tsla_binance_perp`

What must stay true:

- `equities-api`, `equities-portfolio`, and `equities-bridge` remain unchanged
- `/etc/flux/equities-node-*.env` still points only at immutable release roots
- Pulse still exposes a single `equities` group

What changes:

- the target enrolls `19` node units, not `38`
- each node env launches the grouped node runner with multiple strategy configs
- stale per-strategy node env files are removed during cutover
- stale per-strategy node units are explicitly stopped before grouped units are started

## API And Operator Contract

The grouped-node wave must preserve the current API and Pulse operator surface even though service IDs change.

That means:

- `run_api.py` can no longer assume `equities-node-<strategy_id>` is the live Pulse job id
- strategy running-state and pulse-backed alerts must resolve from external strategy id -> node-group id
- `/api/v1/signals`, `/api/v1/balances`, `/api/v1/trades`, `/api/v1/alerts`, `/api/v1/params`, and `/api/v1/param-schema` must still expose external strategy IDs
- `/equities` must keep its current realtime contract over the public route
- Pulse must continue to expose one `equities` group with the grouped node services plus the existing portfolio/bridge services
- the regenerated `/etc/sudoers.d/flux-pulse` contract must match the grouped node service names exactly
- `equities-api` remains the read surface and is still intentionally not a Pulse-managed restart target

## Message Bus And Reconciliation

Today the equities node message-bus stream prefix is keyed off the external strategy ID because each process hosts one strategy. After grouping, the node-scoped resources need a node-group identity instead.

Recommended rule:

- node-scoped runtime identifiers use the node-group id
- strategy-scoped payloads continue to use the external strategy ID
- no mixed-version operation is allowed once node-scoped identifiers change

That means:

- one node input stream prefix per node group
- one shared venue/execution reconciliation pass per node group
- strategy-level event payloads and trades remain attributable to the original strategy IDs

This is the most sensitive internal contract change in the wave and should be covered explicitly in tests.

## Order Ownership

The current split families already share the same asset/account scope and use external strategy IDs plus client-order metadata to distinguish ownership. Grouping maker+taker into one process removes inter-process order-claim competition for the same symbol/venue pair, but it also changes the boundary where ownership is enforced.

Wave scope:

- keep current strategy-local identifiers and external strategy IDs
- keep current strategy-level order attribution and fill attribution
- do not add new cross-strategy arbitration logic
- do not merge strategy state

What must be made explicit:

- same-symbol maker+taker siblings still emit distinct strategy IDs
- shared-node execution must not collapse fills, alerts, or external-order attribution to the node-group id
- external-order claim behavior must be tested for stray orders, sibling orders, and duplicate-claim rejection inside one shared node

## Rollout

This should be a controlled prod cutover, not an experimental pilot.

Because pilot is intentionally out of scope for this wave, the prod rollout must carry equivalent safeguards:

- a fixed maintenance window
- pre-cutover release root creation and verification
- explicit stop of all retired per-strategy node units before grouped units are started
- no mixed old/new node processes at any time
- fail-closed readiness verification before the stack is declared healthy

Sequence:

1. Land the grouped-node runtime and installer support.
2. Cut a fresh immutable equities prod release.
3. Re-render `/etc/flux/equities*.env` from that release.
4. Confirm the target, Pulse metadata, and sudoers now enroll only `19` node units.
5. Stop all legacy `38` per-strategy node services and clear failed state.
6. Restart the equities stack in dependency order.
7. Verify:
   - all `38` strategy rows still appear in API/Fluxboard
   - node count is `19`
   - IBKR handshakes stop flapping
   - `balances.degraded` clears
   - signal stale-state rows clear or fall to genuine market-data issues only
   - grouped-node Pulse job mapping is correct for running-state and alerts
   - no stale legacy node unit remains active or restartable

Rollback is only safe as a full-stack atomic revert because node-scoped identity changes in this wave:

- repoint to the previous immutable release
- rerender envs
- stop grouped node units
- restart the stack on the previous per-strategy registry
- verify no grouped unit remains active or restartable

## Non-Goals

Do not include any of the following in this wave:

- second live gateway productionization
- new strategy arbitration between maker and taker
- strategy logic changes to entry/hedge behavior
- Fluxboard product redesign
- renaming the external `equities` surface
- replacing per-strategy TOMLs with a new permanent node-manifest format

## Testing Strategy

This refactor needs explicit contract coverage in five places:

1. Grouping helper tests
   - `38` enrolled strategy IDs -> `19` node groups
   - mixed one-member and two-member groups
   - stable node-group naming
2. Runner tests
   - one grouped node builds one shared `TradingNodeConfig`
   - two strategy instances are attached
   - shared venue clients are created once
   - node-scoped stream identity switches from external strategy ID to node-group ID
   - runtime params, projection feeds, and balances hooks remain strategy-attributed
   - same-node order ownership semantics stay correct for maker/taker siblings
3. API and Pulse mapping tests
   - `run_api.py` maps external strategy IDs to grouped node job ids for running-state and alerts
   - `signals`, `params`, and per-strategy selectors stay keyed by external strategy ID
   - `balances` keeps the existing profile-level payload and readiness semantics
   - `trades` / `alerts` keep external strategy attribution when rows are present
   - Pulse group membership and restart registry match grouped node ids
4. Installer tests
   - target renders `19` node units
   - per-strategy env files are not emitted
   - node env command lines include both strategy configs
   - stale per-strategy envs are removed and stale node units are explicitly retired
5. Production contract tests
   - API/readiness still expects and emits `38` strategy IDs across selectors and aggregate endpoints
   - docs and runbooks describe grouped nodes rather than one-process-per-strategy
   - deploy docs, strategy docs, checked-in target assets, and sudoers stay aligned
6. Controlled prod verification
   - `19` grouped node services active
   - `38` strategy rows on `signals` / `params`
   - `balances` remains profile-level and non-degraded
   - public `/equities` keeps realtime updates over the existing transport contract
   - no mixed old/new node services
   - readiness gate shows no `missing_required`, no stale grouped-node inventory components, and no pulse-backed mismatch

## Bottom Line

The current maker/taker split is externally correct but internally too expensive. The production-grade move now is not a second gateway dependency. It is to collapse each maker/taker pair into one node per symbol plus maker venue, keep the `38` strategy IDs intact, and make node topology an internal concern again.
