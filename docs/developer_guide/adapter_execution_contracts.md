# Adapter Execution and WebSocket Contracts

Standard revision: 1 — Initial scope for [Issue #4415](https://github.com/nautechsystems/nautilus_trader/issues/4415)

## Overview

Adapters have accumulated variance in how they route live execution updates, reconciliation
reports, query results, and WebSocket dispatch flows. This document defines the standard contracts
that new adapters must follow and that older adapters must migrate toward. It is a focused companion
to the general [adapter development guide](adapters.md) and the
[execution testing specification](spec_exec_testing.md).

The contracts serve three purposes:

1. **Correctness boundary.** The execution engine treats Order Events and Reports as fundamentally
   different inputs. Misrouting one as the other produces duplicate fills, missed state transitions,
   or reconciliation failures.
2. **Migration target.** Existing adapters that deviate from these contracts are migration
   candidates. Each deviation must be tracked and resolved or explicitly documented as a
   venue-specific exception.
3. **Review contract.** New adapter code is reviewed against these rules. A reviewer can reject a
   PR that routes tracked live fills through reports without a documented venue exception.

## Order Events vs Reports

This is the most critical architectural distinction in adapter execution code. The two types serve
different purposes, flow through different engine paths, and must never be conflated.

### Order Events (causal, authoritative)

Order Events drive the order state machine forward. They represent **live, own-order activity** for
orders that the adapter submitted and tracks. Each event is a causal state advancement: the order
was in state A, and this event transitions it to state B.

The canonical Order Event types are:

| Event              | State transition                                                               |
| ------------------ | ------------------------------------------------------------------------------ |
| `OrderSubmitted`   | The command entered the adapter's submission path.                             |
| `OrderAccepted`    | The venue acknowledged the order and assigned a venue order ID.                |
| `OrderRejected`    | The venue definitively rejected the order.                                     |
| `OrderFilled`      | A fill occurred against the order (partial or complete).                       |
| `OrderCanceled`    | The venue confirmed cancellation.                                              |
| `OrderUpdated`     | The venue confirmed a modify (quantity, price, or trigger price change).       |
| `OrderExpired`     | The order expired per its time-in-force or venue policy.                       |
| `OrderTriggered`   | A stop or conditional order reached its trigger condition.                     |
| `OrderDenied`      | Local validation rejected the command before submission.                       |

**Routing path:** Order Events flow through
[`ExecutionEventEmitter::send_order_event`](../../crates/live/src/execution/emitter.rs) as
`ExecutionEvent::Order(OrderEventAny)`. The execution engine applies them to the in-memory order
model, advances the state machine, and notifies strategies.

**Contract:**

- Emit Order Events only for orders that this adapter submitted and tracks via `ClientOrderId`.
- Each event must carry correct `ClientOrderId`, `VenueOrderId`, `StrategyId`, and `InstrumentId`
  correlation.
- Events must be emitted in a causal order consistent with the venue's state transitions. When the
  venue delivers events out of order, synthesize missing intermediate events only when the adapter
  has complete identity and venue evidence proving the skipped state.
- Deduplicate by stable venue identity (trade ID for fills, venue order ID for status changes), not
  by transport origin.

### Reports (observational, reconciliation)

Reports are **observational snapshots** of venue state. They do not drive the state machine
directly. The execution engine uses them for reconciliation, explicit query responses, mass status
results, and external order discovery.

The canonical Report types are:

| Report                   | Purpose                                                                    |
| ------------------------ | -------------------------------------------------------------------------- |
| `OrderStatusReport`      | A point-in-time snapshot of an order's venue state.                        |
| `FillReport`             | A record of a single execution (fill) from the venue's perspective.        |
| `PositionStatusReport`   | A snapshot of a venue-side position.                                       |
| `ExecutionMassStatus`    | A bundled collection of order, fill, and position reports.                 |

**Routing path:** Reports flow through
[`ExecutionEventEmitter::send_execution_report`](../../crates/live/src/execution/emitter.rs) as
`ExecutionEvent::Report(ExecutionReport)`. The execution engine's reconciliation module converts
them into the appropriate order events when needed, handling external order creation, state
catch-up, and fill replay.

**Contract:**

- Reports are emitted in response to explicit query commands (`GenerateOrderStatusReport`,
  `GenerateOrderStatusReports`, `GenerateFillReports`, `GenerateExecutionMassStatus`).
- Reports are emitted for reconciliation at startup and reconnect.
- Reports are emitted for external or untracked orders observed on private streams.
- Reports must preserve available venue identity without inventing strategy, client order, or
  trader identity for untracked orders.

### Anti-pattern: routing tracked live updates through reports

> **Do not route tracked live fills or status updates through `FillReport` or
> `OrderStatusReport`.** This is the single most important rule in this document.

When an adapter receives a fill on its private WebSocket stream for an order it submitted and
tracks, it must emit `OrderFilled` through `send_order_event`, not a `FillReport` through
`send_fill_report`.

**Why this matters:**

- The execution engine applies Order Events synchronously to the state machine. Reports go through
  a separate reconciliation path that may reorder, deduplicate, or drop events that appear
  redundant against existing state.
- Routing a live fill as a report can cause the strategy to miss the fill notification, see
  delayed state, or encounter duplicate position updates when reconciliation later replays the
  same fill.
- The deduplication logic in the reconciliation path is designed for catch-up, not for
  primary event delivery. It makes different assumptions about ordering and completeness.

**Correct routing decision tree:**

```text
Is this order tracked by this adapter (has a ClientOrderId we assigned)?
├── YES → Emit Order Event (OrderFilled, OrderCanceled, etc.)
│         via emitter.emit_order_filled() / emitter.send_order_event()
└── NO  → Emit Report (FillReport, OrderStatusReport)
          via emitter.send_fill_report() / emitter.send_order_status_report()
          Let the engine handle external order creation.
```

This routing decision is documented in the adapter guide's
[tracked and external execution updates](adapters.md#tracked-and-external-execution-updates)
section. The adapter may use any internal state structure to make this decision, but the decision
itself is not optional.

## WebSocket dispatch and lifecycle

WebSocket dispatch is the most variable area across adapters. This section defines the contracts
that all adapters must satisfy regardless of their internal dispatch architecture.

### Reconnect contract

Reconnection must restore **protocol state**, not only the TCP/TLS socket. The following
postconditions must hold after every reconnect:

1. **Command paths are recreated.** No public command can be sent on a stale or closed channel.
   Replace or recreate command senders before reporting the client as active.
2. **Authentication is re-established.** Private sessions must re-authenticate. Use
   [`AuthTracker`](../../crates/network/src/websocket/auth.rs) when authentication state is shared
   across client, handler, and reconnect paths.
3. **Subscription intent is replayed.** All active and pending-subscribe topics must be
   re-sent to the venue. Do not replay pending-unsubscribe topics. Use
   [`SubscriptionState`](../../crates/network/src/websocket/subscription.rs) to track
   intent vs confirmation.
4. **Sequence and snapshot state is reset** when the venue protocol requires a fresh bootstrap
   after reconnect. Do not carry stale sequence numbers, order-book snapshots, or gap state
   across a reconnect boundary.
5. **In-flight execution state is preserved.** Orders submitted before disconnect may have
   reached the venue. Preserve enough state to correlate late responses, and trigger
   reconciliation or query recovery for commands with unknown outcomes.
6. **Downstream consumers are notified** when they must reset protocol-dependent local state
   (e.g., order-book sequence tracking).
7. **Cancellation tokens are replaced.** A canceled token must be replaced before reconnect
   starts new work. A reused canceled token causes every new task to exit immediately.

### Replay and dispatch responsibilities

The adapter owns replay correctness. The venue does not guarantee that a reconnect replays every
message from the exact disconnection point.

| Responsibility                | Owner   | Contract                                                                |
| ----------------------------- | ------- | ----------------------------------------------------------------------- |
| Socket reconnect and backoff  | Network | Shared `WebSocketClient` handles transport-level reconnect.             |
| Authentication replay         | Adapter | Re-authenticate private sessions before subscribing or sending orders.  |
| Subscription replay           | Adapter | Replay active intent from `SubscriptionState` after authentication.     |
| Fill deduplication on replay  | Adapter | Use stable venue trade IDs. Do not emit duplicate `OrderFilled` events. |
| Snapshot re-bootstrap         | Adapter | Request fresh snapshots when the venue requires it post-reconnect.      |
| Gap detection                 | Adapter | Detect and handle sequence gaps per venue protocol.                     |
| Reconciliation after reconnect| Adapter | Query or poll for unknown-outcome commands when stream replay is insufficient. |

### Subscription lifecycle for shared venue streams

Many venues multiplex multiple instruments on a single WebSocket connection. The adapter must
manage subscription lifecycle correctly for shared streams:

**Reference counting:** Use `SubscriptionState` to track subscriber counts. A subscription is
sent to the venue on the first subscriber and removed only when the last subscriber unsubscribes.

**Confirmation model:**

- Confirm from an explicit venue acknowledgement when the protocol provides one.
- Confirm from authoritative first data when acknowledgements are absent or unreliable.
- Both paths can coexist because confirmation is idempotent.
- Never confirm from local send success alone.
- On a negative subscribe result, call `mark_failure` so reconnect retains the intent.

**Unsubscribe isolation:** Correlate unsubscribe results separately. A late subscribe
acknowledgement must not revive removed intent, and a stale unsubscribe acknowledgement must not
erase a later resubscription.

**Topic key stability:** Derive a stable topic key from venue subscription arguments, but
preserve the original arguments when replay would require lossy reverse-parsing.

### Command channel ownership and handler/client boundaries

The standard pattern separates an outer client from a handler task:

```text
Client (orchestrator)           Handler (I/O boundary)
┌──────────────────────┐        ┌──────────────────────┐
│ cmd_tx ──────────────┼───────▶│ cmd_rx               │
│                      │        │   ↓ serialize         │
│                      │        │ WebSocket             │
│                      │        │   ↓ parse → transform │
│ out_rx ◀─────────────┼────────│ out_tx               │
└──────────────────────┘        └──────────────────────┘
```

**Ownership rules:**

| Concern                | Owner   | Rationale                                                              |
| ---------------------- | ------- | ---------------------------------------------------------------------- |
| Lifecycle and connect  | Client  | The client decides when to connect, disconnect, and reconnect.         |
| Subscription intent    | Client  | The client tracks what the user requested vs what the venue confirmed. |
| Wire serialization     | Handler | Keep protocol encoding close to the socket.                            |
| Frame decoding/parsing | Handler | Decode once, route to typed messages.                                  |
| Domain event emission  | Client  | The client owns the `ExecutionEventEmitter` and cache context.         |
| Authentication state   | Shared  | Use `AuthTracker` for cross-boundary auth state.                       |

**Handler initialization invariant:** No public subscribe, order, or control command may overtake
handler initialization. Queue initialization before publishing a command sender or connected
state. Test a command issued at the connection boundary to verify ordering.

**Channel direction:**

- `cmd_tx` → `cmd_rx`: Commands flow from client to handler (subscribe, place order, cancel).
- `out_tx` → `out_rx`: Parsed venue messages flow from handler to client.
- Both channels are unbounded Tokio MPSC channels. Do not introduce bounded channels on live
  event paths without an explicit shared design decision.

## Cancel-replace recovery

When a venue implements modify as cancel-replace (issuing a cancel for the old order and a new
order for the replacement), the adapter must handle several race conditions:

### Venue order ID mapping

Update the venue order ID mapping **before** routing the replacement leg. The mapping from
`ClientOrderId` to `VenueOrderId` must reflect the new venue order ID before any events from the
replacement leg are processed.

### Stale old-leg cancel suppression

A cancel confirmation for the old venue order ID may arrive after the replacement is active. The
adapter must distinguish:

- A stale cancel for the old leg (expected, suppress or make idempotent).
- A genuine cancel of the active replacement (unexpected, route as `OrderCanceled`).

Use the venue order ID to discriminate. If the cancel references the old venue order ID and a
replacement is active under a new venue order ID, treat it as the expected cancel-replace
completion rather than a cancellation of the user's order.

### Quantity calculation

Calculate the replacement order quantity from the **current cumulative fills**, not from the
original order quantity. If fills occurred between the modify request and the cancel-replace
execution, the replacement quantity must account for them.

### Query recovery for unknown modify outcomes

When a modify command has an unknown outcome (transport error, timeout, no acknowledgement):

1. Query the venue for the current order state using the known venue order ID(s).
2. If the old order is still active with the original parameters, the modify did not reach the
   venue. The order remains in its pre-modify state.
3. If a new venue order ID exists with modified parameters, the modify succeeded as a
   cancel-replace. Update the mapping and emit `OrderUpdated`.
4. If the old order is canceled and no replacement exists, the modify partially executed as a
   cancel. Emit `OrderCanceled`.

### Testing requirements

Cancel-replace recovery is venue-specific and requires focused race tests:

- Fill arriving between modify request and cancel-replace execution.
- Stale cancel for the old leg arriving after the replacement is active.
- Modify timeout followed by query recovery finding the replacement.
- Modify timeout followed by query recovery finding the original order unchanged.
- Concurrent cancel and modify for the same order.

These tests should use mock transports with controlled message ordering.

## Venue-specific exceptions

Any deviation from the contracts in this document must be:

1. **Explicitly documented** in the adapter's integration guide
   (`docs/integrations/<adapter>.md`) with the specific contract being deviated from.
2. **Justified with targeted evidence** — venue documentation, protocol captures, or
   reproducible test cases that prove the standard contract cannot apply.
3. **Scoped as narrowly as possible.** A venue quirk in one product family does not justify
   deviating for all products.
4. **Tracked for migration.** If a venue changes its protocol to align with the standard
   contract, the exception must be removed.

### Exception documentation template

When documenting a venue-specific exception, include:

```markdown
### Exception: [brief description]

**Contract deviated from:** [reference to section in this document]

**Venue evidence:** [link to venue docs, protocol capture, or test case]

**Scope:** [which products, order types, or conditions trigger this exception]

**Behavior:** [what the adapter does instead of the standard contract]

**Migration condition:** [under what circumstances this exception can be removed]
```

### Known exception categories

The following categories commonly require venue-specific handling. This list is not exhaustive:

- Venues that do not provide explicit subscription acknowledgements.
- Venues that deliver fills only through REST polling rather than WebSocket streams.
- Venues that use a single event type for both order updates and fills.
- Venues that do not assign stable trade IDs across REST and WebSocket delivery.
- Venues that implement modify as atomic amend rather than cancel-replace.
- Venues that require sequence-based rather than subscription-based reconnect replay.

## References

- [Adapter development guide](adapters.md) — Full adapter development lifecycle and patterns.
- [Execution testing specification](spec_exec_testing.md) — Test matrix for adapter execution.
- [Data testing specification](spec_data_testing.md) — Test matrix for adapter data.
- [`ExecutionEventEmitter`](../../crates/live/src/execution/emitter.rs) — Event generation and
  dispatch.
- [`ExecutionEvent`](../../crates/common/src/messages/mod.rs) — Order event and report routing
  enum.
- [`ExecutionReport`](../../crates/common/src/messages/execution/mod.rs) — Report variants for
  reconciliation.
- [`SubscriptionState`](../../crates/network/src/websocket/subscription.rs) — Subscription
  lifecycle tracking.
- [`AuthTracker`](../../crates/network/src/websocket/auth.rs) — Cross-boundary authentication
  state.
- [Reconciliation module](../../crates/execution/src/reconciliation/orders.rs) — Report-to-event
  conversion for order state catch-up.
