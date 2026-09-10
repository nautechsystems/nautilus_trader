# Design Principles

This page defines the principles, policies, and trade-offs that guide NautilusTrader design.
[Architecture](../concepts/architecture.md) describes the components and runtime structure.

## Design priorities

Design decisions weigh these quality attributes in roughly this order:

- Reliability
- Performance
- Modularity
- Testability
- Maintainability
- Deployability

## Data integrity and failure

NautilusTrader prioritizes data integrity over availability for trading operations. Arithmetic and
data-handling boundaries return errors or panic rather than silently accepting invalid values that
could affect trading decisions.

### Invalid operations

The system fails fast, either by returning an error or panicking according to the API contract, for:

- Arithmetic overflow or underflow in operations on timestamps, prices, or quantities that exceed
  valid ranges.
- Invalid data during deserialization, including NaN, infinity, or out-of-range values in market
  data or configuration.
- Type conversion failures such as negative values where only positive values are valid
  (timestamps, quantities).
- Malformed input parsing for prices, timestamps, or precision values.

In a trading system, one incorrect price, timestamp, or quantity can propagate into:

- Incorrect position sizing or risk calculations.
- Orders placed at incorrect prices.
- Backtests producing misleading results.
- Silent financial losses.

Failing at the invalid operation provides:

- **No silent corruption**: Checked inputs fail before the invalid value propagates.
- **Immediate feedback**: The caller receives an error, or the process terminates, at the point of
  the violated contract.
- **Diagnostic context**: Errors and panic messages identify the rejected operation or value.
- **Deterministic behavior**: With deterministic ordering and configuration, the same invalid input
  produces the same failure; nondeterministic inputs can still vary the outcome.

Expected network, storage, business-validation, and user-input failures have explicit error
surfaces. Unrecoverable invariant violations stop the operation or process before invalid state
propagates. The [Rust error contracts](rust.md#failure-contract-examples) define panic and fallible
API behavior.

## Executable invariants

NautilusTrader incrementally applies high-assurance practices to critical paths. Executable
invariants verify that behavior matches the business requirements:

- Identify high-impact components, including core domain types and risk and execution flows, and
  state their invariants in plain language.
- Codify those invariants as executable checks (unit tests, property tests,
  fuzzers, and static assertions) that run in CI.
- Enforce ownership and state invariants through types and explicit failure contracts. Add formal
  tools where their assurance benefit justifies their cost.
- Require integrations to preserve existing critical-path invariants, and add executable coverage
  for invariants they introduce or alter.

This approach gives high-stakes flows additional scrutiny without applying the same assurance cost
to every path.

Further reading: [High Assurance Rust](https://highassurance.rs/).

## Message immutability

Messages (requests, responses, events, and commands) are immutable after creation. Their fields
remain unchanged for the rest of the message lifetime. See
[Message Bus: message integrity](../concepts/message_bus.md#message-integrity) for the ownership
rules that follow from this invariant.

The invariant protects several properties the system depends on:

- **Determinism**: Every consumer sees the same input. Behavior is easier to reason about, replay,
  and test.
- **Temporal integrity**: A message preserves what was true when the system emitted it. Events and
  commands remain factual records instead of containers of drifting state.
- **Safer concurrency**: Readers do not need coordination to protect message payloads from later
  rewrites. This removes a common source of races around shared state.
- **Easier debugging**: Logs, traces, replay tools, and dead-letter inspection remain useful
  because the message still reflects the original payload.
- **Reliable replay and simulation**: Replaying a sequence yields the same logical inputs as the
  original run. This supports backtesting, incident reconstruction, and regression testing.
- **Clear ownership boundaries**: Components treat incoming messages as input. If a component needs
  a different representation, it derives new local state or a new message explicitly.
- **Better auditability**: The system can answer what it knew, when it knew it, and what it did
  from that information.
- **More robust distribution**: Serialized messages already cross process and service boundaries as
  copies. The same ownership rule keeps the in-memory model aligned with that reality.

## Recovery after failure

Unrecoverable faults must not leave the system operating on potentially invalid state. Normal
startup includes configured state recovery, so restart uses the same initialization path as
ordinary startup. Recovery depends on retained state and backing-store durability.

An external supervisor owns restart after process failure. Recovery aims to minimize downtime;
its duration depends on the state to restore and the backing store. Execution recovery must
reconcile venue state rather than blindly retry venue commands. Normal operation retains graceful
shutdown; an unrecoverable fault may make cleanup unsafe.

[Runtime failure and recovery](../concepts/architecture.md#crash-only-design) describes the process
and persistence boundaries.

## Domain and integration boundaries

Domain types and contracts define trading behavior. Components communicate through explicit
interfaces and immutable messages. Ports and adapters keep venue transport and backing-store
implementations outside the shared trading core, so custom integrations preserve the same domain
contracts.

## Backtest and live behavior

Backtest, sandbox, and live environments share core trading components and behavioral contracts.
The same strategy and execution-algorithm code can run across these environments. Live execution
also introduces venue, transport, timing, persistence, external-activity, and reconciliation
behavior that a simulation may not reproduce. Shared code does not imply identical outcomes from
different inputs or operating conditions.

The [common core](../concepts/architecture.md#common-core) supplies the engines and interfaces;
[behavioral models](../concepts/behavioral_models.md) define how model implementations enter the runtime.

## Queued callback dispatch requirements

The following requirements define ordered actor and strategy callback delivery. They are design
constraints for queued dispatch, not guarantees of the existing synchronous dispatch paths.

### Ordering and reentrancy

Within one runtime thread, canonical actor and strategy callbacks must preserve publication order
across components and topics. The rule applies equally to idle and active components. A nested
publication must not overtake an earlier publication's pending deliveries, including all recipients
of the earlier publication. Independent nodes have no shared global ordering guarantee.

Callbacks require exclusive access to their component and a delivery boundary at which enclosing
mutable runtime borrows have ended. Native and Python components must follow the same ordering
contract. Raw Python topic messaging retains its separate synchronous delivery and object-identity
contract.

### Maintenance and observable state

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

### Bounded progress

Pending callbacks participate in the runtime's drain condition. Live execution uses bounded drain
batches and yields between them. Backtests finish pending work before advancing simulated time.
Runaway callback chains produce an explicit fault.

Queue overflow records a fatal error and halts execution at a safe boundary. It must neither
silently discard callbacks nor interrupt an operation midway through its synchronous effects.
Already completed effects are not rolled back by callback dispatch.
