# Account-Scoped Execution Controller Design

**Date:** 2026-03-28

## Goal

Define the long-term production architecture for shared-account execution across
TokenMM and equities so that:

- one economic writer domain has one canonical execution owner
- pricing strategies keep low-latency local decision-making
- shared balances, readiness, and portfolio truth stop depending on post-hoc
  repair of conflicting strategy snapshots
- the system scales to many venues, many subaccounts, and eventually many boxes
  without duplicate writers or ambiguous reconciliation ownership

## Executive Summary

The current architecture already has the right logical metadata but the wrong
runtime ownership boundary.

Today:

- `strategy_id` owns most runtime order and balance state
- `execution_account_scope_id` is only a logical contract tag
- startup reconciliation is still node-local in Nautilus
- shared-account projections are profile-owned but read-side only
- the portfolio runner merges strategy snapshots after the fact

That split is the root cause behind the repeated shared-account reconciliation
failures. We are trying to infer one canonical account truth from multiple
strategy-local truths after venue state has already diverged.

The chosen design is:

- keep `strategy_id` as the lifecycle and attribution identity
- keep `account_scope_id` as the logical capital/provenance identity
- add `controller_scope_id` as the exclusive physical writer identity
- introduce one leased local execution controller per `controller_scope_id`
- keep pricing strategies separate, but convert them from execution owners into
  intent publishers plus canonical-state consumers
- preserve the portfolio/API plane as a read-side consumer, not an execution
  owner

For the first production wave, the controller path is intentionally narrowed:

- `controller_scope_id` is manually enumerated, not auto-derived
- the first canary is `equities / ibkr.hedge.main`
- active cutover is single-host and no-standby only
- controller canonical ownership uses a synchronous local ownership WAL plus
  asynchronous projections/materializations
- multi-box failover remains blocked until replicated ownership logging and
  split-brain drills are proven
- the first executable package stops at one active `equities / ibkr.hedge.main`
  canary plus one-bounce rollback; read-side authority, multi-box durability,
  and TokenMM migration remain follow-on approval packages

## Glossary And Assumptions

- `TokenMM`: the crypto market-making profile under `systems/flux/flux/runners/tokenmm`
- `equities`: the equities profile under `systems/flux/flux/runners/equities`
- `MakerV3` / `MakerV4`: the existing strategy families that currently own most
  local execution-adjacent state
- `external-client seam`: Nautilus execution-engine support for external client
  IDs and external order claims
- `shadow mode`: controller observes, journals, and compares truth but does not
  own live venue writes
- `shared-account venue class`: multiple strategy routes touching one economic
  account or margin pool
- `controller-managed lane`: a strategy route whose venue writes and startup
  reconciliation are owned by the controller rather than by the strategy-local
  execution engine

Important assumption for this design:

- strategy workers remain separate processes for pricing/market-data logic
- controllers own execution truth
- portfolio/API remain asynchronous read-side consumers

## Current-State Architecture

### Execution ownership today

Both TokenMM and equities hand execution configuration into the shared Nautilus
live execution engine. Startup reconciliation and runtime order/fill/position
reconciliation happen in the node-local `ExecutionEngine`, not in shared
profile services.

Relevant code:

- `systems/flux/flux/runners/tokenmm/run_node.py`
- `systems/flux/flux/runners/equities/run_node.py`
- `nautilus_trader/live/execution_engine.py`
- `nautilus_trader/execution/engine.pyx`

### Balance and inventory ownership today

Strategy-local publication is still the main source of truth for balances and
inventory:

- MakerV3 and MakerV4 publish per-strategy balances
- strategies publish per-strategy portfolio inventory components
- the shared portfolio runner consumes those snapshots and merges them into
  profile-level views

Relevant code:

- `systems/flux/flux/strategies/makerv3/strategy.py`
- `systems/flux/flux/strategies/makerv4/strategy.py`
- `systems/flux/flux/strategies/makerv3/publisher.py`
- `systems/flux/flux/runners/shared/portfolio_runner.py`
- `systems/flux/flux/common/portfolio_snapshot.py`

### Shared-account ownership today

Profile-owned shared-account projection already exists, but it is a control-plane
read model:

- providers are built from `strategy_contracts` plus `account_scopes`
- providers poll venue/broker account state and publish account snapshots
- strategies consume those projections only as fallback reference data
- the portfolio runner carries those rows into `snapshot.accounts`

Relevant code:

- `systems/flux/flux/common/account_scopes.py`
- `systems/flux/flux/common/strategy_contracts.py`
- `systems/flux/flux/runners/shared/profile_accounts.py`
- `systems/flux/flux/common/account_projection.py`

