# Architecture

Welcome to the architectural overview of NautilusTrader.

This guide dives deep into the foundational principles, structures, and designs that underpin
the platform. Whether you're a developer, system architect, or just curious about the inner workings 
of NautilusTrader, this exposition covers:

- The **Design Philosophy** that drives decisions and shapes the system's evolution
- The overarching **System Architecture** providing a bird's-eye view of the entire system framework
- How the **Framework** is organized to facilitate modularity and maintainability
- The **Code Structure** that ensures readability and scalability
- A breakdown of **Component Organization and Interaction** to understand how different parts communicate and collaborate
- And finally, the **Implementation Techniques** that are crucial for performance, reliability, and robustness

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
environment contexts.

```{note}
Throughout the documentation, the term _"Nautilus system boundary"_ refers to operations within
the runtime of a single Nautilus node (also known as a "trader instance").
```

![Architecture](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/_images/architecture-overview.png?raw=true "architecture")

### Environment contexts
- `Backtest` - Historical data with simulated venues
- `Sandbox` - Real-time data with simulated venues
- `Live` - Real-time data with live venues (paper trading or real accounts)

### Common core
The platform has been designed to share as much common code between backtest, sandbox and live trading systems as possible. 
This is formalized in the `system` subpackage, where you will find the `NautilusKernel` class, 
providing a common core system 'kernel'.

A _ports and adapters_ architectural style allows modular components to be 'plugged into' the
core system, providing many hooks for user defined / custom component implementations.

### Messaging
To facilitate modularity and loose coupling, an extremely efficient `MessageBus` passes messages (data, commands and events) between components.

From a high level architectural view, it's important to understand that the platform has been designed to run efficiently 
on a single thread, for both backtesting and live trading. Much research and testing
resulted in arriving at this design, as it was found the overhead of context switching between threads
didn't actually result in improved performance.

When considering the logic of how your algo trading will work within the system boundary, you can expect each component to consume messages
in a deterministic synchronous way (_similar_ to the [actor model](https://en.wikipedia.org/wiki/Actor_model)).

```{note}
Of interest is the LMAX exchange architecture, which achieves award winning performance running on
a single thread. You can read about their _disruptor_ pattern based architecture in [this interesting article](https://martinfowler.com/articles/lmax.html) by Martin Fowler.
```

## Framework organization
The codebase is organized with a layering of abstraction levels, and generally
grouped into logical subpackages of cohesive concepts. You can navigate to the documentation
for each of these subpackages from the left nav menu.

### Core / low-Level
- `core` - constants, functions and low-level components used throughout the framework
- `common` - common parts for assembling the frameworks various components
- `network` - low-level base components for networking clients
- `serialization` - serialization base components and serializer implementations
- `model` - defines a rich trading domain model

### Components
- `accounting` - different account types and account management machinery
- `adapters` - integration adapters for the platform including brokers and exchanges
- `analysis` - components relating to trading performance statistics and analysis
- `cache` - provides common caching infrastructure
- `data` - the data stack and data tooling for the platform
- `execution` - the execution stack for the platform
- `indicators` - a set of efficient indicators and analyzers
- `infrastructure` - technology specific infrastructure implementations
- `msgbus` - a universal message bus for connecting system components
- `persistence` - data storage, cataloging and retrieval, mainly to support backtesting
- `portfolio` - portfolio management functionality
- `risk` - risk specific components and tooling
- `trading` - trading domain specific components and tooling

### System implementations
- `backtest` - backtesting componentry as well as a backtest engine and node implementations
- `live` - live engine and client implementations as well as a node for live trading
- `system` - the core system kernel common between backtest, sandbox and live contexts

## Code structure
The foundation of the codebase is the `nautilus_core` directory, containing a collection of core Rust libraries including a C API interface generated by `cbindgen`. 

The bulk of the production code resides in the `nautilus_trader` directory, which contains a collection of Python and Cython modules. 

Python bindings for the Rust core are achieved by statically linking the Rust libraries to the C extension modules generated by Cython at compile time (effectively extending the CPython API).

```{note}
Both Rust and Cython are build dependencies. The binary wheels produced from a build do not themselves require
Rust or Cython to be installed at runtime.
```
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

### Type safety
The design of the platform holds software correctness and safety at the highest level.

The Rust codebase in `nautilus_core` is always type safe and memory safe as guaranteed by the `rustc` compiler,
and so is _correct by construction_ (unless explicitly marked `unsafe`, see the Rust section of the [Developer Guide](../developer_guide/rust.md)).

Cython provides type safety at the C level at both compile time, and runtime:

```{note}
If you pass an argument with an invalid type to a Cython implemented module with typed parameters, 
then you will receive a ``TypeError`` at runtime.

If a function or methods parameter is not explicitly typed as allowing
``None``, then you can assume you will receive a `ValueError` when passing ``None``
as an argument at runtime.
```

```{warning}
The above exceptions are not explicitly documented, as this would bloat the docstrings significantly.
```

### Errors and exceptions
Every attempt has been made to accurately document the possible exceptions which
can be raised from NautilusTrader code, and the conditions which will trigger them.

```{warning}
There may be other undocumented exceptions which can be raised by Pythons standard 
library, or from third party library dependencies.
```
