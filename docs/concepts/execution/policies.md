# Execution Policies

NautilusTrader coordinates local state with trading venues across a distributed boundary. This
page defines the policies that govern order commands, order events, persistence, and reconciliation,
including their documented behavior and known limits. Use it when interpreting an order state,
implementing an execution adapter, or designing live recovery procedures.

For the component and routing model, see [Execution](index.md). For every order status and the
primary state transitions, see [Orders](../orders/index.md#order-state-flow).

## Policy summary

| Boundary                | Behavior                                                                                        | Limit                                                                                         |
| ----------------------- | ----------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| Order state             | Each applied event must satisfy the order state machine and identity checks.                    | A status alone does not identify whether venue evidence or reconciliation produced it.        |
| Command outcome         | Adapters distinguish definitive local failures, definitive venue results, and unknown outcomes. | A transport result does not necessarily prove what the venue did.                             |
| Command delivery        | Adapters retry state-changing commands only when repeating the same request is safe.            | NautilusTrader does not guarantee exactly-once delivery across the venue boundary.            |
| Event application       | Order identity and transition checks reject invalid events; fills reject a repeated `trade_id`. | No blanket exactly-once guarantee applies to every event type or across lost retained state.  |
| Persistence before send | The cache enqueues the order and resolved execution-client origin before calling the client.    | The built-in cache backends do not wait for durable storage before the client can send.       |
| Bounded recovery        | Reconciliation recovers reported order state without applying unsupported fill economics.       | Partial history does not prove historical position economics, realized PnL, or average price. |
| Terminal policy         | Reconciliation may resolve missing or timed-out orders after configured retries.                | A policy resolution is not a venue-confirmed rejection or cancellation.                       |

## Order state

Orders are event sourced. Every order starts with `OrderInitialized`, and `OrderCore::apply`
validates the event against the order identity and the transition allowed from its present status.
It rejects an invalid transition or a repeated fill `trade_id` before changing the order, then
appends each accepted event to the order's event history.

The [order state flow](../orders/index.md#order-state-flow) shows the primary lifecycle. The model also
accepts selected recovery and real-world edge cases, including fills received while a command is
pending and late fills for canceled orders. Each event page under [Events](../events/) documents its
fields and typical transition.

### Ordering

Within one live node, the runner handles each selected message branch to completion before it
selects another. This serializes kernel-side order mutation. The order appends each accepted event
in application order and does not reorder its history by event timestamp.

When several channels are ready, the runner's priority determines which it handles next. Events
from independent adapter tasks or venues can therefore interleave, and event timestamps do not
define a global FIFO order. See
[Dispatch priority and overload behavior](../live.md#dispatch-priority-and-overload-behavior).

### Duplicate application

NautilusTrader does not use `event_id` as a universal order-level deduplication key and does not
guarantee exactly-once application for every order event. Its narrower protections are:

- An order rejects a second fill with the same `trade_id`.
- The execution engine prevents the same `trade_id` from being applied again to the target
  position.
- Fill voids use the original `trade_id` and reject duplicate, stale, conflicting, or excessive
  cumulative corrections.
- Other repeated lifecycle events must still pass the state transition. Some state-preserving
  updates and repeated pending requests are valid events and can be appended again.

These checks depend on the order and position evidence retained in the cache. Restored state keeps
its earlier trade IDs and event history available after restart. If that state and the required
venue history are absent, NautilusTrader cannot infer exactly-once application from the missing
evidence. Reports that describe one logical fill with different trade IDs remain distinct and are
subject to the normal overfill and integrity checks.

The optional [event store](../event_sourcing.md) has a separate boundary. Its capture adapter
deduplicates repeated dispatches of one message identity within a bounded recent window, and replay
applies each stored sequence entry once. This does not make an uncommitted capture durable or make
venue delivery exactly once.

## Command outcomes

Execution commands resolve according to the evidence available:

| Evidence                 | Meaning                                                         | Result                                                                                          |
| ------------------------ | --------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| Definitive local failure | Validation proves that the command was not sent.                | Denies a submit or rejects a modify or cancel when the failure is attributable to that command. |
| Definitive venue result  | The matching engine or venue explicitly confirms the outcome.   | Applies the corresponding accepted, updated, canceled, or rejected event.                       |
| Unknown live outcome     | The command may have reached the venue, but no result is known. | Keeps the command in flight for stream updates, polling, queries, or reconciliation.            |

The failure event depends on the command and when the failure becomes definitive:

| Command                             | Event                 | Meaning                                                                |
| ----------------------------------- | --------------------- | ---------------------------------------------------------------------- |
| Submit or submit order list         | `OrderDenied`         | Local checks prevent submission; no `OrderSubmitted` event is emitted. |
| Submit or submit order list         | `OrderRejected`       | The submit entered execution and was later proven unsuccessful.        |
| Modify                              | `OrderModifyRejected` | The requested modification was proven unsuccessful.                    |
| Cancel, cancel-all, or batch cancel | `OrderCancelRejected` | The requested cancellation was proven unsuccessful.                    |

For modify or cancel preparation, NautilusTrader emits the matching rejection only when the
failure is attributable to that command and proves it was not sent. Otherwise, it logs the failure
without inventing an outcome.

A successful batch response can still contain definitive per-order failures. A whole-request
failure without per-order evidence does not prove that every child command failed.

:::note[Unknown live outcomes]
Transport errors, timeouts, disconnects, task cancellation, exhausted adapter request retries,
missing acknowledgements, and parse failures after transmission usually leave the venue outcome
unknown. HTTP status codes and rate limits are definitive only when venue-specific semantics prove
that the command was not accepted.
:::

An **in-flight order** is awaiting resolution:

- `SUBMITTED`: Initial submission awaiting acceptance or rejection.
- `PENDING_UPDATE`: Modification awaiting confirmation.
- `PENDING_CANCEL`: Cancellation awaiting confirmation.

### Delivery and retry limits

A request can reach a venue even when its response is lost. NautilusTrader therefore does not make
a broad exactly-once delivery claim for submit, modify, or cancel commands.

An adapter may retry a state-changing command only when the venue protocol makes repetition safe,
such as through stable request identity and duplicate detection or idempotent semantics for the
same target. Otherwise, the adapter sends once and uses stream updates, queries, polling, or
reconciliation to resolve an unknown outcome.

Retryability and command outcome are separate. A failure can be safe to retry while still leaving
the earlier attempt ambiguous. Once an attempt may have reached the venue, a later failure remains
ambiguous unless authoritative evidence resolves the same semantic command.

## Persistence before transport

Creating a client order ID, publishing `OrderInitialized`, and sending `SubmitOrder` are in-process
actions. They do not by themselves confirm durable storage.

For a submission handled by the built-in execution engine:

1. The order exists in the cache with its `ClientOrderId` and `OrderInitialized` event before the
   final execution-client call.
1. The engine selects and validates the execution client.
1. The cache enqueues the resolved order-to-client origin for persistence, then updates the
   in-memory origin index.
1. The engine calls the selected `ExecutionClient`.
1. The cache backend processes its queued writes independently of venue transport and
   acknowledgement.

The enqueue steps fail before the client call when the cache backend cannot accept them. Successful
enqueue does not mean the backing store has committed the order or origin. The built-in Redis and
PostgreSQL cache backends process these writes asynchronously.

The final step is only the call into the adapter's `ExecutionClient`. The adapter owns the later
wire send and maps venue responses or stream updates to order events. Neither a successful client
call nor an `OrderSubmitted` event proves venue acceptance.

A process failure can therefore occur after the venue receives an order but before the local order
and origin become durable. Startup reconciliation can recover that order when the venue reports it,
but incomplete venue history can leave the node without enough evidence to reconstruct the full
execution history.

The optional [event store](../event_sourcing.md) also captures asynchronously and does not gate message
dispatch on durable commit. Live restart continues to use restored cache state plus venue
reconciliation.

## Reconciliation authority

The cached order event stream is the source of local derived order state. During live recovery,
adapter reports provide the venue evidence used to align that state. Reconciliation applies the
reports in order-status, fill, and position phases so position checks build on the reconciled order
and fill state.

Recovery means restoring available cached state, reconciling available venue reports, and holding
the strategy-start barrier until startup reconciliation finishes. It does not prove that the venue
returned complete history or that every unknown command outcome was resolved.

An explicitly bounded report set changes NETTING position and portfolio economics only when the
reports are complete and coherent, retained state is compatible, and replay matches one
authoritative position report. Otherwise, NautilusTrader updates the reported order state without
applying the unsupported fill to a position or portfolio. See
[Bounded history safety](reconciliation.md#bounded-history-safety).

Reports for orders absent from the cache can create external orders. Active claims assign an
external order to a strategy; unclaimed orders use the `EXTERNAL` strategy. See
[External order creation](reconciliation.md#external-order-creation).

External orders and fills still participate in position tracking and portfolio calculations when
their evidence passes the same reconciliation rules. The bounded-history safeguards apply whether
or not a strategy claims the activity.

Startup reconciliation runs before trader components start. A startup failure stops the node from
starting unless a documented compatibility path handles that specific condition.

### Terminal reconciliation provenance

The `reconciliation` field identifies an event generated through reconciliation. It does not by
itself distinguish a venue status report from a local policy resolution:

| Evidence path                                                       | Prior status                        | Terminal event                  | Available event provenance                                                                        |
| ------------------------------------------------------------------- | ----------------------------------- | ------------------------------- | ------------------------------------------------------------------------------------------------- |
| Explicit venue status report                                        | Any transition allowed by the model | `OrderRejected`/`OrderCanceled` | `reconciliation=true`; a rejection keeps the reported reason, or `UNKNOWN` when none is reported. |
| In-flight retry exhaustion                                          | `SUBMITTED`                         | `OrderRejected`                 | `reconciliation=true`, reason `INFLIGHT_TIMEOUT`.                                                 |
| In-flight retry exhaustion                                          | `PENDING_UPDATE`/`PENDING_CANCEL`   | `OrderCanceled`                 | `reconciliation=true`; the event has no reason field.                                             |
| Full-history order remains missing after retries and targeted query | `SUBMITTED`/`ACCEPTED`              | `OrderRejected`                 | `reconciliation=true`, reason `NOT_FOUND_AT_VENUE`.                                               |
| Full-history order remains missing after retries and targeted query | `PARTIALLY_FILLED`                  | `OrderCanceled`                 | `reconciliation=true`; the event has no reason field.                                             |

The first row is backed by an explicit venue status. The remaining rows restore a terminal local
state after an operator-configured retry policy expires. They do not prove that the venue rejected
the submit or canceled the working order.

`OrderCanceled` has no reason field, so the event alone cannot distinguish a venue-reported
cancellation from the two synthetic reconciliation paths. Consumers that require that distinction
must preserve the associated reconciliation inputs and operational logs. The optional event store
captures raw venue reports when enabled, but no `OrderCanceled` field carries the policy reason.

See [Runtime checks](reconciliation.md#runtime-checks) for query coordination, recent-order
protection, and missing-order behavior.

## Related guides

- [Execution](index.md): Component roles, routing, OMS behavior, and risk checks.
- [Execution algorithms](algorithms.md): TWAP, custom algorithms, and spawned orders.
- [Orders](../orders/): Order types, statuses, and the primary state flow.
- [Events](../events/): Event fields, dispatch, and order-to-position effects.
- [Execution reconciliation](reconciliation.md): Startup recovery, continuous checks, and
  reconciliation invariants.
- [Live trading](../live.md): Node lifecycle, dispatch policy, metrics, and shutdown.
- [Configure a live trading node](../../how_to/configure_live_trading.md): Live execution settings.
