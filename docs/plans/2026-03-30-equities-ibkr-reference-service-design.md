# Equities IBKR Reference Service Rehome Design

**Date:** 2026-03-30

## Why This Exists

The reviewed equities market-data recovery V1 work fixed the grouped-node ownership bug for maker-feed recovery, but it deliberately left the shared IBKR reference boundary alone.

That boundary is now the next real blocker:

1. `~/chainsaw` is being retired and must not remain in the equities live path.
2. The live docs and restart order still depend on `chainsaw@md-ibkr-publisher.service`.
3. The checked-in equities node configs still build IBKR data clients directly, so the current repo does not yet enforce the intended "shared IBKR reference feed, node-local execution" boundary.
4. The prod incident on March 30, 2026 showed the operational weakness clearly: the Flux equities stack could be healthy enough to serve `/api/v1/signals?profile=equities`, but real reference prices were still absent when the external publisher path was down.

This follow-on design fixes that repo/runtime mismatch.

## Goal

Move the shared IBKR reference market-data boundary fully into `nautilus_trader` so equities live no longer depends on `~/chainsaw`, while preserving the reviewed grouped-node recovery V1 behavior and the existing external equities API/signal contract.

The immediate target is:

- one Flux-owned shared IBKR reference publisher service for equities
- grouped equities nodes consume that shared reference feed without opening their own IBKR market-data sessions
- IBKR execution stays node-local for now
- existing strategy ids, `/equities`, `/api/v1/signals?profile=equities`, and portfolio/account surfaces stay unchanged

## Non-Goals

This wave does **not**:

- build a generic multi-profile market-data platform
- redesign all venue/session ownership across the fleet
- collapse all equities strategies into one giant `TradingNode`
- move Hyperliquid or Binance into a shared venue service
- change the reviewed V1 maker-feed supervisor state machine
- redesign `makerv4` signal payloads or external equities strategy ids
- migrate IBKR execution into a separate shared hedge service

Longer-term generalized shared venue/session work is tracked separately in GitHub issue `#92`.

## Current Repo Reality

The relevant local surfaces are:

- `deploy/equities/README.md` and `docs/runbooks/equities-shared-node-cutover.md` still treat `chainsaw@md-ibkr-publisher.service` as an explicit precondition.
- `deploy/equities/equities.live.toml` already holds the canonical equities `[[strategy_contracts]]` and `[[account_scopes]]` tables.
- `systems/flux/flux/runners/live/venues.py` still resolves IBKR as a normal data+exec venue client.
- `systems/flux/flux/strategies/shared/equities_arb/core.py` rewrites IBKR execution settings for live equities, but it does not yet switch the IBKR data side to a shared feed adapter.
- `systems/flux/flux/strategies/makerv4/strategy.py` already models the reference feed as `scope = "ibkr.shared_publisher"` and `node_scoped_lifecycle = False`, which is the correct semantic direction for this migration.

So the architecture intent is already visible in the strategy/recovery layer, but the actual venue wiring and deploy contract are still split between Flux and Chainsaw.

## Options

### Option 1: Copy the Chainsaw publisher into this repo almost verbatim

This is the lowest migration effort, but it keeps the wrong abstractions:

- standalone INI config instead of `deploy/equities/equities.live.toml`
- Chainsaw-style Redis contract instead of Flux-owned shared keys
- no repo-native consumer path for grouped nodes
- no clean systemd/deploy-lane ownership inside the Flux installer

It would restore service ownership, but not the architecture.

### Option 2: Flux-owned publisher plus Flux-owned shared-reference data client

This is the recommended option.

Rehome the publisher as a first-class equities runner, derive the reference universe from the shared equities contract tables, publish a Flux-owned shared Redis contract, and add a repo-native shared IBKR reference market-data client so grouped nodes consume shared reference quotes without opening per-node IBKR market-data sessions.

This keeps the current grouped-node shape, keeps IBKR execution where it already lives, and finally makes the shared-reference boundary explicit in code.

### Option 3: Keep per-node IBKR data clients and only add a Flux-owned publisher for API/readiness

This would leave the core session-pressure problem unsolved. It restores some observability, but not the actual ownership boundary. It is not acceptable as the long-term repo direction.

## Recommendation

Implement Option 2.

The right long-term shape for this repo is:

- grouped nodes stay responsible for pair-local strategy logic, maker-feed recovery, and hedge execution
- IBKR reference market data becomes one shared Flux-owned service
- grouped nodes receive shared IBKR reference quote ticks through a repo-native shared-reference market-data client instead of direct IBKR market-data sessions

That is the smallest change that both retires Chainsaw and fixes the actual ownership boundary.

## Proposed Architecture

### 1. Flux-Owned Shared IBKR Reference Publisher

Add a new equities runner:

- `nautilus_trader.flux.runners.equities.run_ibkr_reference_publisher`

Back it with a shared implementation module under `systems/flux/flux/runners/shared/`.

Responsibilities:

- load `deploy/equities/equities.live.toml`
- derive the publish universe from `[[strategy_contracts]]`
- resolve the IBKR reference account scope from `[[account_scopes]]`
- connect once to the configured IBKR reference gateway/session
- subscribe to the unique set of `reference_instrument_id` values
- apply the existing session-aware SMART vs overnight feed selection logic
- publish shared reference snapshots plus explicit health/status

The publisher should not depend on `md_ibkr.ini` or any Chainsaw runtime helpers.

### 2. Flux-Owned Shared Redis Contract

The publisher writes profile-scoped shared keys, not Chainsaw `last:*` keys.

Add Flux key builders for:

