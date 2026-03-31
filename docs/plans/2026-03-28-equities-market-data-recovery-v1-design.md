# Equities Market-Data Recovery V1 Design

**Date:** 2026-03-28

## External Review Context

This document is meant to be readable without prior chat history.

It follows the March 27 grouped-node rollout and the subsequent production incident where the public equities Signal page showed rows as live while many of those rows were in fact stale for minutes or tens of minutes.

The verified facts behind this design are:

1. This was not just a frontend cache issue. When the incident appeared, both the public `/equities` view and the direct equities signals API were serving stale strategy rows.
2. One-off inspection showed stale data on the producer side as well, so the failure was upstream of the API serializer.
3. Restarting one grouped node could make that node advance briefly, then flatten again, which strongly suggested quote-subscription/runtime stalls rather than a permanently dead static payload.
4. `MakerV3` already had timer-driven stale-market-data handling; `MakerV4` did not. A timer-based MakerV4 resubscribe patch was attempted as a stabilization measure, but live evidence showed that strategy-local retries alone were not sufficient.
5. Separate honesty regressions in `_payloads_signals.py` and Fluxboard were real and have already been fixed on this branch. Those fixes stop stale rows from pretending to be fresh, but they do not repair broken market-data recovery.
6. A temporary dual-IBKR-gateway topology and post-Docker 2FA churn were cleaned up during the investigation. That was an operational distraction, not the remaining core market-data architecture problem.

The current design therefore focuses on the unresolved issue: real market-data recovery under grouped nodes after the maker/taker split.

One external reviewer referenced the upstream public NautilusTrader project rather than only this fork. This revision adopts only the parts of that feedback that were verified locally in this branch, for example existing `ComponentState.degrade()` / `fault()` support and existing adapter reconnect-replay behavior. It does not treat upstream deployment boundaries or a new distributed market-data service as V1 requirements.

## System Context

An external reviewer should assume the following runtime model:

- the equities stack keeps the March 27 grouped-node topology
- one grouped `TradingNode` can host maker and taker siblings for the same strategy family
- strategies remain strategy-scoped in the external API and UI even when they share a node internally
- IBKR remains the shared reference/account side for equities live trading, and its reference market data already enters the equities stack through the shared `chainsaw@md-ibkr-publisher.service` boundary rather than via per-node maker-feed recovery loops
- maker-market-data recovery is the primary instability under review here, especially for the venue adapters already implicated in live failures
- the public `equities` contract and strategy ids are intentionally frozen for this V1 wave

## Review Questions

External review should focus on these questions:

1. Does the design create one clear recovery owner per node plus feed identity?
2. Does it fail closed for trading, not just for display/readiness?
3. Does it preserve strategy-local quote delivery while moving external lifecycle ownership out of the strategy?
4. Are the IBKR boundary and the Hyperliquid/Binance repair responsibilities assigned at the right layer?
5. Is the rollout plan safe enough for a live production cutover and rollback?

## Goal

Get the equities stack to a production-ready V1 trading state by making market-data recovery deterministic, honest, and scalable across grouped nodes, without changing the external `equities` API or `/equities` operator surface.

The immediate target is simple:

- real quote movement reaches strategy signals in real time
- stale feeds stop pretending to be healthy
- grouped maker/taker nodes recover one broken venue feed once, not twice
- recovery failure becomes an explicit fail-closed state rather than an infinite optimistic loop
- non-tradeable required feeds immediately force cancel-only behavior instead of leaving stale maker quotes working

This design is intentionally narrower than a full market-data platform rewrite. It fixes the ownership boundary that broke after the maker/taker split and shared-node rollout while keeping open a clean path to broader multi-market scaling later.

## Current Failure

The current failure mode is structural, not cosmetic:

1. Grouped nodes intentionally host maker and taker siblings in one `TradingNode`.
2. Each strategy still owns its own quote-liveness timer in `MakerV4Strategy.on_time_event`.
3. When a feed stalls, each sibling independently calls `unsubscribe_quote_ticks` then `subscribe_quote_ticks`.
4. The venue adapters for Hyperliquid and Binance are thin pass-throughs and do not provide an idempotent, stateful recovery contract.
5. Hyperliquid logs are emitting `Instrument not found in cache`, which means recovery is firing against invalid adapter/client state.
6. The result is repeated reset churn without restored quote flow.

