# Design Principles

This page defines the principles, policies, and trade-offs that guide NautilusTrader design.
[Architecture](../concepts/architecture.md) describes the components and runtime structure.
These policies guide implementation and review; they do not establish that every existing path
already conforms. Specific guides describe current behavior and limits.

## Design priorities

Design decisions weigh these quality attributes in roughly this order:

- Reliability
- Performance
- Modularity
- Testability
- Maintainability
- Deployability

Performance improvements must preserve critical invariants and their required verification.
Testability and maintainability sustain reliability; deployability affects safe rollout,
configuration, and recovery. The priority order does not make these qualities optional.

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

### Failure containment

Stop the smallest scope whose integrity can no longer be established, provided the remaining system
can continue safely. Rejected input or a failed operation need not stop unrelated components when
isolation is established. Untrustworthy shared state may require stopping the node. Containment
must follow the API's failure contract and proven isolation boundaries; it must not assume a
component can recover from an arbitrary panic.

Stopping a process does not cancel working venue orders or remove exposure. Recovery must establish
venue state before deciding which further actions are safe.

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

- **Stable inputs**: Every consumer sees the same message payload. Replaying a sequence preserves
  the original logical inputs for backtesting, incident reconstruction, and regression testing.
- **Temporal integrity**: A message preserves what its producer reported, observed, requested, or
  inferred at creation time. Preserve the available provenance; immutability does not establish
  that the producer's information is true. Corrections require an explicit new record.
- **Safer concurrency**: Readers do not need coordination to protect message payloads from later
  rewrites. This removes a common source of races around shared state.
- **Debugging and auditability**: Logs, traces, replay tools, and dead-letter inspection retain the
  original payload, allowing investigation of what the system received or created, when, and
  what it did with that information.
- **Clear ownership boundaries**: Components treat incoming messages as input. If a component needs
  a different representation, it derives new local state or a new message explicitly.
- **More robust distribution**: Serialized messages already cross process and service boundaries as
  copies. The same ownership rule keeps the in-memory model aligned with that reality.

## Evidence and authority

Distinguish external reports, local observations, inferences, and policy decisions. Preserve their
source and uncertainty when deriving state. A valid numeric value or state transition does not by
itself establish a venue fact. Missing evidence must not become a confirmed outcome merely because
a timeout or retry limit expires.

The [execution policies](../concepts/execution/policies.md) apply this rule to command outcomes,
reconciliation, and the limits of retained history.

## Controlled nondeterminism

Make time, randomness, input ordering, and external effects explicit at the boundaries that consume
them. Reproducible tests must control the sources that affect their assertions and retain the inputs,
seeds, and configuration needed to investigate a failure. State the binary, platform, and input
conditions of any determinism guarantee.

The [DST contract](../concepts/dst.md) defines the supported scope of seed-controlled execution.
Live venue behavior and independent external inputs remain outside that guarantee.

## Bounded resource use

Design queues, retries, retained history, and callback chains with explicit resource budgets and
exhaustion behavior. Account for payload size as well as item count where memory use varies, and
bound work as well as storage so a replenishing queue cannot monopolize execution.

Choose backpressure, rejection, or safe termination according to the affected contract. Overload
must not silently lose required state transitions or leave partially applied operations presented
as complete. These are design requirements; existing paths can still be unbounded, as documented
in [live dispatch and overload behavior](../concepts/live.md#dispatch-priority-and-overload-behavior).

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

Canonical actor and strategy delivery must preserve publication order, exclusive component access,
and lifecycle eligibility, with bounded progress. Reentrancy must not change the ordering rule.
Ordered delivery does not imply an event-time cache snapshot.

These are requirements for queued dispatch, not guarantees of existing synchronous paths.
The [callback dispatch contract](callback_dispatch.md) specifies maintenance timing, ownership
boundaries, lifecycle invalidation, and draining behavior.