- latest shared reference quote per `profile_id + account_scope_id + instrument_id`
- per-instrument pubsub/update channel
- shared IBKR reference service status for readiness and operators

This gives the repo one canonical shared-reference contract:

- publisher writes shared profile-scoped IBKR reference data
- grouped nodes subscribe to that shared stream
- strategies continue publishing their own strategy-scoped `market_last` rows through the existing bridge/API path after they ingest quote ticks locally

That preserves the current downstream API surface without forcing the shared service to write one copy per strategy.

### 3. Repo-Native Shared-Reference Market-Data Client

Add a new IBKR-specific data adapter mode for equities reference data, tentatively:

- adapter id: `interactive_brokers_shared_reference`

Behavior:

- data side: subscribe to the shared Redis contract emitted by the new publisher
- exec side: reuse the existing Interactive Brokers execution client/factory
- the client converts shared snapshot payloads into normal Nautilus `QuoteTick` objects and forwards them into `DataEngine.process`

This is the key migration seam. It preserves normal node-local `on_quote_tick` behavior while removing per-node IBKR market-data sessions.

In other words:

- strategies keep receiving quote ticks locally
- `MakerV4`'s shared supervisor semantics remain valid
- the actual IBKR reference market-data connection count drops to one shared publisher session

### 4. Equities Venue Resolution Rewrite

Update the equities venue rewrite logic so the live equities path uses the shared-reference adapter on the IBKR data side.

For equities:

- IBKR data should resolve to `interactive_brokers_shared_reference`
- IBKR execution should stay on the normal Interactive Brokers execution client
- Hyperliquid and Binance wiring stay unchanged

This can live in the existing equities-specific rewrite layer (`equities_arb.core` / `run_node.py`) so the change stays scoped to equities rather than silently affecting every IBKR consumer in the repo.

### 5. Shared Config, Not Sidecar INI

Publisher config belongs in `deploy/equities/equities.live.toml`.

Use the existing shared contract tables as the source of truth:

- `[[strategy_contracts]]` determines the reference instrument universe
- `[[account_scopes]]` determines host/port/client/account scope identity

Add one new top-level service table for publisher-specific knobs only, for example:

- enabled flag
- reference account scope id override if needed
- snapshot interval / stale-after thresholds
- reconnect/backoff tuning

That keeps one live contract file instead of another mutable sidecar config.

### 6. Readiness And Health

The publisher must emit explicit status, not just quotes.

Status should distinguish:

- starting
- connected
- publishing
- stale
- degraded
- down

Readiness should treat shared IBKR reference health as a first-class precondition again, but now through Flux-owned status keys rather than a foreign systemd unit name.

### 7. Failure Model

The publisher is fail-closed:

- if the IBKR session is not connected, shared status becomes degraded/down
- if an instrument loses freshness, that instrument status becomes stale and the shared service status degrades
- grouped nodes continue to receive the absence/staleness honestly through their local quote-aging logic
- the reviewed V1 pair-level tradeability gating and cancel-only behavior stay untouched

This migration must not bypass the V1 safety rails.

## Hardening Requirements

The new service must improve on the old publisher in these concrete ways:

1. Config comes from the shared equities deploy contract, not a separate INI.
2. Universe derivation is deterministic from `strategy_contracts`, with duplicates removed.
3. Publisher health is explicit in Redis for readiness and operator inspection.
4. Reconnect/backoff is bounded and logged; no tight reconnect loops.
5. Session-aware SMART vs overnight selection is preserved.
6. Shared reference snapshots carry enough metadata for consumer-side quote freshness and debugging.
7. No hidden dependency on `~/chainsaw`, `/etc/chainsaw/*`, or `chainsaw@.service`.

## Testing Strategy

The migration needs four test layers:

1. config/universe tests
   - shared config parsing
   - reference-scope resolution
   - deduped instrument universe derivation from `strategy_contracts`

2. publisher tests
   - session classification and feed selection
   - Redis key/channel contract
   - health/status transitions
   - reconnect/backoff behavior

3. shared-reference data-client tests
   - shared snapshot -> `QuoteTick` translation
   - per-instrument subscribe/unsubscribe semantics
   - no direct IBKR market-data session on grouped nodes

4. equities runner/readiness tests
   - IBKR data side resolves to the shared-reference adapter
   - IBKR exec side still resolves correctly
   - readiness fails when shared publisher health is stale/down
   - account/inventory surfaces do not regress

## Rollout Shape

This should ship as one consolidated equities integration PR built on the existing implementation branch that already contains `#88`, `#90`, and `#91`.

Deployment shape:

- create a pinned release root
- install the updated equities systemd contract from that release
- start the new Flux-owned shared IBKR reference publisher
- restart portfolio, bridge, grouped nodes, and API
- verify readiness and live signal freshness from the release lane

Rollback is straightforward:

- stop the new publisher service
- repoint to the previous release
- restart the prior equities stack

The release lane must never point live services at the dev repo or a worktree.

## Open Questions Resolved For This Wave

- Should this be generalized beyond equities now? No.
- Should we move IBKR execution into the shared service now? No.
- Should we preserve the old Chainsaw `md_ibkr.ini` contract? No.
- Should we preserve node-local quote delivery semantics? Yes.
- Should we change external equities strategy ids or `/equities`? No.

## Final Design

Ship one equities-only Flux-owned shared IBKR reference service and one equities-scoped shared-reference market-data adapter.

That gives the repo the architecture it already wants:

- one shared owner for IBKR reference market data
- grouped nodes still own trading logic and local recovery
- strategies still receive normal quote ticks
- account/inventory and public equities surfaces stay stable
- Chainsaw is removed from the runtime path