The public honesty regressions on `/equities` and `_payloads_signals.py` were real and have already been fixed. What remains is the underlying market-data recovery path.

## Options

### Option 1: Keep strategy-owned retries and patch venue adapters until prod looks stable

This is the lowest immediate code churn, but it is not the right V1 design.

It preserves multiple recovery owners for one shared feed, keeps retry semantics hidden inside strategy objects, and makes every new venue fix ad hoc. It may recover one venue temporarily, but it leaves the grouped-node contract fundamentally unsafe.

### Option 2: Build a bounded node-scoped quote recovery layer and keep the rest of the stack unchanged

This is the recommended option.

Make quote subscription ownership node-scoped instead of strategy-scoped. Strategies still observe market data and publish state, but one shared recovery object per node owns stale-feed detection coordination, backoff, and reset side effects.

Venue-specific repair stays in the adapters where it belongs, but the retry policy and ownership boundary become consistent across all grouped equities nodes.

### Option 3: Introduce a standalone venue-level market-data daemon and make strategies consume a local normalized feed

This is directionally attractive for a larger multi-market platform, but it is too large for the current equities V1 objective. It would require a new deploy surface, new failure domains, and new data contracts at the exact moment prod needs a tighter fix, not a broader platform migration.

## Recommendation

Implement Option 2.

It is the smallest architecture change that actually fixes the current failure mode:

- one recovery owner per node plus feed identity
- no duplicate sibling resets
- explicit venue recovery semantics
- no external contract change
- clean extension path if we later move to per-venue or shared cross-node market-data services

## Design Principles

1. Strategies may detect staleness, but they do not own shared-feed side effects.
2. One node plus feed identity has one recovery state machine.
3. Adapter recovery must be idempotent and explicit.
4. Failed recovery must degrade to `recovering` or `down`, never to fake freshness.
5. The external `equities` contract stays strategy-scoped even if recovery becomes node-scoped.
6. V1 must be testable in unit/integration suites before the next prod rollout.
7. Non-tradeable required feeds force cancel-only behavior and quote pull, not just suppression of new actions.

## External Contract

These surfaces must remain unchanged:

- `/equities`
- `/api/v1/signals?profile=equities`
- `profile=equities`
- existing `38` external strategy ids
- Fluxboard selector and strategy-family behavior
- existing grouped-node runtime topology from the March 27 shared-node wave

This design changes the internal recovery contract only.

## V1 Architecture

### 1. Node-Scoped Quote Feed Supervisor

Introduce a shared recovery object, tentatively `NodeQuoteFeedSupervisor`, under `systems/flux/flux/runners/shared/`.

One supervisor instance exists per `TradingNode`. It tracks each quote-backed feed identity used by attached strategies and owns the recovery lifecycle for that node plus feed identity.

Its responsibilities are:

- maintain desired subscription state
- record last successful quote timestamp per feed identity
- track active strategy claimants for each feed identity
- compute feed health and tradeability using claimant budgets
- coalesce duplicate recovery requests from maker/taker siblings
- suppress per-feed reset churn when a shared venue/session blocker is active for the affected venue slice
- enforce backoff and recovery-attempt limits
- expose explicit feed states: `bootstrapping`, `healthy`, `stale`, `blocked`, `recovering`, `down`

The supervisor is not a strategy replacement. Strategies still hold their own local quote snapshots and continue making trading decisions from those snapshots. The supervisor only owns feed recovery coordination.

Feed identity must be more specific than `instrument_id` alone. For V1, the key should include the venue/provider scope or client slice, the `instrument_id`, and the quote source class or topic/channel used for that feed. Distinct feeds for the same instrument, for example maker vs reference or different quote-channel classes, must not alias into one state machine.

This is intentionally above the existing Nautilus data-engine subscription bookkeeping rather than a replacement for it. The current low-level subscription sets and message-bus subscriber checks already avoid some duplicate client actions, but the live grouped-node failure proves that those protections do not provide a safe recovery ownership boundary by themselves. The supervisor exists to prevent control-loop churn above that layer.

The supervisor must be the sole owner of lifecycle state:

- desired subscribed state
- current feed state
- attempt count
- backoff deadline
- last error summary

Venue adapters may own transport-local state, but they should only report transport facts and explicit reset outcomes back to the supervisor. They must not own the policy state that decides whether a feed may reset or transition to `down`.