## Concrete Failure Walkthrough

The current design problem is easiest to see in the recent shared Binance PM
incidents.

1. A TokenMM Binance strategy restarts with stale local cached execution state.
   One node believes it owns a non-flat position and stale open orders.
2. Venue truth is already simple and shared:
   - shared collateral is one Binance PM cash state
   - venue position is flat or otherwise different from the stale local cache
3. A second strategy on the same economic account publishes its own
   strategy-local balance snapshot, using different local account/route metadata.
4. The profile-owned shared-account projector separately polls Binance and emits
   one shared-account view of the same economic account.
5. The portfolio/API plane merges these sources after the fact:
   - strategy-local balances still disagree
   - shared-account cash may appear as duplicate rows
   - startup reconciliation still happens inside the restarting strategy node
6. The operator now sees two classes of failure at once:
   - startup fail-closed because node-local cache truth disagrees with venue
   - portfolio/API ambiguity because one economic account is represented by more
     than one local truth source

Existing metadata does not solve this because `execution_account_scope_id` is
only a contract tag. No process actually owns shared account truth at runtime.

## Constraints And Non-Negotiables

The target design must satisfy all of the following:

- world-class HFT latency on the hot path
- support for many venues and many subaccounts
- support for shared accounts and dedicated accounts
- ability to run across multiple boxes and future co-located deployments
- fail-closed execution safety
- alignment with Nautilus Trader runtime boundaries
- external-review-friendly documentation and rollout plan

Non-goals for the first migration waves:

- rewriting Nautilus around `account_scope_id`
- patching Nautilus core before the external-client / adapter seam proves a
  specific invariant gap
- moving portfolio/API onto the synchronous order path
- replacing all existing strategy-local inventory publication on day one
- enabling multi-box active/standby before replicated ownership logging exists

## Fixed Decisions For V1

These decisions are no longer open for the first implementation wave:

1. Physical writer-domain mapping
   - `controller_scope_id` is manually enumerated in manifests for v1.
   - No automatic derivation is attempted beyond validation helpers.
2. First canary venue class
   - `equities / ibkr.hedge.main`
3. First active deployment shape
   - single host
   - no standby writer
   - lease/fencing still enforced locally and in tests
4. First ownership log
   - synchronous local SQLite WAL per controller scope
   - asynchronous materialized state and read-side projections
5. First same-host transport candidate
   - Unix domain socket transport for controller intents/events
   - no Redis or HTTP on the synchronous write path
6. First Nautilus integration seam
   - start at the external-client / adapter seam
   - only patch `nautilus_trader/execution/engine.pyx` or
     `nautilus_trader/live/execution_engine.py` after an explicit invariant gap
     is demonstrated in this repo
   - public Nautilus issue history is a risk signal, not a substitute for proof
     in this fork

## Approaches Considered

| Approach | Latency | Migration Risk | Nautilus Divergence | Multi-Box Safety | Decision |
| --- | --- | --- | --- | --- | --- |
| More strategies on one node | good | low | low | poor | reject |
| Reconciliation-only hardening in current nodes | good | medium | low | poor | reject |
| Remote centralized controller service | poor | high | medium | good | reject |
| Leased local controller per writer domain | good | medium | low-medium | good | choose |

### 1. Put more strategies on the same node

This reduces some duplication, but it does not change the runtime ownership
boundary. Reconciliation still lives in strategy/node-local execution engines,
and shared-account truth still has to be inferred later.

### 2. Reconciliation-only hardening

This improves specific failure shapes, but it still leaves strategy-local cache
ownership, strategy-published balances, and post-hoc shared-account repair as
the system boundary.

### 3. Remote centralized controller service

This gives clearer ownership, but it adds a remote synchronous hop into the hot
path and pushes too much latency-sensitive behavior into a large central
failure-domain.

### 4. Leased local execution controllers per writer domain

This creates one canonical writer per physical writer domain while preserving
local low-latency pricing and a clean multi-box ownership story.

## Chosen Design

### Identity model

The runtime uses three distinct identities:

- `strategy_id`
  Lifecycle, attribution, PnL, cooldowns, strategy-local diagnostics, and
  operator-facing ownership.
- `account_scope_id`
  Logical capital/provenance domain. This ties strategies to shared balances,
  reference accounts, hedge accounts, and API surfaces.
- `controller_scope_id`
  Exclusive physical writer domain. This means “exactly one active controller
  process may write to this venue account or subaccount at a time.”

Important rule:

