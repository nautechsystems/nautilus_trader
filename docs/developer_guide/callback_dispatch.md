# Callback Dispatch Contract

This page defines the ownership, ordering, and progress requirements for queued actor and strategy
callbacks. The [design principles](design_principles.md#queued-callback-dispatch-requirements) explain
the policy.

:::info Queued delivery is inactive
These requirements are design constraints for queued actor and strategy callback delivery, not
guarantees of the existing synchronous dispatch paths.
Support for synchronous message-bus reentry does not activate queued actor or strategy callbacks.
:::

Read the [ordering contract](#ordering-and-reentrancy) first. For runtime work, use
[drain boundary design](#drain-boundary-design) and [runtime integration](#runtime-integration).
The [private primitives](#private-dispatch-primitives) describe the mechanisms and edge cases.

## Implementation limits

| Area                                                  | Implemented                                                   | Limit                                                                   |
| ----------------------------------------------------- | ------------------------------------------------------------- | ----------------------------------------------------------------------- |
| Actor delivery                                        | Private ordered-delivery primitives                           | Queued actor and strategy delivery inactive                             |
| [Root propagation](#callback-roots-and-budgets)       | Causal roots through retained work and command/event channels | Owner-thread ancestry; independent scheduled firings                    |
| [Drain safety](#draining-and-progress)                | Backtest and live running-loop boundaries                     | Live lifecycle integration and activation incomplete                    |
| [Progress budgets](#callback-roots-and-budgets)       | Completed callback deliveries charged per root                | No bound on callback duration or command-only loops                     |
| [Memory accounting](#storage-limits)                  | Retained-unit and known-storage limits                        | Not a total-process memory cap; no user configuration                   |
| [Failure handling](#failure-cleanup)                  | Unwind restoration and guarded teardown                       | Fatal errors stop all roots; no rollback of completed effects           |
| [Access and backends](#backend-compatibility)         | Private checked-access guards                                 | Unchecked access and native/Python/dynamic parity outside the guarantee |
| [Observable state](#maintenance-and-observable-state) | Synchronous cache and facade effects                          | Current cache state, not an event-time snapshot                         |

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

### Raw Python subscribers

Raw Python [topic messaging](../concepts/message_bus.md) delivers the original object synchronously to subscribed callables,
including during nested publication. It shares the runtime bus and topic space with canonical
custom-data subscriptions. The queued ordering requirement applies to canonical callbacks; raw
callables remain synchronous. A nested publication from a raw callable must still preserve the
publication order of pending canonical deliveries.

## Maintenance and observable state

Component maintenance runs with each queued event, **before that event's author callbacks**. This includes:

- Indicator updates.
- Timer cleanup.
- Contingent-order handling.

Maintenance retains its applicable lifecycle rules when author callbacks are suppressed. Events
emitted by maintenance enter the same ordered dispatch mechanism.

### Cache state and eligibility

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

## Drain boundary design

Runtime integration must concentrate callback draining at a small set of explicit ownership
boundaries. **The runtime owns drain scheduling**; individual dispatch and flush methods must not each
acquire their own draining policy. This keeps borrow release, callback ordering, and failure handling
in a small number of places and limits repeated drain calls, async propagation, and error plumbing.

Each boundary must identify:

- Which component, engine, and cache borrows have ended.
- Which callbacks may run next.
- Which existing lifecycle path handles failure.

Keep shutdown and ownership cleanup under that lifecycle owner. Callback dispatch must not introduce
a parallel lifecycle or recovery state machine. On failure, stop further dispatch, preserve the error,
and release retained ownership in the order required by the cleanup contract.

Use the smallest set of boundaries that satisfies ordering, progress, and ownership requirements.
Moving a drain outward is valid only when it preserves those requirements. Verify boundary placement
with exact callback sequence tests, including continuation across bounded drain passes; fewer drain
sites alone do not establish correctness. In the synchronous core and backtesting, deterministic
callback order remains a required activation criterion.

### Scheduling and ownership boundaries

Keep `actor::drain_callbacks(budget)` as the common operation: it processes a bounded batch and
returns whether callbacks remain or dispatch failed. Backtest settlement and live scheduling own
the repetition around that operation. Prefer these small runtime-specific loops over a shared
scheduler with runtime modes, lifecycle state, or policy objects. Share more code only when doing
so removes duplication without adding those mechanisms.

The boundary map below distinguishes existing integration points from constraints on further
integration. It does not authorize queued callback activation.

| Boundary                   | Backtest                                                                        | Live                                                                              |
| -------------------------- | ------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| Startup readiness          | Data commands and callbacks; trading commands stay queued                       | First running-loop drain after successful startup                                 |
| Event settlement           | `drain_command_queues` after enclosing engine work and after each timer handler | Bounded running-loop passes with yield and stop checks                            |
| Normal stop                | Settle due work, stop trader, settle stop-generated commands, stop engines      | Residual deadline and final buffered dispatch; standalone runner retains channels |
| Fatal failure and teardown | Release owned work before clearing callback state                               | Existing lifecycle handles failure; disposal releases runner before cleanup       |

### Backtest boundaries

Backtest settlement is the reference for concentrated scheduling, not a single end-of-timestamp
drain. Timer handlers settle their work before the next handler, including at the same timestamp;
venue processing releases its borrow before settlement. Moving all drains to the outermost loop
would change these boundaries. Startup's data-command and callback pass also cannot use unrestricted
settlement without changing when trading commands execute.

### Live startup and manual lifecycle

Preserve startup ordering: instrument events reach the cache before execution clients connect and
before subscription commands are handled.

After successful live startup, `run_with_mode` reaches its first running-loop drain after trader
start, plug-in start, and startup system dispatch, before selecting another channel message. Use
that existing boundary unless an identified producer and ordering requirement establish a need for
an earlier drain.

Connection futures hold mutable engine borrows across their awaits, as does the
mass-status request during startup reconciliation. Their completion alone does not justify another
drain site. Replay and startup failures take separate exits and require their own ownership proof.

The manual `start`/`stop` path does not run a continuous event loop: `start` returns with the runner
retained, and stop or abort processes its pending messages. It has no queued callback delivery
schedule. A lone drain in `finish_startup_trader` cannot provide continuation; activation must either
supply that schedule or explicitly keep queued delivery unsupported on this path.

### Live stop and terminal cleanup

`LiveNode::finalize_stop` cannot alone own callback teardown: callers still hold channel receivers
and normal shutdown processes buffered events afterward. The `start`/`stop` path retains its runner,
whereas `run_with_mode` owns receivers locally. The terminal cleanup owner is `LiveNode::dispose`,
not normal stop or the end of the running loop.

:::info Terminal ownership cleanup
`LiveNode::dispose` performs callback cleanup in this order:

1. Dispose the kernel. Ensure no engine or component borrow remains before releasing the runner.
1. Release the retained runner and discard its pending messages, including work queued by stop
   callbacks during disposal.
1. Log any latched callback failure, then attempt to clear callback state. Log cleanup rejection
   without forcing a reset.

Disposal must not become another callback delivery loop. Preserve normal stop and runner reuse.
Do not close channels in normal stop or at the end of the running loop to make clearing succeed;
releasing the retained runner belongs to disposal.
:::

Dropping the runner does not prove that all roots are released. Clearing must still reject active
scopes or externally retained roots and report failure without forcing a reset. A rejected clear
leaves callback state intact; disposal can be retried after the blocking ownership is released.
Verify that:

- A rooted message in the retained runner blocks clearing before disposal and is released by disposal.
- Stop-generated rooted messages are discarded by disposal without callback delivery.
- A fatal running-loop latch clears on disposal when ownership permits.
- External ownership continues to block clearing.

### Required boundary verification

Boundary changes require exact sequence assertions for:

- Backtest startup data commands and callbacks before the first input, with trading commands queued.
- Live instrument processing before execution connection, plus any ordering required by an activated route.
- Same-timestamp timers and callback-generated commands before the next timer or time advance.
- Continuation across bounded passes, live stop eligibility, and unchanged channel-only scheduling.
- Normal stop-generated work versus fatal-abort discard, plus cleanup rejection while roots remain.

These sequence checks are activation requirements, not evidence of implemented live queued delivery.
Exercise activated routes through real native and Python components. Private dispatcher tests alone
do not establish that runtime borrows end at the selected boundary.

## Runtime integration

The [root propagation tests](../../crates/common/src/actor/dispatch.rs) cover retained continuations
and data and trading commands in synchronous and live runtimes, including deferred trading children.
Live data and execution event channels also preserve roots through mixed channel hops and
[startup buffering](../../crates/live/src/node/mod.rs).

### Backtests

Backtests call the boundary drain after startup data-command batches, command and execution-event
settlement, and timer handlers. Each callback pass processes a bounded number of slots; settlement
continues while callbacks or synchronous commands remain before simulated time advances. Retained
roots alone do not keep settlement running. Startup continues to leave trading commands queued until
normal settlement.

Dispatch errors propagate out of backtest execution and trigger abort cleanup. Failed trader startup
also stops the engines and discards pending synchronous commands. Abort (`abort_run`), reset, and disposal release
synchronous command batches before clearing callback captures; externally retained work can still
prevent callback cleanup. A normal end (`end_impl`) drains residual work before clearing the dispatcher.

### Live running loops

Live running loops drain one bounded callback batch before selecting the next event and yield when
callbacks remain. Pending callbacks resume without another channel message; stop signals remain
eligible between batches. Existing channel priorities and channel-only yielding remain unchanged.

On callback failure:

- **Standalone runner**: Return the error without closing channels or discarding their messages.
- **Live node**: Trigger existing shutdown and skip both the residual event window and final buffered
  dispatch. Discard pending channel messages, including commands queued by stop callbacks, when the
  receivers drop.

Both paths retain the fatal callback latch; they do not clear callback ownership.

Startup and residual flushes do not integrate callback drains. Disposal releases owned work before
attempting callback cleanup, without delivering callbacks. Activation still requires explicit
scheduling and ownership coverage for live lifecycle paths, without assuming that each lifecycle
method needs a drain.
Live report futures can retain client borrows across loop iterations. A loop-top drain alone does
not establish client access safety; queued callback activation must account for those retained borrows.

### Activation requirements

These boundaries do not activate queued actor delivery. Before activation, runtime integration must:

- Preserve the [independent ingress boundaries](#sender-types-and-ingress) when activating additional callback routes or
  introducing reusable invocation storage.
- Complete live lifecycle drain boundaries and ownership coverage outside terminal disposal.
- Establish native and Python ownership safety for every activated callback route.
- Validate queued callbacks through complete backtest and live runtime lifecycles.
- Prove deterministic callback sequences through native and Python components in the synchronous core
  and backtests, including nested publication, fan-out, callback-generated commands, same-timestamp
  work, and continuation across drain batches. Final counts and balances alone do not prove ordering.

Public trading messages and their direct `dispatch()` path carry no callback context across threads.
The synchronous queue owns its contexts privately; live command channels use the envelopes described below.
Runtime drains alone do not establish complete live lifecycle integration or backend parity.

## Private dispatch primitives

The actor module contains private access, admission, publication, invocation, and storage primitives.
Data and trading commands preserve [callback roots](#callback-roots-and-budgets) in synchronous and live runtimes.
Locally emitted live data, execution, and system messages and time events preserve roots through their channels.
Production actor lookups, component access, and message-bus routes do not use queued callback delivery. Activating that
delivery requires native and Python ownership support at runtime boundaries; the primitives and runtime
drains alone do not establish runtime ownership safety or native, direct, and dynamic callback parity.

### Publication and admission

A publication scope reserves an **ordinal** (a sequence number) before synchronous subscribers run. Callback reservations
sort by publication ordinal and then admission ordinal, so nested publications follow every pending
recipient of their enclosing publication.

Scopes nest and drop in stack order. Dropping a scope
restores publication state without delivering callbacks. A publication unwind latches a fatal error.
The message bus does not install these scopes automatically.

Admission reserves count, known storage, and a queue slot **before the caller constructs owned
captures**:

- A successful reservation accepts its capture even if another operation subsequently latches a failure.
- An unfinished reservation blocks later deliveries and teardown.
- Cancellation leaves a slot that an explicit drain or teardown releases.
- Actor reservations retain registration identity, so replacement or re-registration of the same
  allocation skips the stale handler. The skipped delivery still counts as completed against its root's budget.

### Draining and progress

A drain processes at most its supplied **slot budget**, including cancelled slots. The queue has a
**busy head** when its first callback cannot acquire exclusive access to its component; this blocks
later delivery.

The private boundary drain adds a stricter contract to an explicit drain. Its caller must establish
a safe delivery point:

| Explicit drain outcome      | Meaning                                                                  | Boundary drain behavior                    |
| --------------------------- | ------------------------------------------------------------------------ | ------------------------------------------ |
| Empty queue                 | No queued slots remain                                                   | Return normally                            |
| Exhausted slot budget       | Slots remain after the pass                                              | Return normally; not a stall               |
| Deferred entry              | Publication, recursive drain, teardown, or checked access prevents entry | Report active work; do not latch a failure |
| Unfinished head reservation | The first reservation has no capture yet                                 | Report active work; do not latch a failure |
| Busy head                   | Exclusive component access is unavailable                                | Latch a fatal stalled-delivery error       |

Deferred entry does no work. An unfinished head reservation or busy head can occur after earlier
deliveries in the same pass. **Error results do not return a delivery count.** A busy head at a
boundary is fatal even if earlier callbacks made progress. Exhausting a slot budget, including on
cancelled slots, is not stalled delivery.

An empty queue does not imply that all retained roots have released. Ownership accounting alone is
not a reason to keep draining. The boundary drain preserves the first fatal error and leaves queued
captures owned until explicit teardown.

:::warning Release enclosing borrows before draining
Guard destruction only releases access; it does not drain callbacks. The caller must also end enclosing
engine, cache, and other untracked borrows before draining.
:::

#### Draining during unwind

An otherwise successful drain entered during panic unwinding latches a fatal delivery failure on
exit, even when its queue is empty. Its returned result remains successful; the failure is available
through the callback failure check and stops the next boundary drain.

### Callback roots and budgets

A **root** owns the delivery budget for an incoming publication or reservation and all callback work
it causes. Publication order does not depend on which root owns the work.

- Nested publications and reservations inherit the active root.
- A publication with no active root starts one on its first callback admission or command send.
  An empty scope allocates none.
- Outside a publication scope, each admission starts a separate root unless a root is already active.

#### Delivery budget versus slot budget

Each completed delivery counts once against its root's budget. Busy attempts and cancelled slots do
not count, though cancelled slots still consume the drain's slot budget.

At the chain limit, dispatch latches a fatal runaway error before running another ready callback.
The blocked callback stays queued for explicit teardown. **A fatal error halts the whole dispatcher**,
even though roots have separate budgets.

### Retained work and cleanup

Queued slots, active scopes, and retained invocation work keep their root alive. **An empty queue
does not reset a surviving root's budget.** Retained storage captures the active root, or starts one
when none is active. Growing that storage preserves its root.

#### Resumption and storage reuse

Resuming retained work through `with_chain` makes its root active for the call. Return or unwind
restores the enclosing root. Invocation batches use this mechanism for each retained value;
resuming work does not itself count as a delivery. Each invocation batch is local to one preparation
and invocation call. Its backing allocation does not choose the roots of later values: each value
captures the context active at its own admission.

#### Independent timer firings

Callback registration and causal ownership have separate lifetimes. Reusing a registered timer callback
does not reuse a firing's budget. Each independent live time-event envelope establishes its own scope
before callback lookup and invocation, even when delivery occurs inside another active scope. Retained continuations
keep their original root; storage reuse alone does not make an unrelated event a continuation.

#### Destruction and release

A delivered capture's destructor runs with the delivery's root active, including during unwind.
Uninvoked batch values also restore their root when destroyed.

The root and its storage charge release when the last owner drops. Cancellation, failed batches,
and explicit teardown use the same ownership cleanup as other retained storage. Retained work
prevents teardown until released.

### Failure cleanup

Overflow latches the first fatal failure and rejects further reservations without constructing
captures. Synchronous facade effects can finish before the runtime reports the failure at a safe
boundary. Queued slots remain owned for explicit teardown. Failed invocation batches release their
captures after preparation exits and its guards release.

The teardown order is mandatory:

1. Report the fatal error before clearing resets the latch; this is the caller's responsibility.
1. End all active publication, reservation, invocation, chain, drain, and access scopes before
   teardown begins. Clearing rejects active scopes and externally retained roots.
1. Detach storage before releasing captures.
1. Release captures.

Teardown rejects destructor-driven admission throughout clearing.

#### Preparation and destructor safety

Invocation preparation keeps the batch owner outside the call that acquires guards. On rejection or
unwind, previously transferred captures survive until those guards release. The caller reserves
before constructing each capture and acquires preparation guards inside that call. The primitives
cannot control arbitrary locals that author code constructs or explicitly destroys while holding a
borrow. Destructors must not panic during an existing unwind.

## Synchronous commands

Command ancestry depends on the context at send time:

- With an active root, commands capture it, including during callback delivery and retained-work resumption.
- Inside a publication or command-processing scope, a send starts that scope's root if needed.
- Outside those scopes and without an active root, a send carries no root. Processing then creates
  an independent root when it first admits callback work or sends a nested command.

Commands in the same drain batch do not share a root merely because they run together. Processing
and destruction restore the enclosing root on return or unwind.

### Batch and child ordering

A command drain processes the batch collected at entry, in order. Commands enqueued through a synchronous
sender by its handlers remain queued for a subsequent drain. Trading handlers can also capture deferred
children: these run depth-first, in capture order, before the next command in the collected batch. Each
child captures the root active at capture time, including a temporary nested context, rather than using
the parent's context after its handler returns. Direct endpoint routing stays unchanged.

Queue and dispatch-frame borrows end before handlers run or abandoned commands are destroyed. Command
processing does not count as callback delivery and does not automatically drain callbacks.

### Panic and overflow

If a handler panics, the panic propagates. Unprocessed commands in the collected batch and pending
deferred children are destroyed under their own contexts; newly enqueued commands remain queued. Captured
roots keep callback accounting alive and prevent explicit dispatcher teardown until those commands release them.
Abandoned commands also release their captured ownership at thread teardown.

Callback storage limits do not reject command sends. If a send needs a root and its allocation exceeds
those limits, it latches callback overflow while the command is still queued. Command entries do not
consume the callback retained-unit limit, and command payloads and queue capacity are excluded from
known callback storage. Each root allocation is charged once.

:::warning Command transport is not callback budgeting
These limits do not bound command-queue memory or loops that generate only commands.
:::

## Live command and event channels

Live data and trading command senders capture roots only on the thread where the sender is constructed.
Construct senders on the runtime thread. Sends from other threads are independent ingress, even when
another runtime has an active root there. Dispatch envelopes carry send-safe tokens; callback roots and
accounting remain thread-local. Processing a rooted envelope on another thread panics before dispatch.

### Dispatch and buffering

Envelopes retain ancestry through startup buffers, runner polling, and shutdown drains. Trading children
capture their own active roots and retain depth-first order. On the node's polled command path, the
execution observer runs before each matching endpoint dispatch, including children. Processing an
unrooted envelope starts an independent, lazy root scope.
This does not activate queued callbacks or drain them automatically.

Startup buffering preserves each event's root when splitting execution batches into individual orders.
Independent batches remain independent until dispatch. Reports still drain before buffered order events,
and account events still process immediately. Node execution observation and fill bookkeeping run under
the received event's context. Event transport does not itself consume the callback delivery budget.

### Sender types and ingress

Live data, execution, and system event senders use `EventSender<T>`, an alias for
`DispatchSender<T>`, with the same `DispatchMessage<T>` envelope as commands. System commands and
time events also use this sender. Runtime binding establishes the owner thread. Events emitted synchronously on
that thread inherit the active root; foreign-thread sends and sends without an active scope remain
independent. Context does not propagate across arbitrary spawned tasks or await points.

### System and time events

System events and commands retain their envelopes through startup buffering, polling, and shutdown.
Time event delivery restores the envelope context before resolving and invoking the callback.
Timer registration does not capture a root: scheduled firings enter independently, and reusing a
callback does not share budgets between firings. A time event sent synchronously from an active
callback scope inherits that scope instead. Callback leases, cancellation, and cleanup retain their
existing behavior; callback transport does not itself consume the delivery budget.

### Standalone senders

Standalone clients can convert a plain Tokio sender into `EventSender<T>` to keep direct domain-event
receivers. That mode carries no callback context and is not used by the live runner's channel binding.
Event payloads and channel capacity remain outside callback storage accounting, as do command payloads and queues.

### Destruction across threads

| Destruction thread | Payload context                                       | Root release                                                                |
| ------------------ | ----------------------------------------------------- | --------------------------------------------------------------------------- |
| Owner thread       | Restore the message's root while dropping its payload | Release ownership on the owner thread                                       |
| Foreign thread     | Cannot resume the owner-thread context                | Release a token through a cleanup queue without accessing the owner's roots |

The owner reclaims foreign-thread releases at its next channel capture or dispatch, callback
quiescence check, or dispatcher clear. Thread teardown also releases them. Dispatch envelopes,
thread-local context registry capacity, and cleanup-queue storage remain outside callback storage limits.

## Storage limits

The private limits are:

- **Retained units**: At most 65,536. Invocation captures and batch-capacity reservations share the
  count with queued callbacks, so the count can conservatively exceed the number of callbacks.
- **Known storage**: At most 64 MiB.
- **Callback chain**: At most 1,048,576 completed deliveries per root.

These limits are internal and expose no user configuration.

### Included storage

Known storage includes:

- Callback values, queue slots, and visible queue capacity.
- Chain allocations and invocation vector capacity.
- The caller-supplied heap charge.

Payload measurements cover accessible string and vector capacities, recursive JSON contents,
metadata entries, order-event collections, visible book entries, and option-chain entries. With
`defi`, they also cover directly owned blockchain strings.

Shared payload storage may be charged more than once. Data-type names, topics, and identifiers expose
string slices, so their charges cover lengths and exclude inaccessible spare capacity. Charges remain
held while delivery is in flight and while batches retain captures.

:::warning Not a total-process memory cap
This accounting is not a bound on total process memory. It excludes:

- Allocator bookkeeping.
- Collection internals whose capacity is not exposed by these measurements.
- Intern-pool storage and registered actor allocations.
- Opaque custom-data payloads.
- Python object graphs and dynamic-module storage.
:::

Callers must supply the known heap charge for their capture type; the generic reservation cannot
inspect arbitrary owned fields.

## Backend compatibility

Private allocation guards reject overlapping checked access. **Unchecked access and enclosing
engine/cache borrows remain outside those guards.** Native, Python, and dynamic-backend parity is
not established.

Direct and dynamic backends must preserve the same callback ordering, exclusive component access,
and lifecycle eligibility requirements. Facade effects remain synchronous. The
[plug-in boundary rules](plugins.md#boundary-rules) apply to all values crossing a dynamic-library
boundary; callback dispatch does not implicitly transfer allocation ownership across that boundary.

Admission tickets and cleanup scopes remain framework machinery; author fields, callback signatures,
and canonical facade APIs do not expose them.