Transport outcomes must re-enter the node/kernel thread before they mutate supervisor state. Async adapter tasks may emit facts, callbacks, or events through a runner-owned ingress path, but they must not mutate Python supervisor state directly from background runtimes.

### 1A. IBKR Boundary

V1 does not turn IBKR into another per-node venue-recovery loop.

In this stack, the IBKR reference side is already mediated by the shared `chainsaw@md-ibkr-publisher.service` boundary. This plan therefore treats IBKR as:

- a shared reference/session prerequisite for equities live trading
- an input to pair-level tradeability and session-aware blocking inside each node
- a rollout precondition that must already be healthy before maker-feed recovery is judged

If IBKR gateway, publisher, or session health is bad, the relevant reference feeds should become `blocked` and the rollout should stop. V1 does not add a second node-local recovery controller for the publisher path in this wave.

### 2. Runner Attachment Seam

The current equities runner only has explicit post-construction attachment hooks for runtime params, inventory feeds, and reference/projection providers. It does not yet have a seam for runner-owned quote recovery objects.

V1 must add that seam explicitly before the supervisor can be wired cleanly.

Required new attachment contract:

- the runner creates one shared supervisor per built node
- the runner injects that supervisor into each attached strategy through an explicit strategy attachment API
- the runner preserves or injects a strategy-local quote-topic attachment path so `on_quote_tick` delivery still works independently of supervisor-owned external feed lifecycle
- the runner also injects one canonical node-scoped quote-control emitter used for initial subscribe, recovery reset, and final unsubscribe side effects

The exact method names can be chosen during implementation, but the seam must be explicit. The design should not rely on constructor magic or on ad hoc mutation of strategy internals from `run_node.py`.

### 3. Strategy Contract

`MakerV4Strategy` stops directly unsubscribing and resubscribing quote feeds from its timer callback.

V1 must separate two concerns that are currently entangled in the strategy actor methods:

- strategy-local quote-topic attachment/detachment so each strategy continues receiving `on_quote_tick` callbacks and can maintain honest local snapshots
- supervisor-owned external venue lifecycle side effects for first subscribe, reset, and last unsubscribe

If the current actor helpers combine those behaviors, V1 must split them explicitly rather than accidentally dropping local quote delivery when external lifecycle ownership moves out of the strategy.

Instead, each strategy:

- preserves local quote-topic attachment/detachment needed for `on_quote_tick`
- registers its tracked feed identities and runtime freshness budgets with the shared supervisor during startup
- deregisters interest during shutdown
- reports new quote timestamps to the supervisor from `on_quote_tick`
- reports timer observations to the supervisor from `on_time_event`
- consumes supervisor-provided feed state for snapshot publication and internal gating

This preserves the existing per-strategy view of quote health while eliminating duplicate side effects.

For V1, the strategy timer can remain in place to minimize runner churn, but it becomes observer-only. The supervisor decides whether an initial subscribe, recovery action, or final unsubscribe is allowed and whether one is already in flight.

Because these budgets are strategy-local runtime params today, the supervisor must treat them as claimant-specific inputs, not as one shared static config blob.

Two separate decisions are required:

- local strategy unusable/tradeable state may use the strictest active claimant budget
- node-level reset admission must be feed-scoped and governed by its own minimum safe interval and backoff policy

That separation prevents one aggressive claimant from thrashing a shared feed later as the topology expands beyond the current maker/taker pair.

V1 observation may still enter the supervisor through strategy callbacks to minimize runner churn, but the supervisor must not depend on one specific sibling strategy to stay informed. Any live claimant must be able to advance the shared feed timestamp, and the test plan must prove that loss of one sibling does not blind node-level feed health.

Local strategy tradeability must require the required legs to be tradeable together, not merely individually healthy. V1 pair gating should require:

- all required feeds in the pair to be tradeable
- compatible session state for the required legs
- a bounded max age delta between the relevant legs so the pair is economically coherent

V1 should derive that gate from existing quote timestamps, feed states, and session inputs. A stronger recovery generation token or epoch model is a reasonable phase-2 extension, but not V1 scope.

### 4. Recovery State Machine

Each node-scoped feed identity follows this state machine:

- `bootstrapping`
  The node wants the feed, but the first valid quote has not arrived yet.
