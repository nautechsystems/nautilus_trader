# Callback Dispatch Contract

This page defines the ownership, ordering, and progress requirements for queued actor and strategy
callbacks. The [design principles](design_principles.md#queued-callback-dispatch-requirements) explain
the policy.

:::info
These requirements are design constraints for queued actor and strategy callback delivery, not
guarantees of the existing synchronous dispatch paths.
:::

## Ordering and reentrancy

Within one runtime thread, canonical actor and strategy callbacks must preserve **publication order**
across components and topics. The rule applies equally to idle and active components. A nested
publication must not overtake an earlier publication's pending deliveries, including all recipients
of the earlier publication. Independent nodes have no shared global ordering guarantee.

Callbacks require exclusive access to their component and a delivery boundary at which enclosing
mutable runtime borrows have ended. Native and Python components must follow the same ordering
contract.

Raw Python topic messaging delivers the original object synchronously to subscribed callables,
including during nested publication. It shares the runtime bus and topic space with canonical
custom-data subscriptions. The queued ordering requirement applies to canonical callbacks; raw
callables remain synchronous. A nested publication from a raw callable must still preserve the
publication order of pending canonical deliveries.

## Maintenance and observable state

Component maintenance runs with each queued event, before that event's author callbacks. This
includes indicator updates, timer cleanup, and contingent-order handling. Maintenance retains its
applicable lifecycle rules when author callbacks are suppressed. Events emitted by maintenance
enter the same ordered dispatch mechanism.

Engine cache mutations and direct facade effects remain synchronous. Callbacks observe
**current cache state**; ordered delivery does not provide an event-time cache snapshot. The immutable event
payload records the event, while the cache may already reflect later changes. Keeping indicator
updates with event delivery preserves their ordering relative to the corresponding callbacks.

Author callbacks require eligibility at **both event arrival and delivery**. Stop, reset, or retirement
must not carry old callbacks into a new registration or lifecycle generation.

## Bounded progress

Pending callbacks participate in the runtime's drain condition. Live execution uses bounded drain
batches and yields between them. Backtests finish pending work before advancing simulated time.
Runaway callback chains produce an explicit fault.

Queue overflow records a fatal error and halts execution at a safe boundary. It must neither
silently discard callbacks nor interrupt an operation midway through its synchronous effects.
Already completed effects are not rolled back by callback dispatch.

## Private dispatch primitives

The actor module contains private access, admission, publication, invocation, and storage primitives.
Production actor lookups, component access, and message-bus routes do not use them. Integrating them
requires explicit native and Python runtime boundaries; the primitives alone do not establish
runtime ownership safety or native, direct, and dynamic callback parity.

### Publication and admission

A publication scope reserves an ordinal before synchronous subscribers run. Callback reservations
sort by publication ordinal and then admission ordinal, so nested publications follow every pending
recipient of their enclosing publication. Scopes nest and drop in stack order. Dropping a scope
restores publication state without delivering callbacks. A publication unwind latches a fatal error.
The message bus does not install these scopes automatically.

Admission reserves count, known storage, and a queue slot before the caller constructs owned
captures. A successful reservation accepts its capture even if another operation subsequently
latches a failure. An unfinished reservation blocks later deliveries and teardown; cancellation
leaves a slot that an explicit drain or teardown releases. Actor reservations retain registration
identity, so replacement or re-registration of the same allocation cancels stale delivery.

### Draining and progress

A drain processes at most its supplied slot budget, including cancelled slots. A busy head prevents
later delivery. Drains do no work during publication, recursive draining, teardown, or checked
allocation access. **Guard destruction only releases access.** The caller must also end enclosing
engine, cache, and other untracked borrows before draining.

The callback-chain counter persists while queued work remains and faults at its limit. Accounting
across command-mediated chains and runtime quiescence belongs to runtime integration. Before activation, runtime integration must
separate independent ingress from callback chains so sustained legitimate traffic does not exhaust
the chain limit, and detect a busy head that cannot make progress.

### Storage limits

The private limits are:

- **Retained units**: At most 65,536. Invocation captures and batch-capacity reservations share the
  count with queued callbacks, so the count can conservatively exceed the number of callbacks.
- **Known storage**: At most 64 MiB.
- **Callback chain**: At most 1,048,576 deliveries before reaching queue quiescence.

These limits are internal and expose no user configuration.

Known storage includes callback values, queue slots and visible queue capacity, invocation vector
capacity, and the caller-supplied heap charge. Payload measurements include accessible string and
vector capacities, recursive JSON contents, metadata entries, order-event collections, visible book
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
invocation, drain, and access scopes end.
It detaches storage before releasing captures and rejects destructor-driven admission while clearing.

Invocation preparation keeps the batch owner outside the call that acquires guards. On rejection or
unwind, previously transferred captures survive until those guards release. The caller reserves
before constructing each capture and acquires preparation guards inside that call. The primitives
cannot control arbitrary locals that author code constructs or explicitly destroys while holding a
borrow. Destructors must not panic during an existing unwind.

### Backing-binding compatibility

These requirements govern runtime integration with direct and dynamic backends. A callback-scoped
binding associates an author with its host backing for the duration of that callback. A nested
callback shadows the enclosing binding, temporarily preventing use of its access token.

Queued work must retain owned payloads and registration identity without retaining an active
callback-scoped binding token. Backend-owned entry code must install the proper binding and validate
the current instance and lifecycle before author delivery. Expired or shadowed tokens cannot authorize
a later callback. Host backing operations remain synchronous while author delivery follows the shared
ordering rule.

Dynamic backends must own callback storage, binding installation, module lifetime, and
producer-side release. The [plug-in boundary rules](plugins.md#boundary-rules) still apply. The
dispatcher neither assumes that thread-local storage crosses module boundaries nor transfers
allocation ownership implicitly across a dynamic-library boundary. Admission tickets and
cleanup scopes remain framework machinery; author fields, callback signatures, and canonical facade
APIs do not expose them.
