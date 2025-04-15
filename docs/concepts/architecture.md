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
Throughout the documentation, the term _"Nautilus system boundary"_ refers to operations within
the runtime of a single Nautilus node (also known as a "trader instance").
:::

## Design philosophy

The major architectural techniques and design patterns employed by NautilusTrader are:
- [Domain driven design (DDD)](https://en.wikipedia.org/wiki/Domain-driven_design)
- [Event-driven architecture](https://en.wikipedia.org/wiki/Event-driven_programming)
- [Messaging patterns](https://en.wikipedia.org/wiki/Messaging_pattern) (Pub/Sub, Req/Rep, point-to-point)
- [Ports and adapters](https://en.wikipedia.org/wiki/Hexagonal_architecture_(software))
- [Crash-only design](https://en.wikipedia.org/wiki/Crash-only_software)

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

## System architecture

The NautilusTrader codebase is actually both a framework for composing trading
systems, and a set of default system implementations which can operate in various
[environment contexts](/concepts/architecture.md#environment-contexts).

![Architecture](https://github.com/nautechsystems/nautilus_trader/blob/develop/assets/architecture-overview.png?raw=true "architecture")

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

The _ports and adapters_ architectural style enables modular components to be integrated into the
core system, providing various hooks for user-defined or custom component implementations.

### Messaging

To facilitate modularity and loose coupling, an extremely efficient `MessageBus` passes messages (data, commands and events) between components.

From a high level architectural view, it's important to understand that the platform has been designed to run efficiently
on a single thread, for both backtesting and live trading. Much research and testing
resulted in arriving at this design, as it was found the overhead of context switching between threads
didn't actually result in improved performance.

When considering the logic of how your algo trading will work within the system boundary, you can expect each component to consume messages
in a deterministic synchronous way (_similar_ to the [actor model](https://en.wikipedia.org/wiki/Actor_model)).

:::note
Of interest is the LMAX exchange architecture, which achieves award winning performance running on
a single thread. You can read about their _disruptor_ pattern based architecture in [this interesting article](https://martinfowler.com/articles/lmax.html) by Martin Fowler.
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
- `msgbus`: A universal message bus for connecting system components.
- `persistence`: Data storage, cataloging and retrieval, mainly to support backtesting.
- `portfolio`: Portfolio management functionality.
- `risk`: Risk specific components and tooling.
- `trading`: Trading domain specific components and tooling.

### System implementations

- `backtest`: Backtesting componentry as well as a backtest engine and node implementations.
- `live`: Live engine and client implementations as well as a node for live trading.
- `system`: The core system kernel common between `backtest`, `sandbox`, `live` [environment contexts](/concepts/architecture.md#environment-contexts).

## Code structure

The foundation of the codebase is the `nautilus_core` directory, containing a collection of core Rust crates including a C foreign function interface (FFI) generated by `cbindgen`.

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
and so is _correct by construction_ (unless explicitly marked `unsafe`, see the Rust section of the [Developer Guide](../developer_guide/rust.md)).

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
There may be other undocumented exceptions which can be raised by Pythons standard
library, or from third party library dependencies.
:::

### Processes and threads

:::tip
For optimal performance and to prevent potential issues related to Python's memory
model and equality, it is highly recommended to run each trader instance in a separate process.
:::
