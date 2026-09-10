# Callback Dispatch Contract

This page defines the ownership, ordering, and progress requirements for queued actor and strategy
callbacks. The [design principles](design_principles.md#queued-callback-dispatch-requirements) explain
the policy.

The following requirements define ordered actor and strategy callback delivery. They are design
constraints for queued dispatch, not guarantees of the existing synchronous dispatch paths.

## Ordering and reentrancy

Within one runtime thread, canonical actor and strategy callbacks must preserve publication order
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

Engine cache mutations and direct facade effects remain synchronous. Callbacks observe current
cache state; ordered delivery does not provide an event-time cache snapshot. The immutable event
payload records the event, while the cache may already reflect later changes. Keeping indicator
updates with event delivery preserves their ordering relative to the corresponding callbacks.

Author callbacks require eligibility at both event arrival and delivery. Stop, reset, or retirement
must not carry old callbacks into a new registration or lifecycle generation.

## Bounded progress

Pending callbacks participate in the runtime's drain condition. Live execution uses bounded drain
batches and yields between them. Backtests finish pending work before advancing simulated time.
Runaway callback chains produce an explicit fault.

Queue overflow records a fatal error and halts execution at a safe boundary. It must neither
silently discard callbacks nor interrupt an operation midway through its synchronous effects.
Already completed effects are not rolled back by callback dispatch.
