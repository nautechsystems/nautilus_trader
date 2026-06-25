# Betfair Integration Tests

End-to-end integration tests for the Betfair execution adapter. Each scenario:

- drives a real strategy (or a direct command) through the real `RiskEngine` and `ExecutionEngine`
  into a real `BetfairExecutionClient` pointed at the in-process mock venue
- routes the client's emitted events back through the real `AsyncRunner` routing fork into a real
  `Cache`
- asserts a reusable set of cross-layer invariants

The load-bearing invariant is the routing contract: a tracked happy-path order reaches the engine via
direct order events (`ExecutionEvent::Order`), never reconciliation reports (`ExecutionEvent::Report`).
A report for a tracked order routes it to reconciliation instead of the state machine; the contract is
documented in `docs/developer_guide/adapters.md`.

## Layout

- `live.rs`: the seam test target, one test per scenario.
- `node.rs`: the full-node smoke target (phase 2), booting a real `LiveNode` against the mock
  venue (see [Full-node smoke](#full-node-smoke) below).
- `harness/mod.rs`: the reusable harness, made up of:
  - `Harness::build`
  - the `submit_via_risk` / `modify_via_risk` command drivers
  - the `reconcile_from_venue` HTTP reconcile driver
  - `override_betting_result` and `mark_pending_cancel` for scenario setup
  - the drain-and-route pump
  - the `StreamFeeder`
  - the `invariants` module
  - the order/quote builders

  This module is the unit to extract into a shared `nautilus-live-testkit` crate once a second
  adapter adopts the pattern.
- `../test_data/stream/ocm_harness_*.json`: matched OCM frames (cancel, fill, partial fill, external).

## Why two targets

The two layers are complementary: each asserts something the other structurally cannot.

- `live.rs` (seam) is the sharp, deterministic regression net. It taps each `ExecutionEvent` before the
  routing fork consumes it, so it can assert the routing contract (order events, never reports) that a
  node test can't observe (the `AsyncRunner` owns that channel internally). A `TestClock` makes ordering
  exact, so it covers the edge cases (modify, reconcile, venue errors) cheaply.
- `node.rs` (full-node) is the production-fidelity check. It boots a real `LiveNode` (builder ->
  `ExecutionClientFactory` -> connect -> run -> stop), exercising the `ExecutionManager` bookkeeping and
  real assembly the seam hand-wires past. Wall-clock, so it stays a thin smoke.

A seam failure localizes to the fork; a node failure points at assembly or the run loop.

## Running

```bash
cargo nextest run -p nautilus-betfair --test live
cargo nextest run -p nautilus-betfair --test node
```

nextest runs each test in its own process, which isolates the thread-local message bus and logging
that the engines rely on. The seam harness also tolerates multiple builds on one thread (it installs
a fresh bus per build and uses the replace-style sender setters), so `cargo test -- --test-threads=1`
works too. The `node` target boots a real `LiveNode` (one global logger per process), so prefer
nextest for it.

## Flow

```
ExecTester.on_quote / submit_via_risk -> RiskEngine -> ExecutionEngine
  -> BetfairExecutionClient.submit_order -> HTTP -> mock venue
StreamFeeder.feed(frame) -> client parses OCM -> ExecutionEvent on the test channel
  -> pump_until: AsyncRunner::handle_exec_event (the real fork)
       Order  -> ExecEngine.process        (state machine)
       Report -> ExecEngine.reconcile_...   (reconciliation)
  -> Cache updated -> invariants asserted
```

`Harness::build`:

- installs a bus with a known trader id
- seeds the `betting()` instrument
- wires both engines with `manage_own_order_books` enabled
- connects the client against the mocks

`pump_until` drains the client's event channel, tags each event (`RoutedKind`) for the routing
assertion, and routes it through the real fork until a cache predicate holds.

## Invariants

- `assert_tracked_used_events`: the routing contract, no report on a tracked happy path.
- `assert_order_status`: the order reaches the expected state.
- `assert_own_book_consistent`: no closed order lingers in the own order book.
- `assert_filled_qty`: cumulative filled quantity matches.
- `assert_in_own_book`: an order is present or absent in the own order book as expected.

## Adding a scenario

Scenarios drive the venue one of three ways:

- an OCM stream frame (`StreamFeeder.feed`)
- an HTTP modify (`modify_via_risk` -> `replaceOrders` for a price change, `cancelOrders` for a
  quantity reduction, both emitting `OrderUpdated`)
- an HTTP reconcile (`reconcile_from_venue` -> `generate_order_status_reports` over
  `listCurrentOrders`, then `reconcile_execution_mass_status`)

Use `override_betting_result` to point a betting method at a fixture: a place-order error to assert
`OrderRejected`, or a `listCurrentOrders` snapshot for reconciliation. The reconcile snapshot
correlates by `customerOrderRef` (the order's client id) and `betId` (its venue order id). For an
OCM-stream scenario:

1. Author a matched OCM frame under `../test_data/stream/`. Correlation is by `uo.rfo` (the truncated
   client order id) and the `oc.id` / `orc.id` market and selection that rebuild the instrument id.
   The terminal state is encoded as:
   - cancel: `status: "EC"` with `sm: 0, sc > 0`
   - full fill: `status: "EC"` with `sm > 0, sc: 0`
   - partial fill: `status: "E"` with `0 < sm < size`
2. Submit a known order via `submit_via_risk` (client order id `"O-1"`), then pump to `Accepted`.
3. `feeder.feed(...)` the frame, pump to the terminal state, assert the invariants.

## Reusing for another adapter

Supply the adapter's own mock venue and matched frames; reuse the engine wiring, the pump, and the
invariants from `harness/mod.rs`. ExecTester registers against any adapter via the `Strategy` trait's
`core_mut`, configured to the adapter's instrument and client id.

## Routing-contract proof

The routing-contract assertion catches a report-for-tracked-order regression. Forcing the Betfair tracked path
to emit a report (set the `tracked` binding in `execution.rs` to `None`) makes
`tracked_cancel_emits_event_and_shrinks_own_book` fail with:

```
tracked happy path routed 1 report(s), expected 0 (routing-contract violation): [Account, Order, Order, Report]
```

The order still reaches `Canceled` via reconciliation (book size is double-guarded, since the
`PendingCancel` defer was also removed), so the channel-level routing assertion, not the book size, is
the sensitive guard.

## Full-node smoke

`node.rs` is the phase 2 counterpart to the seam harness. It boots a real `LiveNode` via the
builder, registers a real `BetfairExecutionClient` through an `ExecutionClientFactory`, and runs the
node's event loop against the same mock venue. Where the seam harness drives the routing fork
directly on a `TestClock` with manual pumping, this exercises the same fork wrapped in the
`ExecutionManager` bookkeeping that `LiveNode::run` adds (fill-dedup, post-dispatch close handling),
at the cost of a wall-clock run loop.

The factory (`MockBetfairExecFactory`) mirrors `BetfairExecutionClientFactory::create` but injects
the mock HTTP and stream endpoints, because `BetfairExecConfig` has no HTTP base-URL override to
point at the mock. Everything downstream of the factory (the client, both engines, the run loop, the
routing fork, the `ExecutionManager`) is the production path. A minimal `SubmitLimitOnStart` strategy
submits one passive limit order on start and records each terminal order event into a shared
`LifecycleProbe` (`Arc<AtomicBool>` flags), giving the driver task a deterministic signal without
polling the cache (which is not `Send`). The five scenarios exercise the core lifecycle through the
node:

- boot -> connect -> stop clean
- submit -> `Accepted`
- a venue place-order error -> `Rejected`
- a strategy-issued cancel (confirmed by an OCM frame) -> `Canceled`
- a fed OCM fill -> `Filled`

Reconciliation is disabled so startup is deterministic and focused on the order lifecycle.

The `node` feature on `nautilus-live` (which pulls the builder/config/node modules) is enabled for
the betfair test build via a `[dev-dependencies]` feature bump, so the target runs under the default
`cargo nextest run` with no extra `--features`.

## Known limitations

- Risk-engine denials (for example `max_notional_per_order` -> `OrderDenied`) cannot run through the
  full seam. A synchronous denial republishes the `OrderDenied` on `events.order` while the risk
  engine's command borrow is still held, re-entering the risk engine's own `events.order` subscriber
  and panicking on the `RefCell`. The risk engine's queued command endpoint defers exactly this kind of
  re-entrant dispatch (`crates/risk/src/engine/mod.rs`), but the harness drives the direct endpoint.
  The path is venue-agnostic and is covered in `crates/risk/tests/risk_engine.rs`.