`account_scope_id` and `controller_scope_id` are related but not identical.
Multiple logical scopes can point at one physical writer domain, and a
controller may publish multiple logical read-side account views.

### Component model

#### 1. Strategy Worker

Owns:

- market data
- pricing and quoting logic
- alpha state
- local shadow view of order/exposure state
- strategy-level preferences and reservations

Does not own:

- canonical venue order IDs
- canonical open-order state
- startup reconciliation
- orphan/external repair
- canonical shared-account balances

#### 2. Account Execution Controller

Owns one `controller_scope_id`.

Owns:

- lease/fencing ownership
- venue session lifecycle
- order submission/cancel/replace sequencing
- startup reconciliation and recovery
- canonical account order/fill/position state
- rate limits, throttles, kill switches, and venue protections
- external/orphan claim handling
- strategy-attributed fill/exposure allocation

#### 3. Intent And Order Lifecycle Contract

Controller-managed lanes use one explicit lifecycle so strategy shadow state,
controller canonical state, and read-side snapshots cannot invent different
meanings for the same order.

Required states:

- `published`
  - strategy emitted an intent, but the controller has not accepted ownership
- `accepted`
  - controller accepted the intent and assigned `controller_epoch`,
    `controller_seq`, and generated `client_order_id`
- `owned_pre_write`
  - ownership WAL append succeeded, but the venue write has not yet been
    attempted
- `rejected`
  - controller refused ownership before any venue write
- `sent_to_venue`
  - the venue write was attempted after `owned_pre_write`
- `working`
  - venue acknowledges a live working order
- `partially_filled`
  - venue or recovery path confirms a non-terminal partial fill
- `filled`
  - terminal fill state
- `canceled`
  - terminal cancel/expire/reject state
- `quarantined`
  - controller found venue/account activity that cannot yet be claimed safely

The lifecycle chain that must remain deterministic on managed lanes is:

`intent_id -> controller_epoch -> controller_seq -> client_order_id -> venue_order_id`

No strategy may claim or mutate canonical ownership once an intent has reached
`accepted`. Strategy workers may keep a one-callback-late local shadow view, but
they do not redefine the lifecycle.

#### 4. Canonical Ownership WAL

This is the minimal synchronous ownership record and is the real canonical layer
for recovery.

Before a venue write is considered owned, the controller must durably append a
record containing at least:

- `intent_id`
- `strategy_id`
- `account_scope_id`
- `controller_scope_id`
- `controller_epoch`
- controller-local sequence number
- operation type
- generated `client_order_id`
- idempotency key / venue-order claim key
- creation timestamp

If the WAL append fails, the venue write must not happen.

The v1 durability contract is:

- SQLite `journal_mode=WAL`
- SQLite `synchronous=FULL`
- one transaction per owned venue write
- fence/epoch validation happens before append and again before venue write

Each managed-lane adapter must define its own claim tuple and replay table
before activation. For the first active canary, that means `ibkr.hedge.main`
must prove how the controller binds:

- `intent_id`
- `controller_epoch`
- `controller_seq`
- generated `client_order_id`
- venue-native order identifier when available

Replay is intentionally conservative:

- if the WAL shows `owned_pre_write` and no venue evidence exists yet, restart
  remains in a pre-write recovery state and re-validates before any first send
- if the WAL shows `sent_to_venue` but no venue evidence exists yet, restart
  remains in pending-recovery and re-queries before any resend
- if venue truth matches the claim tuple, the controller binds and advances the
  lifecycle without reissuing ownership
- if venue truth is terminal but the controller never observed the final ack,
  the controller materializes the terminal lifecycle from venue truth
- if venue truth exists with no matching claim tuple, the activity is not
  canonical and moves to `quarantined`

#### 5. Materialized Ledger And Snapshots

These are asynchronous and derived:

- human/operator-readable order lifecycle
- fill and attribution history
- positions and balances
- reconciliation events
- controller health/state snapshots

They are durable and important, but they are not the synchronous ownership
boundary.

Every controller-published order, position, and balance snapshot must carry:

- `controller_scope_id`
- `controller_epoch`
- highest included `controller_seq`
- snapshot publication timestamp
- freshness deadline / stale marker
- authority state: `legacy`, `shadow`, or `controller`

These fields exist so the read side can reject stale controller outputs and, in
coexistence windows, deterministically reject stale legacy rows rather than
merging ambiguous truth.

#### 6. Portfolio / Risk / API Plane

Consumes controller/account state and publishes:

- portfolio inventory
- shared balances
- readiness
- operator surfaces
- cross-profile and cross-box aggregates

This layer remains read-side.

## Hot Path

The synchronous write path is:

`strategy worker -> local controller -> venue`

Not:

`strategy worker -> Redis/API/portfolio service -> venue`

The hot path must remain same-host and low overhead:

- Unix domain sockets for the v1 intent/event transport
- no remote Redis dependency for every order
- no HTTP hop on submit/cancel/replace
- no global portfolio service dependency on the write path

### Provisional latency budgets

The first controller canary must meet all of the following relative to the
current direct path on the same host:

- additional submit/cancel/replace overhead p50 `<= 100us`
- additional submit/cancel/replace overhead p99 `<= 750us`
- queue backlog age p99 `<= 2ms` under defined canary burst load
- zero dropped intents

If the canary cannot meet those budgets, the write-owner cutover is blocked.

## Recovery Contract

The controller may call state canonical only if recovery is precise.

### Crash windows

1. WAL append succeeds, venue write not attempted
   - lifecycle remains `owned_pre_write`
   - safe to replay and either issue or cancel intent on restart
2. WAL append succeeds, venue write happens, async projections lag
   - lifecycle remains `sent_to_venue` until venue truth resolves it
   - safe to recover from WAL + venue truth
3. Venue shows a write with no WAL ownership record
   - not canonical
   - classify as `external` / `manual` / `orphan`
   - quarantine until explicit attribution or cleanup

### External / manual activity policy

The first active canary uses a strict policy:

| Activity Shape | Default Controller Action | Operator Options |
| --- | --- | --- |
| venue activity matches claim tuple | bind and continue lifecycle | none needed |
| venue activity has no claim tuple but is clearly manual/external | `quarantined` | explicit operator claim, controlled reconcile, or controlled flatten |
| venue activity is ambiguous/orphaned | `quarantined` | operator claim after audit only |
| liquidation/forced venue action | `quarantined` plus degraded readiness | controlled reconcile or flatten |

The v1 canary does not auto-flatten or auto-claim ambiguous external activity.
That policy is safer than hidden repair and is narrow enough for one writer
canary.

### What is allowed to lag

- operator projections
- portfolio snapshots
- derived materializations
- analytics/telemetry enrichment

### What is not allowed to lag

- ownership WAL append for new venue writes
- lease/fencing state required to validate writes
- idempotency/claim data required for restart recovery

## Nautilus / Controller Ownership Matrix

For controller-managed lanes:

| Responsibility | Owner | Notes |
| --- | --- | --- |
| Venue session lifecycle | controller | Nautilus is used as the execution seam, not the ownership boundary |
| Startup reconciliation | controller | strategy-local reconciliation disabled/bypassed for managed lanes |
| Canonical open-order truth | controller | Nautilus order objects remain strategy-scoped outputs |
| External/orphan order claim policy | controller | one source of truth for claim registration on managed lanes |
| Rate limits and throttles | controller | per-writer-domain protections |
| Strategy callbacks / lifecycle semantics | Nautilus | preserve strategy-scoped events and lifecycle |
| Risk-engine command/event routing | Nautilus + controller adapter | controller-managed write path must remain additive rather than a full core rewrite |
| Portfolio/API snapshots | read-side plane | consumes controller-owned account truth |

The implementation must prove that each row above has exactly one effective
owner on controller-managed lanes.

## Transition-State Matrix

During coexistence:

| Surface | Authoritative Source | Allowed Drift | Alarm Condition | Retirement Trigger |
| --- | --- | --- | --- | --- |
| Strategy local order/exposure shadow state | controller callbacks mirrored into strategies | local shadow may lag one callback cycle | strategy shadow differs from controller canonical state past grace window | controller callbacks proven stable |
| Shared-account balances | controller account state | none | any controller vs legacy shared-account balance mismatch | read-side cutover complete |
| Portfolio/API balances | legacy read-side until canary authority switch | temporary compatibility overlap allowed only when controller sequence/freshness ordering says the controller row is stale | controller and legacy rows disagree without explicit degraded flag or stale-sequence explanation | follow-on read-side package complete |
| Readiness | legacy + controller health side by side | degraded flags allowed | controller stale without surfaced readiness degradation | read-side cutover complete |

## Deployment And Failover Model

### Placement

- strategies and their primary controllers should usually be co-located on the
  same box
- controllers remain small failure domains
- multi-venue firms can run many controllers on one host, but each controller
  still owns one physical writer domain

### Single-writer invariant

For each `controller_scope_id`:

- many readers allowed
- many strategy workers allowed
- exactly one active writer allowed

### V1 cutover constraint

Before replicated ownership logging and split-brain drills exist, active
controller cutovers are limited to:

- single host
- no standby writer
- explicit operator rollback switch

### Multi-box failover target state

1. standby acquires lease
2. standby replays ownership WAL and materialized state
3. standby queries venue truth
4. standby reconciles account state
5. standby enables writes only after reconciliation completes

### Production failover budgets

Before multi-box rollout:

- stale writer must stop new venue writes within `250ms` of lease loss or before
  the next outbound write, whichever is sooner
- split-brain drill must show `0` duplicate writes
- failover drill must show `0` ambiguous order owners after reconciliation

## Migration Waves

### Wave 0: Topology and recovery decisions

- manual writer-domain mapping rules
- canary scope lock
- synchronous ownership WAL contract
- transport choice and latency budgets

### Wave 1: Shadow controllers and safety harnesses

- shadow-mode controller runner
- ownership WAL + replay
- lease/fencing tests
- latency benchmarks
- failover/split-brain drills
- shadow parity against the legacy path

### Wave 2: Single-host active-writer canary

- `equities / ibkr.hedge.main`
- controller owns venue writes and startup reconciliation
- legacy read-side compatibility outputs remain enabled
- no multi-box standby yet

## Execution Package Boundary

The first executable package ends at Wave 2:

- shadow controller in production shape
- one active `equities / ibkr.hedge.main` writer canary
- one-bounce rollback back to legacy ownership

Inside that package there is an internal review boundary:

- platform cut: lifecycle contract, mapping, transport, WAL/fencing, shadow
  controller, and adapter-only managed-lane proof
- canary cut: attribution/fencing gates, canary strategy conversion, and one
  active writer canary

Strategy behavior changes and live writer activation do not start until the
platform cut is reviewed green.

Wave 3 and beyond are intentionally separate approval packages:

- Wave 3: read-side authority switch
- Wave 4: replicated ownership logging and multi-box / co-lo hardening
- Wave 5: broader controller-managed venue expansion, including TokenMM after
  the multi-box gates pass

### Wave 3: Read-side authority switch for the canary

- portfolio/API/readiness consume controller-owned account truth as primary
- legacy repair paths remain only as explicit fallback/degraded behavior

### Wave 4: Multi-box / co-lo hardening

- replicated ownership logging
- split-brain-safe failover
- stale-writer rejection under partitions
- production multi-box rollout

### Wave 5: Controller-managed shared-account expansion

- more shared-account venue classes after the canary and multi-box gates pass
- fast crypto writer domains only after canary latency proof
- TokenMM shared Binance writer domains only after the multi-box wave completes

## Exit Criteria Per Wave

### Before Wave 2

- writer-domain mapping validated
- ownership WAL replay proven across crash drills
- shadow parity shows `0` unexplained order/fill/position diffs across five
  controlled restart drills and one full shadow canary session
- latency budgets pass
- lease-loss and split-brain drills pass in the single-host harness

### Before Wave 3

- active-writer canary runs one full session with no duplicate writers
- one recorded canary-session artifact shows `0` unexplained ownership diffs
- no unexplained controller vs legacy balance divergence
- rollback can restore legacy ownership in one bounce

### Before Wave 5

- replicated ownership log chosen and implemented
- multi-box split-brain drills show `0` duplicate writes
- failover drill shows no ambiguous ownership after recovery

## Risks And Remaining Open Questions

### 1. Replicated ownership log for multi-box rollout

The single-host canary will use local SQLite WAL. Multi-box production still
needs a replicated ownership log or equivalent durability guarantee.

### 2. UDS transport sufficiency for fast crypto paths

The first canary uses UDS. Fast crypto maker paths may require a different
same-host transport or controller colocation model if the latency budgets fail.

### 3. Broader external/manual activity policy after the canary

The canary defaults to quarantine-first behavior. A broader production policy
for auto-claim, controlled reconcile, or controlled flatten across venue classes
still needs explicit approval before wider rollout.

## External Review Checklist

Any external architecture review should explicitly answer:

1. Is the three-identity split sufficient and well-defined?
2. Is the ownership WAL strong enough for restart correctness?
3. Is the Nautilus/controller ownership matrix crisp enough to avoid split
   ownership inside one node?
4. Are the latency and failover gates strong enough before active cutover?
5. Is the first execution package narrow enough to stop at one active canary?
6. Are the later read-side / multi-box / TokenMM waves correctly separated from
   the canary package?

## Recommendation

Proceed with the controller architecture, but implement it incrementally and
prove it first on one shared-account canary. Do not expand the current
portfolio/account-projection plane into an execution owner, and do not attempt a
flag-day rewrite of Nautilus or strategy identity semantics.