- `healthy`
  A fresh quote arrived within the configured budget.
- `stale`
  Quote age exceeded the stall budget, but no recovery is currently running.
- `blocked`
  A required precondition is missing, for example venue session/auth not ready, client cache not hydrated, shared publisher/reference health not ready, or market/session policy disallows live recovery.
- `recovering`
  A bounded reset attempt is in flight or the post-reset observation window is still open.
- `down`
  Recovery exceeded the configured attempt budget or adapter/client preconditions failed repeatedly.

State transitions:

1. `bootstrapping -> healthy` when the first valid quote arrives.
2. `bootstrapping -> blocked` when startup/session preconditions are not met.
3. `healthy -> stale` when the local freshness budget is exceeded.
4. `stale -> recovering` when the supervisor admits a reset.
5. `blocked -> recovering` only when the blocking precondition clears and a retry window opens.
6. `recovering -> healthy` only when a newer quote timestamp is observed.
7. `recovering -> down` after bounded failed attempts or explicit unrecoverable adapter error.
8. `down -> recovering` only after a later bounded retry window or explicit operator/node restart.

The important behavioral rule is fail-closed:

- `bootstrapping`, `blocked`, `recovering`, and `down` are not tradeable quote states
- required legs must be jointly tradeable as a pair, not only individually healthy
- API/UI surfaces must render them honestly
- readiness must count them as unhealthy
- strategy quote placement, quote amendment, hedge placement, and any live-quote-dependent execution path must be gated off while required feeds are not tradeable
- on transition into any non-tradeable required-feed condition, the strategy must enter cancel-only mode: pull working maker quotes, suppress new quote/amend/hedge actions, and keep cancel/reduce-only or emergency-exit paths available

The internal feed FSM should also use the existing component lifecycle rails already present in this fork. Sustained `blocked` or `recovering` conditions should degrade the owning strategy or runner component, and unrecoverable `down` or explicit fatal adapter errors should escalate through the existing fault path. V1 should reuse those lifecycle APIs rather than inventing a second operator-facing lifecycle model.

### 5. Recovery Execution Contract

The supervisor needs one canonical command emitter per node-scoped feed identity.

For V1, the runner should wire one shared reset emitter owned by the node build path, not by an arbitrary sibling strategy object. The key point is that only one shared owner may perform the side effect for a given node plus feed identity, and that owner should remain stable even if one strategy is later refactored or removed.

The lifecycle action sequence should be:

1. Check whether subscribe/reset/unsubscribe is already in flight or backoff is active.
2. If a shared venue/session blocker is active, remain `blocked` and suppress per-feed reset churn.
3. Apply the canonical lifecycle action using an escalation ladder:
   - startup subscribe when the first claimant registers
   - validate preconditions and attempt desired-subscription replay on a healthy transport first
   - reconnect/reset only when replay or validation does not restore the feed
   - shutdown unsubscribe when the last claimant deregisters
4. Prime cached quote state to stale locally.
5. Invoke venue-specific recovery logic.
6. Start a bounded observation window.
7. Mark `healthy` only after a newer quote arrives.

No direct `subscribe`, `unsubscribe`, or reset loop should run from both maker and taker strategies anymore.
Async adapter tasks must report reset results through the runner-owned ingress path so the supervisor state machine continues to mutate on the node/kernel thread only.

## Adapter Responsibilities

### Hyperliquid

Hyperliquid is the highest-priority venue because the live stale set is concentrated there and the logs already show cache failures.

V1 requirements for the actual Hyperliquid recovery path:

- `crates/adapters/hyperliquid/src/http/client.rs`
- `crates/adapters/hyperliquid/src/websocket/client.rs`
- the Python adapter wiring in `nautilus_trader/adapters/hyperliquid/data.py`

- quote resets must be idempotent per feed identity
- `subscribe_quotes` must fail explicitly when instrument cache is missing, not just warn
- recovery must attempt cache rehydrate before the next quote subscribe
- desired quote subscriptions must survive reconnect/reset paths
- recovery must attempt desired-subscription replay on a healthy transport before escalating to reconnect/reset churn
- repeated reset attempts must not create duplicate in-flight subscriptions

The design implication is that Hyperliquid recovery cannot remain a dumb `unsubscribe_quotes` plus `subscribe_quotes` pass-through.

### Binance

