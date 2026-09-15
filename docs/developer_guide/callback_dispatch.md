# Callback Dispatch Contract

This page defines the ownership, ordering, and progress requirements for queued actor and strategy
callbacks. The [design principles](design_principles.md#queued-callback-dispatch-requirements) explain
the policy.

:::info
These requirements are design constraints for queued actor and strategy callback delivery, not
guarantees of the existing synchronous dispatch paths.
Support for synchronous message-bus reentry does not activate queued actor or strategy callbacks.
:::

## Implementation limits

| Area                                                  | Implemented behavior                                                                                                | Limit                                                                                                                                                                                             |
| ----------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Actor delivery                                        | Private primitives support ordered, owned callback delivery.                                                        | **Queued actor and strategy delivery is inactive.** Existing synchronous paths do not gain these guarantees.                                                                                      |
| [Root propagation](#callback-roots-and-budgets)       | Retained work, data and trading commands, and locally emitted live data and execution events preserve causal roots. | Live roots remain on their owner thread and do not follow arbitrary tasks or await points. System channels and time events lack complete root propagation.                                        |
| [Drain safety](#draining-and-progress)                | Explicit drains respect slot budgets and checked access; a busy head blocks later delivery.                         | Callers must end enclosing mutable borrows. Automatic safe drains and detection of a head that cannot progress require runtime integration.                                                       |
| [Progress budgets](#callback-roots-and-budgets)       | Completed callbacks consume a per-root delivery budget.                                                             | Command and event transport do not consume that budget. Loops without callback delivery and individual callback duration are not bounded.                                                         |
| [Memory accounting](#storage-limits)                  | Private limits cover retained units and known callback storage.                                                     | Command and event payloads, channel and command-queue capacity, and the listed opaque storage are excluded. This is not a total-process memory cap; limits have no user configuration.            |
| [Failure handling](#failure-cleanup)                  | Contexts restore on unwind; retained roots block premature teardown.                                                | Fatal callback errors halt the dispatcher across roots. Command-handler panics propagate and discard pending children and the unprocessed collected batch; completed effects are not rolled back. |
| [Access and backends](#backend-compatibility)         | Private allocation guards reject overlapping checked access.                                                        | Unchecked access and enclosing engine/cache borrows remain outside those guards. Native, Python, and dynamic-backend parity is not established.                                                   |
| [Observable state](#maintenance-and-observable-state) | Event payloads describe their event; cache mutations and facade effects stay synchronous.                           | Callbacks observe current cache state, not an event-time snapshot. Queued delivery does not defer or undo facade effects.                                                                         |

## Ordering and reentrancy

:::tip What is reentrancy?
Reentrancy occurs when code is entered again before an earlier call to that code returns.
For example, a message-bus subscriber publishes another message, causing nested delivery before
the original publication finishes. This can happen on one thread; it does not require parallel execution.
:::

Within one runtime thread, canonical actor and strategy callbacks must preserve **publication order**
across components and topics. The rule applies equally to idle and active components. A nested
publication must not overtake an earlier publication's pending deliveries, including all recipients
of the earlier publication. Independent nodes have no shared global ordering guarantee.

Callbacks require **exclusive access** to their component and a delivery boundary at which enclosing
mutable runtime borrows have ended. Native and Python components must follow the same ordering
contract.

Raw Python [topic messaging](../concepts/message_bus.md) delivers the original object synchronously to subscribed callables,
including during nested publication. It shares the runtime bus and topic space with canonical
custom-data subscriptions. The queued ordering requirement applies to canonical callbacks; raw
callables remain synchronous. A nested publication from a raw callable must still preserve the
publication order of pending canonical deliveries.

## Maintenance and observable state

Component maintenance runs with each queued event, before that event's author callbacks. This
includes indicator updates, timer cleanup, and contingent-order handling. Maintenance retains its
applicable lifecycle rules when author callbacks are suppressed. Events emitted by maintenance
enter the same ordered dispatch mechanism.

Engine [cache](../concepts/architecture.md#cache) mutations and direct facade effects remain synchronous. Callbacks observe
**current cache state**; ordered delivery does not provide an event-time cache snapshot. The immutable event
payload records the event, while the cache may already reflect later changes. Keeping indicator
updates with event delivery preserves their ordering relative to the corresponding callbacks.

Author callbacks require eligibility at **both event arrival and delivery**. Stop, reset, or retirement
must not carry old callbacks into a new registration or [lifecycle](../concepts/actors.md#lifecycle) generation.

## Bounded progress

A **drain** processes queued callbacks in order, attempting delivery when permitted. Pending callbacks
participate in the runtime's drain condition. Live execution uses bounded drain batches and yields
between them. Backtests finish pending work before advancing simulated time.
Runaway callback chains produce an explicit fault.

Queue overflow records a fatal error and halts execution at a safe boundary. It must neither
silently discard callbacks nor interrupt an operation midway through its synchronous effects.
Already completed effects are not rolled back by callback dispatch.

## Private dispatch primitives

The actor module contains private access, admission, publication, invocation, and storage primitives.
Data and trading commands preserve [callback roots](#callback-roots-and-budgets) in synchronous and live runtimes.
Locally emitted live data and execution events preserve roots through their channels.
Production actor lookups, component access, and message-bus routes do not use queued callback delivery. Activating that
delivery requires explicit native and Python runtime boundaries; the primitives alone do not establish
runtime ownership safety or native, direct, and dynamic callback parity.

### Publication and admission

A publication scope reserves an **ordinal** (a sequence number) before synchronous subscribers run. Callback reservations
sort by publication ordinal and then admission ordinal, so nested publications follow every pending
recipient of their enclosing publication. Scopes nest and drop in stack order. Dropping a scope
restores publication state without delivering callbacks. A publication unwind latches a fatal error.
The message bus does not install these scopes automatically.

Admission reserves count, known storage, and a queue slot **before the caller constructs owned
captures**. A successful reservation accepts its capture even if another operation subsequently
latches a failure. An unfinished reservation blocks later deliveries and teardown; cancellation
leaves a slot that an explicit drain or teardown releases. Actor reservations retain registration
identity, so replacement or re-registration of the same allocation cancels stale delivery.

### Draining and progress

A drain processes at most its supplied slot budget, including cancelled slots. The queue has a
**busy head** when its first callback cannot acquire exclusive access to its component. This blocks later delivery.
Drains do no work during publication, recursive draining, teardown, or checked allocation access.

:::warning
Guard destruction only releases access; it does not drain callbacks. The caller must also end enclosing
engine, cache, and other untracked borrows before draining.
:::

### Callback roots and budgets

A **root** owns the delivery budget for an incoming publication or reservation and all callback work
it causes. Publication order does not depend on which root owns the work.

- Nested publications and reservations inherit the active root.
- A publication with no active root starts one on its first callback admission or command send.
  An empty scope allocates none.
- Outside a publication scope, each admission starts a separate root unless a root is already active.

Each completed delivery counts once against its root's budget. Busy attempts and cancelled slots do
not count, though cancelled slots still consume the drain's slot budget.

At the chain limit, dispatch latches a fatal runaway error before running another ready callback.
The blocked callback stays queued for explicit teardown. **A fatal error halts the whole dispatcher**,
even though roots have separate budgets.

### Retained work and cleanup

Queued slots, active scopes, and retained invocation work keep their root alive. **An empty queue
does not reset a surviving root's budget.** Retained storage captures the active root, or starts one
when none is active. Growing that storage preserves its root.

Resuming retained work through `with_chain` makes its root active for the call. Return or unwind
restores the enclosing root. Invocation batches use this mechanism for each retained value;
resuming work does not itself count as a delivery.

A delivered capture's destructor runs with the delivery's root active, including during unwind.
Uninvoked batch values also restore their root when destroyed.

The root and its storage charge release when the last owner drops. Cancellation, failed batches,
and explicit teardown use the same ownership cleanup as other retained storage. Retained work
prevents teardown until released.

### Synchronous commands

Commands capture any root active when they are sent, including during callback delivery and retained-work
resumption. A send inside a publication or command-processing scope starts that scope's root if needed.
A send with no active root outside those scopes carries no root; processing then creates an independent
root when it first admits callback work or sends a nested command. Commands in
the same drain batch do not share a root merely because they run together. Processing and destruction
restore the enclosing root on return or unwind.

A command drain processes the batch collected at entry, in order. Commands enqueued through a synchronous
sender by its handlers remain queued for a subsequent drain. Trading handlers can also capture deferred
children: these run depth-first, in capture order, before the next command in the collected batch. Each
child captures the root active at capture time, including a temporary nested context, rather than using
the parent's context after its handler returns. Direct endpoint routing stays unchanged.

Queue and dispatch-frame borrows end before handlers run or abandoned commands are destroyed. Command
processing does not count as callback delivery and does not automatically drain callbacks.

If a handler panics, the panic propagates. Unprocessed commands in the collected batch and pending
deferred children are destroyed under their own contexts; newly enqueued commands remain queued. Captured
roots keep callback accounting alive and prevent explicit dispatcher teardown until those commands release them.
Abandoned commands also release their captured ownership at thread teardown.

Callback storage limits do not reject command sends. If a send needs a root and its allocation exceeds
those limits, it latches callback overflow while the command is still queued. Command entries do not
consume the callback retained-unit limit, and command payloads and queue capacity are excluded from
known callback storage. Each root allocation is charged once. **This does not bound command-queue memory
or loops that generate only commands.**

### Runtime integration

The [root propagation tests](../../crates/common/src/actor/dispatch.rs) cover retained continuations
and data and trading commands in synchronous and live runtimes, including deferred trading children.
Live data and execution event channels also preserve roots through mixed channel hops and
[startup buffering](../../crates/live/src/node/mod.rs).
Before queued callback activation, runtime integration must:

- Extend root propagation to live system commands and time events.
- Preserve independent ingress boundaries when reusing long-lived storage, so unrelated events do not
  accumulate against one root's budget.
- Provide safe drain boundaries.
- Detect a busy head that cannot make progress.

Public trading messages and their direct `dispatch()` path carry no callback context across threads.
The synchronous queue owns its contexts privately; live command channels use the envelopes described below.
These primitives do not establish runtime integration or backend parity.

### Live command and event channels

Live data and trading command senders capture roots only on the thread where the sender is constructed.
Construct senders on the runtime thread. Sends from other threads are independent ingress, even when
another runtime has an active root there. Dispatch envelopes carry send-safe tokens; callback roots and
accounting remain thread-local. Processing a rooted envelope on another thread panics before dispatch.

Envelopes retain ancestry through startup buffers, runner polling, and shutdown drains. Trading children
capture their own active roots and retain depth-first order. On the node's polled command path, the
execution observer runs before each matching endpoint dispatch, including children. Processing an
unrooted envelope starts an independent, lazy root scope.
This does not activate queued callbacks or drain them automatically.

Live data and execution event senders use `EventSender<T>` with the same `DispatchMessage<T>`
envelope as commands. Runtime binding establishes the owner thread. Events emitted synchronously on
that thread inherit the active root; foreign-thread sends and sends without an active scope remain
independent. Context does not propagate across arbitrary spawned tasks or await points.

Startup buffering preserves each event's root when splitting execution batches into individual orders.
Independent batches remain independent until dispatch. Reports still drain before buffered order events,
and account events still process immediately. Node execution observation and fill bookkeeping run under
the received event's context. Event transport does not itself consume the callback delivery budget.

Standalone clients can convert a plain Tokio sender into `EventSender<T>` to keep direct domain-event
receivers. That mode carries no callback context and is not used by the live runner's channel binding.
Event payloads and channel capacity remain outside callback storage accounting, as do command payloads and queues.

Owner-thread destruction restores the message's root while dropping its payload. Foreign-thread drops
release a token through a cleanup queue without accessing the owner's roots. The owner reclaims these
roots at its next channel capture or dispatch, callback quiescence check, or dispatcher clear; thread
teardown also releases them. Foreign-thread payload destruction cannot resume an owner-thread context.
Dispatch envelopes, thread-local context registry capacity, and cleanup-queue storage remain
outside callback storage limits.

### Storage limits

The private limits are:

- **Retained units**: At most 65,536. Invocation captures and batch-capacity reservations share the
  count with queued callbacks, so the count can conservatively exceed the number of callbacks.
- **Known storage**: At most 64 MiB.
- **Callback chain**: At most 1,048,576 completed deliveries per root.

These limits are internal and expose no user configuration.

Known storage includes callback values, queue slots and visible queue capacity, chain allocations,
invocation vector capacity, and the caller-supplied heap charge. Payload measurements include accessible
string and vector capacities, recursive JSON contents, metadata entries, order-event collections, visible book
entries, option-chain entries, and, with the `defi` feature, directly owned blockchain strings.
Shared payload storage may be charged more than once. Data-type names, topics, and identifiers expose
string slices, so their charges cover lengths and exclude inaccessible spare capacity. Charges remain
held while delivery is in flight and while batches retain captures.

:::warning
This accounting is not a bound on total process memory. It excludes allocator bookkeeping,
collection internals whose capacity is not exposed by these measurements, intern-pool storage,
registered actor allocations, opaque custom-data payloads, Python object graphs, and dynamic-module
storage.
:::

Callers must supply the known heap charge for their capture type; the generic reservation cannot
inspect arbitrary owned fields.

### Failure cleanup

Overflow latches the first fatal failure and rejects further reservations without constructing
captures. Synchronous facade effects can finish before the runtime reports the failure at a safe
boundary. Queued slots remain owned for explicit teardown. Failed invocation batches release their
captures after preparation exits and its guards release.

Teardown must follow reporting of the fatal error and must run after active publication, reservation,
invocation, chain, drain, and access scopes end.
It detaches storage before releasing captures and rejects destructor-driven admission while clearing.

Invocation preparation keeps the batch owner outside the call that acquires guards. On rejection or
unwind, previously transferred captures survive until those guards release. The caller reserves
before constructing each capture and acquires preparation guards inside that call. The primitives
cannot control arbitrary locals that author code constructs or explicitly destroys while holding a
borrow. Destructors must not panic during an existing unwind.

### Backend compatibility

Direct and dynamic backends must preserve the same callback ordering, exclusive component access,
and lifecycle eligibility requirements. Facade effects remain synchronous. The
[plug-in boundary rules](plugins.md#boundary-rules) apply to all values crossing a dynamic-library
boundary; callback dispatch does not implicitly transfer allocation ownership across that boundary.

Admission tickets and cleanup scopes remain framework machinery; author fields, callback signatures,
and canonical facade APIs do not expose them.
