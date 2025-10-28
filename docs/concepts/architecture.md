# Architecture

Welcome to the architectural overview of NautilusTrader.

This guide dives deep into the foundational principles, structures, and designs that underpin
the platform. Whether you're a developer, system architect, or just curious about the inner workings
of NautilusTrader, this section covers:

- The design philosophy that drives decisions and shapes the system's evolution.
- The overarching system architecture providing a bird's-eye view of the entire system framework.
- How the framework is organized to facilitate modularity and maintainability.
- The code structure that ensures readability and scalability.
- A breakdown of component organization and interaction to understand how different parts communicate and collaborate.
- And finally, the implementation techniques that are crucial for performance, reliability, and robustness.

:::note
Throughout the documentation, the term *"Nautilus system boundary"* refers to operations within
the runtime of a single Nautilus node (also known as a "trader instance").
:::

## Design philosophy

The major architectural techniques and design patterns employed by NautilusTrader are:

- [Domain driven design (DDD)](https://en.wikipedia.org/wiki/Domain-driven_design)
- [Event-driven architecture](https://en.wikipedia.org/wiki/Event-driven_programming)
- [Messaging patterns](https://en.wikipedia.org/wiki/Messaging_pattern) (Pub/Sub, Req/Rep, point-to-point)
- [Ports and adapters](https://en.wikipedia.org/wiki/Hexagonal_architecture_(software))
- [Crash-only design](#crash-only-design)

These techniques have been utilized to assist in achieving certain architectural quality attributes.

### Quality attributes

Architectural decisions are often a trade-off between competing priorities. The
below is a list of some of the most important quality attributes which are considered
when making design and architectural decisions, roughly in order of 'weighting'.

- Reliability
- Performance
- Modularity
- Testability
- Maintainability
- Deployability

### Assurance-driven engineering

NautilusTrader is incrementally adopting a high-assurance mindset: critical code
paths should carry executable invariants that verify behaviour matches the
business requirements. Practically this means we:

- Identify the components whose failure has the highest blast radius (core
  domain types, risk and execution flows) and write down their invariants in
  plain language.
- Codify those invariants as executable checks (unit tests, property tests,
  fuzzers, static assertions) that run in CI, keeping the feedback loop light.
- Prefer zero-cost safety techniques built into Rust (ownership, `Result`
  surfaces, `panic = abort`) and add targeted formal tools only where they pay
  for themselves.
- Track “assurance debt” alongside feature work so new integrations extend the
  safety net rather than bypass it.

This approach preserves the platform’s delivery cadence while giving mission
critical flows the additional scrutiny they need.

Further reading: [High Assurance Rust](https://highassurance.rs/).

### Crash-only design

NautilusTrader embraces [crash-only design](https://en.wikipedia.org/wiki/Crash-only_software),
a philosophy where *"the only way to stop the system is to crash it"*, and *"the only way to bring it
up is to recover from a crash"*. This approach simplifies state management and improves reliability
by eliminating the complexity of graceful shutdown code paths that are rarely tested.

Key principles:

- **Single code path** - Recovery from crash is the primary (and only) initialization path, ensuring it is well-tested.
- **No graceful shutdown** - The system does not attempt complex cleanup procedures that may fail or hang.
- **Externalized state** - Critical state is persisted externally (database, message bus) so crashes do not lose data.
- **Fast restart** - The system is designed to restart quickly after a crash, minimizing downtime.
- **Idempotent operations** - Operations are designed to be safely retried after restart.

This design complements the [fail-fast policy](#data-integrity-and-fail-fast-policy), where
unrecoverable errors (data corruption, invariant violations) result in immediate process termination
rather than attempting to continue in a compromised state.

**References:**

- [Crash-Only Software](https://www.usenix.org/conference/hotos-ix/crash-only-software) - Candea & Fox, HotOS 2003 (original research paper)
- [Microreboot—A Technique for Cheap Recovery](https://www.usenix.org/conference/osdi-04/microreboot—-technique-cheap-recovery) - Candea et al., OSDI 2004
- [The properties of crash-only software](https://brooker.co.za/blog/2012/01/22/crash-only.html) - Marc Brooker's blog
- [Crash-only software: More than meets the eye](https://lwn.net/Articles/191059/) - LWN.net article
- [Recovery-Oriented Computing (ROC) Project](http://roc.cs.berkeley.edu/) - UC Berkeley/Stanford research

### Data integrity and fail-fast policy

NautilusTrader prioritizes data integrity over availability for trading operations. The system employs
a strict fail-fast policy for arithmetic operations and data handling to prevent silent data corruption
that could lead to incorrect trading decisions.

#### Fail-fast principles

The system will fail fast (panic or return an error) when encountering:

- Arithmetic overflow or underflow in operations on timestamps, prices, or quantities that exceed valid ranges.
- Invalid data during deserialization including NaN, Infinity, or out-of-range values in market data or configuration.
- Type conversion failures such as negative values where only positive values are valid (timestamps, quantities).
- Malformed input parsing for prices, timestamps, or precision values.

Rationale:

In trading systems, corrupt data is worse than no data. A single incorrect price, timestamp, or quantity
can cascade through the system, resulting in:

- Incorrect position sizing or risk calculations.
- Orders placed at wrong prices.
- Backtests producing misleading results.
- Silent financial losses.

By crashing immediately on invalid data, NautilusTrader ensures:

1. **No silent corruption** - Invalid data never propagates through the system.
2. **Immediate feedback** - Issues are discovered during development and testing, not in production.
3. **Audit trail** - Crash logs clearly identify the source of invalid data.
4. **Deterministic behavior** - The same invalid input always produces the same failure.

#### When fail-fast applies

Panics are used for:

- Programmer errors (logic bugs, incorrect API usage).
- Data that violates fundamental invariants (negative timestamps, NaN prices).
- Arithmetic that would silently produce incorrect results.

Results or Options are used for:

- Expected runtime failures (network errors, file I/O).
- Business logic validation (order constraints, risk limits).
- User input validation.
- Library APIs exposed to downstream crates where callers need explicit error handling without relying on panics for control flow.

#### Example scenarios

```rust
// CORRECT: Panics on overflow - prevents data corruption
let total_ns = timestamp1 + timestamp2; // Panics if result > u64::MAX

// CORRECT: Rejects NaN during deserialization
let price = serde_json::from_str("NaN"); // Error: "must be finite"

// CORRECT: Explicit overflow handling when needed
let total_ns = timestamp1.checked_add(timestamp2)?; // Returns Option<UnixNanos>
```

This policy is implemented throughout the core types (`UnixNanos`, `Price`, `Quantity`, etc.)
and ensures that NautilusTrader maintains the highest standards of data correctness for production trading.

In production deployments, the system is typically configured with `panic = abort` in release builds,
ensuring that any panic results in a clean process termination that can be handled by process supervisors
or orchestration systems. This aligns with the [crash-only design](#crash-only-design) principle, where unrecoverable errors
lead to immediate restart rather than attempting to continue in a potentially corrupted state.

## System architecture

The NautilusTrader codebase is actually both a framework for composing trading
 systems, and a set of default system implementations which can operate in various
[environment contexts](#environment-contexts).

![Architecture](https://github.com/nautechsystems/nautilus_trader/blob/develop/assets/architecture-overview.png?raw=true "architecture")

### Core components

The platform is built around several key components that work together to provide a comprehensive trading system:

#### `NautilusKernel`

The central orchestration component responsible for:

- Initializing and managing all system components.
- Configuring the messaging infrastructure.
- Maintaining environment-specific behaviors.
- Coordinating shared resources and lifecycle management.
- Providing a unified entry point for system operations.

#### `MessageBus`

The backbone of inter-component communication, implementing:

- **Publish/Subscribe patterns**: For broadcasting events and data to multiple consumers.
- **Request/Response communication**: For operations requiring acknowledgment.
- **Command/Event messaging**: For triggering actions and notifying state changes.
- **Optional state persistence**: Using Redis for durability and restart capabilities.

#### `Cache`

High-performance in-memory storage system that:

- Stores instruments, accounts, orders, positions, and more.
- Provides performant fetching capabilities for trading components.
- Maintains consist state across the system.
- Supports both read and write operations with optimized access patterns.

#### `DataEngine`

Processes and routes market data throughout the system:

- Handles multiple data types (quotes, trades, bars, order books, custom data, and more).
- Routes data to appropriate consumers based on subscriptions.
- Manages data flow from external sources to internal components.

#### `ExecutionEngine`

Manages order lifecycle and execution:

- Routes trading commands to the appropriate adapter clients.
- Tracks order and position states.
- Coordinates with risk management systems.
- Handles execution reports and fills from venues.
- Handles reconciliation of external execution state.

#### `RiskEngine`

Provides comprehensive risk management:

- Pre-trade risk checks and validation.
- Position and exposure monitoring.
- Real-time risk calculations.
- Configurable risk rules and limits.

### Environment contexts

An environment context in NautilusTrader defines the type of data and trading venue you are working
with. Understanding these contexts is crucial for effective backtesting, development, and live trading.

Here are the available environments you can work with:

- `Backtest`: Historical data with simulated venues.
- `Sandbox`: Real-time data with simulated venues.
- `Live`: Real-time data with live venues (paper trading or real accounts).

### Common core

The platform has been designed to share as much common code between backtest, sandbox and live trading systems as possible.
This is formalized in the `system` subpackage, where you will find the `NautilusKernel` class,
providing a common core system 'kernel'.

The *ports and adapters* architectural style enables modular components to be integrated into the
core system, providing various hooks for user-defined or custom component implementations.

### Data and execution flow patterns

Understanding how data and execution flow through the system is crucial for effective use of the platform:

#### Data flow pattern

1. **External Data Ingestion**: Market data enters via venue-specific `DataClient` adapters where it is normalized.
2. **Data Processing**: The `DataEngine` handles data processing for internal components.
3. **Caching**: Processed data is stored in the high-performance `Cache` for fast access.
4. **Event Publishing**: Data events are published to the `MessageBus`.
5. **Consumer Delivery**: Subscribed components (Actors, Strategies) receive relevant data events.

#### Execution flow pattern

1. **Command Generation**: User strategies create trading commands.
2. **Command Publishing**: Commands are sent through the `MessageBus`.
3. **Risk Validation**: The `RiskEngine` validates trading commands against configured risk rules.
4. **Execution Routing**: The `ExecutionEngine` routes commands to appropriate venues.
5. **External Submission**: The `ExecutionClient` submits orders to external trading venues.
6. **Event Flow Back**: Order events (fills, cancellations) flow back through the system.
7. **State Updates**: Portfolio and position states are updated based on execution events.

#### Component state management

All components follow a finite state machine pattern with well-defined states:

- **PRE_INITIALIZED**: Component is created but not yet wired up to the system.
- **READY**: Component is configured and wired up, but not yet running.
- **RUNNING**: Component is actively processing messages and performing operations.
- **STOPPED**: Component has been gracefully stopped and is no longer processing.
- **DEGRADED**: Component is running but with reduced functionality due to errors.
- **FAULTED**: Component has encountered a critical error and cannot continue.
- **DISPOSED**: Component has been cleaned up and resources have been released.

### Messaging

To facilitate modularity and loose coupling, an extremely efficient `MessageBus` passes messages (data, commands and events) between components.

From a high level architectural view, it's important to understand that the platform has been designed to run efficiently
on a single thread, for both backtesting and live trading. Much research and testing
resulted in arriving at this design, as it was found the overhead of context switching between threads
didn't actually result in improved performance.

When considering the logic of how your algo trading will work within the system boundary, you can expect each component to consume messages
in a deterministic synchronous way (*similar* to the [actor model](https://en.wikipedia.org/wiki/Actor_model)).

:::note
Of interest is the LMAX exchange architecture, which achieves award winning performance running on
a single thread. You can read about their *disruptor* pattern based architecture in [this interesting article](https://martinfowler.com/articles/lmax.html) by Martin Fowler.
:::

## Framework organization

The codebase is organized with a layering of abstraction levels, and generally
grouped into logical subpackages of cohesive concepts. You can navigate to the documentation
for each of these subpackages from the left nav menu.

### Core / low-Level

- `core`: Constants, functions and low-level components used throughout the framework.
- `common`: Common parts for assembling the frameworks various components.
- `network`: Low-level base components for networking clients.
- `serialization`: Serialization base components and serializer implementations.
- `model`: Defines a rich trading domain model.

### Components

- `accounting`: Different account types and account management machinery.
- `adapters`: Integration adapters for the platform including brokers and exchanges.
- `analysis`: Components relating to trading performance statistics and analysis.
- `cache`: Provides common caching infrastructure.
- `data`: The data stack and data tooling for the platform.
- `execution`: The execution stack for the platform.
- `indicators`: A set of efficient indicators and analyzers.
- `persistence`: Data storage, cataloging and retrieval, mainly to support backtesting.
- `portfolio`: Portfolio management functionality.
- `risk`: Risk specific components and tooling.
- `trading`: Trading domain specific components and tooling.

### System implementations

- `backtest`: Backtesting componentry as well as a backtest engine and node implementations.
- `live`: Live engine and client implementations as well as a node for live trading.
- `system`: The core system kernel common between `backtest`, `sandbox`, `live` [environment contexts](#environment-contexts).

## Code structure

The foundation of the codebase is the `crates` directory, containing a collection of Rust crates including a C foreign function interface (FFI) generated by `cbindgen`.

The bulk of the production code resides in the `nautilus_trader` directory, which contains a collection of Python/Cython subpackages and modules.

Python bindings for the Rust core are provided by statically linking the Rust libraries to the C extension modules generated by Cython at compile time (effectively extending the CPython API).

### Dependency flow

```
┌─────────────────────────┐
│                         │
│                         │
│     nautilus_trader     │
│                         │
│     Python / Cython     │
│                         │
│                         │
└────────────┬────────────┘
 C API       │
             │
             │
             │
 C API       ▼
┌─────────────────────────┐
│                         │
│                         │
│      nautilus_core      │
│                         │
│          Rust           │
│                         │
│                         │
└─────────────────────────┘
```

:::note
Both Rust and Cython are build dependencies. The binary wheels produced from a build do not require
Rust or Cython to be installed at runtime.
:::

### Type safety

The design of the platform prioritizes software correctness and safety at the highest level.

The Rust codebase in `nautilus_core` is always type safe and memory safe as guaranteed by the `rustc` compiler,
and so is *correct by construction* (unless explicitly marked `unsafe`, see the Rust section of the [Developer Guide](../developer_guide/rust.md)).

Cython provides type safety at the C level at both compile time, and runtime:

:::info
If you pass an argument with an invalid type to a Cython implemented module with typed parameters,
then you will receive a `TypeError` at runtime.
:::

If a function or method's parameter is not explicitly typed to accept `None`, passing `None` as an
argument will result in a `ValueError` at runtime.

:::warning
The above exceptions are not explicitly documented to prevent excessive bloating of the docstrings.
:::

### Errors and exceptions

Every attempt has been made to accurately document the possible exceptions which
can be raised from NautilusTrader code, and the conditions which will trigger them.

:::warning
There may be other undocumented exceptions which can be raised by Python's standard
library, or from third party library dependencies.
:::

### Processes and threads

:::tip
For optimal performance and to prevent potential issues related to Python's memory
model and equality, it is highly recommended to run each trader instance in a separate process.
:::