Binance has the same ownership problem even if the current evidence is less dramatic.

V1 requirements for the Binance recovery path:

- `nautilus_trader/adapters/binance/data.py`
- `nautilus_trader/adapters/binance/websocket/client.py`

- book-ticker quote subscription reset must be idempotent
- duplicate sibling resets must collapse into one effective reset
- desired subscription state must remain explicit during reset windows
- recovery must try desired-subscription replay on a healthy transport before escalating to reconnect/reset churn
- failure should surface as explicit recovery failure rather than silent churn

### Shared Adapter Rule

The set-based subscription bookkeeping in `nautilus_trader/data/client.pyx` is not enough by itself for grouped-node recovery. V1 needs explicit recovery semantics above the bare subscribed/unsubscribed set:

- desired state
- reset in flight
- last reset attempt
- last adapter error
- feed identity (`venue/provider scope`, `instrument_id`, `topic/channel class`)

The node-scoped supervisor must own the lifecycle policy state. Venue adapters may keep only the transport-local details required to execute a reset and report the reset outcome back to the supervisor.
The adapter remains the source of truth for transport replay and low-level subscription restoration; the supervisor must not become a second conflicting transport subscription table above it.

## Readiness, API, And Observability

The existing honest stale-state fixes stay in place.

V1 should add explicit recovery-state visibility to the internal payload/debug path so operators can distinguish:

- a passive old quote
- a startup feed waiting for first data
- a feed blocked on preconditions
- an actively recovering feed
- a feed that exhausted recovery and is down

This state must remain internal or debug-oriented unless already supported by the contract. The public equities payload should stay fail-closed without inventing new top-level quote-health enums in this wave. In practice that means:

- public-facing quote-health fields continue degrading to the existing stale/unusable semantics
- recovery-specific detail lives in internal/debug fields and logs
- readiness consumes the richer internal recovery state directly
- Fluxboard and API clients do not need a schema migration just to stop lying

Required operator outcomes:

- `/equities` never shows `good` for `recovering` or `down`
- readiness clearly counts `recovering` and `down` maker legs as unhealthy
- logs show one recovery owner and one attempt sequence per node plus feed identity
- we can answer "why is this row stale?" without tailing multiple sibling strategy logs
- supervisor state transitions emit structured logs with feed identity, node-group id, prior state, new state, attempt count, last adapter error, and any active venue/session blocker

## Implementation Scope

### In Scope For V1

- node-scoped quote recovery ownership
- MakerV4 integration with the supervisor
- pair-level tradeability gating and cancel-only quote pull
- Hyperliquid cache-aware recovery
- Binance idempotent quote reset handling
- explicit `recovering/down` fail-closed behavior
- regression tests for grouped-node recovery semantics
- integration with existing component degrade/fault lifecycle rails

### Out Of Scope For V1

- a standalone cross-node venue daemon
- rewriting the external equities API contract
- replacing grouped nodes with another topology
- a generic all-markets recovery platform across every strategy family
- redesigning `chainsaw@md-ibkr-publisher.service` or the shared IBKR publisher path in this wave

## Phase 2 Direction

This V1 is intentionally a node-scoped repair, not the final cross-node market-data architecture.

The longer-run direction is to move more venue/session ownership up toward shared ingest or normalized publisher layers and let nodes consume a stable downstream feed. The existing `chainsaw@md-ibkr-publisher.service` already points that way for the IBKR reference side. V1 does not generalize that pattern across every venue or node yet; it fixes the immediate grouped-sibling ownership bug first.

## Testing Strategy

V1 must be proven with tests before the next prod rollout.

Minimum required coverage:

1. Supervisor unit tests
   - duplicate maker/taker recovery requests coalesce
   - same-instrument feeds with different feed identities do not alias
   - strictest active claimant budget only affects local unusable/tradeable state
   - feed-scoped reset admission and backoff stay independent from claimant-local freshness budgets
   - node-local venue/session blockers suppress per-feed reset storms
   - one fresh quote clears `recovering`
   - repeated failed recovery transitions to `down`

2. Strategy integration tests
   - `MakerV4Strategy` no longer issues direct unsubscribe/subscribe in its timer path
   - stale detection still works
   - grouped siblings share one supervisor
   - pair-level tradeability requires all required legs plus bounded leg-age delta and session compatibility
   - non-tradeable required feeds force cancel-only behavior and pull working maker quotes

