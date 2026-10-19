# Polymarket Integration Tests

These tests exercise the Polymarket adapter against an in-process HTTP and WebSocket venue. They
never connect to Polymarket or submit funded orders.

## Run the tests

Use nextest so every test receives its own process, message bus, event senders, and logger.

```bash
cargo nextest run -p nautilus-polymarket --test integration live::
cargo nextest run -p nautilus-polymarket --test integration node::
cargo nextest run -p nautilus-polymarket --test integration
```

The last command also runs the adapter's lower-level data-client, HTTP, WebSocket, and Python
integration modules.

## Test layers

| Module                       | Boundary exercised                                                    | Purpose                                         |
| ---------------------------- | --------------------------------------------------------------------- | ----------------------------------------------- |
| `integration/exec_client.rs` | Execution client -> mock HTTP and WebSocket venue                     | Detailed adapter behavior and venue edge cases  |
| `integration/live.rs`        | Risk engine -> execution engine -> client -> venue -> cache           | Deterministic cross-layer lifecycle and routing |
| `integration/node.rs`        | Strategy -> `LiveNode` -> production client factory -> venue -> cache | Production assembly and execution-manager smoke |

The seam layer uses `nautilus_live::testing::ExecutionHarness`, enabled by the `nautilus-live`
`test-support` feature. The harness owns the test clock, cache, risk engine, execution engine,
event routing, and shared lifecycle assertions. Polymarket keeps its client construction,
instruments, and orders in `integration/harness/mod.rs`. The venue routes and state live in
`integration/mock_venue.rs`.

The node layer enables the `nautilus-live` `node` feature and constructs the adapter through
`PolymarketExecutionClientFactory`. Endpoint overrides in `PolymarketExecutionClientConfig` point
the production factory at the local venue.

## Seam flow

```text
strategy or direct command -> RiskEngine -> ExecutionEngine
  -> PolymarketExecutionClient -> mock HTTP venue
mock user WebSocket frame -> PolymarketExecutionClient
  -> AsyncRunner execution routing -> ExecutionEngine -> Cache
```

Tracked lifecycle events must use the typed order-event route. Reconciliation reports cover
external venue state and missed terminal updates. The tests assert exact status, quantity, venue
identity, risk command count, own-order-book membership, and fill deduplication where each value
applies.

The seam scenarios cover:

- connection and client registration
- direct and `ExecTester` submissions through the risk engine
- acceptance, rejection, cancellation, partial fill, full fill, and duplicate fill handling
- cancel-replace with a stable client order ID and a replaced venue order ID
- external order and fill reports from mass-status reconciliation
- startup reconciliation and recovery of a missed terminal cancellation
- ambiguous submit and cancel outcomes resolved by user WebSocket events

## Full-node smoke

The node tests boot and stop a real `LiveNode`, use a strategy to submit through the node, and let
the `ExecutionManager` consume adapter events. The scenarios cover acceptance, rejection,
cancellation, fill-driven position creation, and fill voiding with position removal.

Reconciliation is disabled in these tests so the smoke boundary stays focused on node assembly and
the order-event run loop. The seam tests cover reconciliation separately.

## Mock venue and fixtures

`integration/mock_venue.rs` owns the mock routes and state used by the existing execution-client,
seam, and node tests. It records requests, serves configurable HTTP responses, tracks open venue
order IDs, and sends fixture-backed user WebSocket frames after a client connects.

Fixtures under `../test_data/` use distinct, exact values so incorrect field mapping, correlation,
deduplication, or quantity accounting fails visibly. `TestServerState::configure_default_order_success`
sets the balance, accepted order response, and cancel response used by end-to-end happy paths.

## Add a scenario

1. Reuse an existing fixture when it represents the required venue message exactly. Add a focused
   fixture under `../test_data/` only when the wire state differs.
1. Configure `TestServerState` before dispatching the command.
1. Submit through the risk engine or a node strategy, then wait for the exact cache state.
1. Send user WebSocket state with `feed_user` when the venue confirms asynchronously.
1. Assert the stable report fields, order events, quantities, venue identity, book state, and
   positions affected by the scenario.