3. Adapter recovery tests
   - Hyperliquid cache-miss recovery path
   - Hyperliquid idempotent reset behavior
   - Binance idempotent reset behavior
   - recovery escalates from validation/replay to reconnect/reset instead of jumping straight to churn

4. Rust and boundary tests
   - Hyperliquid cache-miss path is exercised at the actual client layer
   - desired-subscription restore is preserved across reset/reconnect
   - Python wiring still exposes the repaired behavior cleanly
   - transport outcomes re-enter the node/kernel thread before mutating supervisor state

5. API/readiness tests
   - `recovering` never serializes as quote-usable
   - existing `down` behavior remains fail-closed
   - grouped-node ids do not leak into external payloads
   - pair-level disabled reasons stay explicit without inventing new public enums

6. Live validation
   - targeted stale rows on Hyperliquid and Binance advance after rollout
   - strategies with non-tradeable required feeds hold zero working maker quotes
   - readiness reaches `38/38` healthy under the current equities basket

## Rollout

Rollout should happen as a bounded production recovery wave:

1. confirm rollout preconditions:
   - IBKR gateway is authenticated
   - `chainsaw@md-ibkr-publisher.service` is healthy
   - the current stale baseline is captured from readiness plus direct signal probes
   - the validation window is an active US regular trading session for the targeted equities basket
2. land the redesign behind the existing grouped-node topology
3. run targeted unit/integration suites
4. cut an immutable release
5. restart only the equities stack
6. validate historically broken symbols first:
   - `aapl_tradexyz`
   - `amd_tradexyz`
   - `meta_tradexyz`
   - `msft_tradexyz`
   - `orcl_tradexyz`
   - `tsla_tradexyz`
   - `ewy_binance_perp`
7. hold the rollout until those rows show real quote movement over a 10-15 minute soak window and readiness reaches full health
8. require final go/no-go verification during a real US regular trading session so the production signoff is not based only on overnight or weekend behavior

Minimum rollout gates:

- capture a pre-restart baseline of stale rows and their quote ages
- capture post-restart ages for the historically bad rows at least twice over the soak window
- confirm supervisor logs show bounded state transitions rather than silent infinite retry churn
- confirm no strategy with non-tradeable required feeds retains working maker quotes
- rollback immediately if readiness regresses below the pre-rollout baseline or if the historically bad rows remain frozen after the soak window

## Acceptance Criteria

The V1 redesign is not done until all of the following are true:

- grouped-node siblings no longer issue duplicate direct feed resets
- per-feed identity separation prevents same-instrument feeds from aliasing into one recovery state machine
- local strategy tradeability requires the required legs to be jointly tradeable with bounded leg-age skew
- strategies with non-tradeable required feeds immediately pull maker quotes and remain cancel-only until tradeability is restored
- Hyperliquid and Binance quote-reset behavior is idempotent under repeated stale-feed observations
- Hyperliquid and Binance recovery escalates from validation/replay to reconnect/reset rather than straight to reset churn
- stale feeds transition to explicit `recovering/down` states instead of infinite optimistic loops
- external `equities` strategy ids and payload contracts remain unchanged
- account and inventory surfaces do not regress as a side effect of the feed recovery change
- sustained `blocked/recovering` conditions degrade and unrecoverable `down` conditions fault/escalate through the existing component lifecycle rails
- live readiness reaches `38/38` healthy during an active US regular trading session
- previously broken symbols advance real quote timestamps across repeated live probes

Rollback remains a full release-root revert. No mixed recovery ownership model should be run in prod.

## Why This Is The Right V1 Cut

This design does not pretend the current problem is one missing `if` statement. It also avoids the opposite mistake of turning an urgent production recovery into a multi-week platform rewrite.

It fixes the broken boundary:

- strategies stop fighting over shared feed recovery
- grouped nodes gain one accountable owner per feed
- adapters become responsible for real venue-specific reset semantics
- external operators keep the same surfaces they already know

It also stays honest about what V1 is not. The shared in-process Python supervisor is the right repair for the current multi-minute freeze incident in this fork. It is not the claimed end-state cross-node HFT market-data architecture, and the document now states that explicitly.

That is the most reasonable path to a stable prod trading V1 while still setting up a cleaner scaling story for more markets later.
