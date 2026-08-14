# Architecture

This guide explains the architectural principles and structure of NautilusTrader:

- Design philosophy and quality attributes.
- Core components and how they interact.
- Environment contexts (backtest, sandbox, live).
- Framework organization and code structure.

:::note
For this guide, the *Nautilus system boundary* means the runtime of one Nautilus node instance.
:::

## Design philosophy

NautilusTrader uses these architectural techniques and design patterns:

- [Domain‑driven design (DDD)](https://en.wikipedia.org/wiki/Domain-driven_design)
- [Event‑driven architecture](https://en.wikipedia.org/wiki/Event-driven_programming)
- [Messaging patterns](https://en.wikipedia.org/wiki/Messaging_pattern) (publish/subscribe,
  request/response, and point‑to‑point)
- [Ports and adapters](https://en.wikipedia.org/wiki/Hexagonal_architecture_(software))
- [Crash‑only design](#crash-only-design)

These techniques help achieve certain architectural quality attributes.

### Quality attributes

Architectural decisions often trade one priority against another.
The following quality attributes guide design and architectural decisions,
roughly in order of weighting.

- Reliability
- Performance
- Modularity
- Testability
- Maintainability
- Deployability

### Assurance-driven engineering

NautilusTrader incrementally applies high‑assurance practices to critical paths. Executable
invariants verify that behavior matches the business requirements:

- Identify high‑impact components, including core domain types and risk and execution flows, and
  state their invariants in plain language.
- Codify those invariants as executable checks (unit tests, property tests,
  fuzzers, and static assertions) that run in CI.
- Use Rust's ownership and type systems, explicit `Result` surfaces, and abort‑on‑panic release
  behavior. Add formal tools where their assurance benefit justifies their cost.
- Require integrations to preserve existing critical‑path invariants, and add executable coverage
  for invariants they introduce or alter.

This approach gives high‑stakes flows additional scrutiny without applying the same assurance cost
to every path.

Further reading: [High Assurance Rust](https://highassurance.rs/).

### Crash-only design

NautilusTrader draws on [crash‑only design](https://en.wikipedia.org/wiki/Crash-only_software) when
handling unrecoverable faults. Repository release builds abort on panic, allowing an external
supervisor to restart the process instead of letting it continue with potentially invalid state.

Key principles:

- **Startup recovery**: Configured cache and event‑store recovery run through normal startup rather
  than through a separate crash‑only entry point. Ordinary startup and focused recovery tests
  exercise the same initialization flow.
- **External state**: Configured backing stores preserve selected state across process restarts,
  reducing recovery work and the risk of losing state. Durability depends on the backing store and
  its settings.
- **Supervisor‑managed restart**: An external process supervisor owns restart policy after an
  unrecoverable failure. Aborting skips graceful cleanup inside the failed process; actual downtime
  depends on the supervisor, configured state, and backing store.
- **Prompt recovery**: The design aims to minimize downtime by using normal startup recovery after
  a supervisor restarts the process. Recovery time depends on the state to restore and its backing
  store.
- **Execution recovery**: Venue commands are not generally safe to retry blindly; execution
  reconciliation handles that boundary.
- **Fail fast**: Data corruption or invariant violations terminate the operation or process instead
  of allowing invalid state to propagate.

:::note
Normal operation still uses graceful shutdown flows such as `stop` and `dispose`. They tear down
clients and, when configured, save state and flush writers. Crash‑only behavior applies to
unrecoverable faults, where continuing normal cleanup may be unsafe.
:::

This design complements the [fail‑fast policy](#data-integrity-and-fail-fast-policy): a panic caused
by an unrecoverable invariant violation immediately terminates a process built with the repository
release profile.

**References:**

- [Crash‑Only Software](https://www.usenix.org/conference/hotos-ix/crash-only-software): Candea and
  Fox, HotOS 2003.
- [Microreboot: A technique for cheap recovery](https://www.usenix.org/events/osdi04/tech/candea.html):
  Candea et al., OSDI 2004.
- [The properties of crash‑only software](https://brooker.co.za/blog/2012/01/22/crash-only.html):
  Marc Brooker.
- [Crash‑only software: More than meets the eye](https://lwn.net/Articles/191059/): LWN.net.
- [Recovery‑Oriented Computing (ROC) Project](http://roc.cs.berkeley.edu/): UC Berkeley and
  Stanford.

### Data integrity and fail-fast policy

NautilusTrader prioritizes data integrity over availability for trading operations. Arithmetic and
data‑handling boundaries return errors or panic rather than silently accepting invalid values that
could affect trading decisions.

#### Fail-fast principles

The system fails fast, either by returning an error or panicking according to the API contract, for:

- Arithmetic overflow or underflow in operations on timestamps, prices, or quantities that exceed
  valid ranges.
- Invalid data during deserialization, including NaN, infinity, or out‑of‑range values in market
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

#### When fail-fast applies

Panics are used for:

- Programmer errors (logic bugs, incorrect API usage).
- Data that violates fundamental invariants (negative timestamps, NaN prices).
- Arithmetic that would silently produce incorrect results.

APIs return `Result` or `Option` when callers, including downstream crates, can handle a failure or
absence, including:

- Expected runtime failures (network errors, file I/O).
- Business logic validation (order constraints, risk limits).
- User input validation.

#### Example scenarios

```rust
let total_ns = timestamp1 + timestamp2; // Panics on overflow.

let price = Price::new_checked(f64::NAN, precision); // Returns Err.

let total_ns = timestamp1.checked_add(timestamp2.as_u64()); // Returns None on overflow.
```

This policy is implemented throughout the core types (`UnixNanos`, `Price`, `Quantity`, etc.)
and helps NautilusTrader maintain strong data correctness for production trading.

The repository release profile sets `panic = "abort"`, so a panic terminates the process for a
supervisor or orchestration system to handle. Downstream Rust binaries control their own release
profile.

## System architecture

NautilusTrader provides both a framework for composing trading systems and default implementations
for several [environment contexts](#environment-contexts).

```mermaid
flowchart LR
    data_clients[Data clients]
    exec_clients[Execution clients]
    cache_backing[(Optional cache backing)]
    bus_backing[(Optional message bus backing)]

    subgraph kernel[NautilusKernel]
        data[DataEngine]
        risk[RiskEngine]
        execution[ExecutionEngine]
        portfolio[Portfolio]
        trader[Trader: actors, strategies, algorithms]
        bus[MessageBus]
        cache[(Cache)]
    end

    data_clients -->|market data| data
    data -->|store| cache
    data -->|publish| bus
    bus -->|callbacks| trader
    trader -->|strategy portfolio access| portfolio
    trader -->|trading commands| risk
    risk -->|validated commands| execution
    execution <--> exec_clients
    execution -->|execution state| cache
    execution -->|events| bus
    bus -->|order and position events| risk
    risk -->|read state| cache
    risk -->|read portfolio state| portfolio
    bus -->|account, order, position, and price events| portfolio
    portfolio <-->|state| cache
    cache <--> cache_backing
    bus <--> bus_backing
```

The kernel owns the shared trading core; adapters exchange market data and execution messages
through the engine boundaries.

### Core components

#### `NautilusKernel`

The central orchestration component:

- Initializes and manages the shared core components.
- Configures the messaging infrastructure.
- Selects environment‑specific clocks and behavior.
- Coordinates shared resources and lifecycle management.
- Provides one lifecycle boundary for system operations.

#### `MessageBus`

`MessageBus` centrally routes inter‑component communication:

- **Publish/subscribe**: Broadcasts events and data to multiple consumers.
- **Request/response**: Correlates requests with their responses.
- **Command/event messaging**: Routes actions and state changes through typed endpoints.
- **Optional external backing**: Sends selected publications and, for live nodes, receives
  configured external streams through a backing such as Redis. These streams provide live
  transport; durable state recovery belongs to the cache or event store.

#### `Cache`

`Cache` keeps trading state in memory:

- Stores instruments, accounts, orders, positions, and more.
- Provides indexed reads for trading components.
- Optionally persists configured state through a cache database backing.

#### `DataEngine`

Processes and routes market data throughout the system:

- Handles quotes, trades, bars, order books, custom data, and other supported types.
- Manages subscriptions and correlated request/response flows through data clients.
- Routes resulting data to consumers according to their subscriptions and requests.
- Manages data flow from external sources to internal components.

#### `ExecutionEngine`

Manages order lifecycle and execution:

- Routes trading commands to the appropriate execution clients.
- Tracks order and position states.
- Coordinates with risk management systems.
- Handles execution reports and fills from venues.
- Handles reconciliation of external execution state.

#### `RiskEngine`

Provides risk management:

- Validates order fields, balances, quantities, notionals, reduce‑only behavior, and trading state.
- Applies configurable submission and modification rate limits.
- Monitors order and position events used by its controls.

#### `Portfolio`

Maintains derived account and position state:

- Tracks balances, net positions, margin, realized and unrealized profit and loss (PnL), and
  exposure.
- Updates state and valuations from account, order, position, quote, bar, and mark‑price events.

#### `Trader`

Coordinates user trading components:

- Registers actors, strategies, and execution algorithms.
- Manages their lifecycle, clocks, and event subscriptions.

### Environment contexts

An environment context defines the data source and execution setting for a node:

- `Backtest`: Historical data with simulated execution.
- `Sandbox`: Real‑time data with simulated execution.
- `Live`: Real‑time data with live venue connections, including paper or real accounts.

### Common core

Backtest, sandbox, and live systems share the `NautilusKernel` struct from the `nautilus-system`
crate. The kernel owns the common cache, portfolio, engines, trader, clock, and messaging
infrastructure.

The *ports and adapters* architectural style enables modular components to be integrated into the
core through explicit client, backing‑store, and component interfaces, including custom
implementations.

### Data and execution flow patterns

#### Data flow: life of a quote tick

The following trace shows the path a `QuoteTick` takes from the network to a
strategy. Trades and bars follow the same cache‑then‑publish path with different
handler names. Order book deltas and depth snapshots take a different route (see
the note below the steps).

```mermaid
sequenceDiagram
    participant Adapter as DataClient adapter
    participant Channel as MPSC channel
    participant DE as DataEngine
    participant Cache as Cache
    participant MB as MessageBus
    participant Strategy as Strategy

    Adapter->>Channel: DataEvent::Data(Data::Quote(quote))
    Channel->>DE: process_data(Data::Quote)
    DE->>DE: handle_quote(quote)
    DE->>Cache: add_quote(quote)
    DE->>MB: publish_quote(topic, quote)
    MB->>Strategy: on_quote(quote)
```

1. **Adapter receives raw data.** A venue‑specific `DataClient`, such as Binance or Bybit,
   receives a WebSocket message, parses it, and constructs a `QuoteTick`.
1. **Adapter sends a data event.** The adapter sends
   `DataEvent::Data(Data::Quote(quote))` through an MPSC channel. In live mode
   this is an async unbounded channel; in backtests the engine feeds data directly.
1. **`DataEngine` processes the event.** The channel receiver routes the event to
   `DataEngine::process_data`, which dispatches to `handle_quote`.
1. **`Cache` stores the quote.** `handle_quote` calls `cache.add_quote(quote)`. When the insertion
   succeeds, components can read it through `self.cache.quote(instrument_id)`.
1. **`MessageBus` publishes.** The engine publishes the quote on a topic derived
   from the instrument ID, such as `data.quotes.BINANCE.BTCUSDT-PERP`. The
   `MessageBus` finds all handlers subscribed to that topic.
1. **Strategy handler runs.** Each subscribed strategy's `on_quote(quote)` runs on the
   single‑threaded core. After a successful cache insertion,
   `self.cache.quote(instrument_id)` returns the same quote.

:::note
For quotes, trades, and bars, the engine attempts cache insertion before publication. A synchronous
persistence or enqueue error prevents the in‑memory insertion, but the engine logs the error and
still publishes the value. Built‑in database backings perform the actual write asynchronously, so a
later database error does not roll back the cache insertion. Order book deltas and depth snapshots
are published directly, while `BookUpdater` subscriptions maintain book state separately.
:::

#### Execution flow: life of an order

A submitted order flows through validation and routing, then returns to the strategy as execution
events:

```mermaid
sequenceDiagram
    participant Strategy as Strategy
    participant RE as RiskEngine
    participant EE as ExecutionEngine
    participant EC as ExecutionClient
    participant Venue as Venue
    participant MB as MessageBus

    Strategy->>RE: submit_order(command)
    RE->>RE: pre-trade risk checks
    RE->>EE: route command
    EE->>EC: submit_order
    EC->>Venue: place order (REST/WS)
    Venue-->>EC: OrderAccepted
    EC->>EE: OrderAccepted event
    EE->>MB: publish OrderAccepted
    MB->>Strategy: on_order_accepted(event)
    Venue-->>EC: OrderFilled
    EC->>EE: OrderFilled event
    EE->>MB: publish OrderFilled
    MB->>Strategy: on_order_filled(event)
```

1. **Strategy creates a command.** The strategy calls `self.submit_order(order)`.
1. **`RiskEngine` validates.** Configured order, balance, quantity, notional, trading‑state, and
   rate checks run. If a check fails, the strategy receives `OrderDenied`,
   and the order never reaches the venue.
1. **`ExecutionEngine` routes.** The command is routed to the `ExecutionClient`
   for the target venue.
1. **`ExecutionClient` submits.** The adapter sends the order to the venue over
   REST or WebSocket.
1. **Events flow back.** The venue responds with acknowledgments and fills.
   Each event (`Accepted`, `Filled`, `Canceled`, `Rejected`, or `Expired`) flows through
   the `ExecutionEngine`, which updates order state in the `Cache` and delivers
   the event to the strategy's handler. Fill events also trigger position and
   portfolio updates.

#### Component state management

Types that implement the `Component` trait use a finite state machine. `ComponentState` defines
stable and transitional states, while `ComponentTrigger` constrains valid transitions:

```mermaid
stateDiagram-v2
    [*] --> PRE_INITIALIZED

    PRE_INITIALIZED --> READY : initialize()

    READY --> STARTING : start()
    STARTING --> RUNNING
    STARTING --> STOPPING : stop()
    STARTING --> FAULTING : fault()

    RUNNING --> STOPPING : stop()
    STOPPING --> STOPPED
    STOPPING --> FAULTING : fault()

    STOPPED --> RESETTING : reset()
    RESETTING --> READY

    STOPPED --> RESUMING : resume()
    DEGRADED --> RESUMING : resume()
    RESUMING --> RUNNING
    RESUMING --> STOPPING : stop()
    RESUMING --> FAULTING : fault()

    RUNNING --> DEGRADING : degrade()
    DEGRADING --> DEGRADED

    DEGRADED --> STOPPING : stop()
    DEGRADED --> FAULTING : fault()

    RUNNING --> FAULTING : fault()
    STOPPED --> FAULTING : fault()
    FAULTING --> FAULTED

    READY --> RESETTING : reset()
    READY --> DISPOSING : dispose()
    STOPPED --> DISPOSING : dispose()
    DISPOSING --> DISPOSED

    DISPOSED --> [*]
```

**Stable states:**

- **PRE_INITIALIZED**: The component exists but is not ready to fulfill its specification.
- **READY**: The component is configured and can start.
- **RUNNING**: The component operates normally and can fulfill its specification.
- **STOPPED**: The component has stopped successfully.
- **DEGRADED**: The component may not meet its full specification.
- **FAULTED**: The component has shut down because of a detected fault.
- **DISPOSED**: The component has shut down and released its resources.

**Transitional states:**

- **STARTING**: The component is executing its `start` actions.
- **STOPPING**: The component is executing its `stop` actions.
- **RESUMING**: The component is executing its `resume` actions after a stop or degradation.
- **RESETTING**: The component is executing its `reset` actions.
- **DISPOSING**: The component is executing its `dispose` actions.
- **DEGRADING**: The component is executing its `degrade` actions.
- **FAULTING**: The component is executing its `fault` actions.

Transitional states cover the corresponding lifecycle callback and should remain brief. If a
callback returns an error, the transition halts in its transitional state.

#### Actor vs Component traits

The Rust implementation separates targeted message dispatch from lifecycle management:

```mermaid
classDiagram
    class Actor {
        <<trait>>
        +id() Ustr
        +handle(message)
    }

    class Component {
        <<trait>>
        +component_id() ComponentId
        +state() ComponentState
        +register()
        +initialize()
        +start()
        +stop()
        +resume()
        +reset()
        +dispose()
        +degrade()
        +fault()
    }

    class ActorRegistry {
        +insert(actor)
        +get(id) shared actor handle
    }

    class ComponentRegistry {
        +insert(component)
        +get(id) shared component handle
    }

    Actor <|.. Throttler : implements
    Actor <|.. Strategy : implements
    Component <|.. Strategy : implements
    Component <|.. Trader : implements

    ActorRegistry --> Actor : manages
    ComponentRegistry --> Component : manages

    class Throttler {
        Actor only
    }

    class Strategy {
        Actor + Component
    }

    class Trader {
        Component only
    }
```

**`Actor` trait: message dispatch**

- Provides the `handle` method for receiving messages dispatched through the actor registry.
- Supports lookup by actor ID; typed unchecked accessors check the concrete actor type at runtime
  and return an `ActorRef` guard.
- Used by types that receive targeted messages, such as strategies and throttlers.

**`Component` trait: lifecycle management**

- Manages state transitions such as `start`, `stop`, `resume`, `reset`, and `dispose`.
- Registers a component with the trader ID, clock, and cache.
- Tracks component state via the finite state machine described above.
- Used by actors, strategies, execution algorithms, and the `Trader` when they need managed
  lifecycle behavior. The data, risk, and execution engines expose their own lifecycle methods but
  do not implement this trait.

:::note
Message bus access does not depend on the `Actor` trait. Code running on the node thread can use the
thread‑local `MessageBus` APIs, while `Actor` specifically enables registry‑based dispatch to an
actor ID.
:::

This separation allows:

- **Actor only**: Lightweight message handlers without lifecycle, such as `Throttler`.
- **Component only**: Lifecycle‑managed types without targeted actor dispatch, such as `Trader`.
- **Both traits**: Data actors, including strategies and execution algorithms, that need lifecycle
  management and targeted dispatch.

Separate thread‑local registries support these access patterns. Both registry `get` methods return
shared `Rc<UnsafeCell<dyn ...>>` handles. Component lifecycle wrapper functions use a private borrow
guard to reject overlapping lifecycle access; that protection does not apply to arbitrary access
through a raw registry handle. Typed actor accessors return `ActorRef` guards, which do not prevent
two simultaneous guards for the same actor. Creating overlapping mutable references is undefined
behavior. Obtain, use, and drop an `ActorRef` within one synchronous scope. Never store one or hold
it across an `.await` point. Same‑actor re‑entrant lookup is a constraint of the current dispatch
model, not a safe aliasing guarantee.

### Messaging

The `MessageBus` passes data, commands, and events between components without requiring direct
component references.

#### Threading model

Within a node, the core consumes and dispatches messages on a single thread. This includes:

- The `MessageBus` and actor callback dispatch.
- Strategy logic and order management.
- Risk engine checks and execution coordination.
- Cache reads and writes.

This single‑threaded core provides deterministic event ordering and helps maintain backtest‑live
parity, though live inputs and latency can still cause behavioral differences. Components consume
messages synchronously in a pattern *similar* to the
[actor model](https://en.wikipedia.org/wiki/Actor_model).

:::note
The [LMAX architecture](https://martinfowler.com/articles/lmax.html) is a related example of
single‑threaded transaction processing.
:::

Background services use separate threads or the process‑wide, multi‑threaded Tokio runtime. The
runtime's worker count is configurable:

- **Network and adapters**: WebSocket connections, REST clients, and data feeds run as async tasks.
- **Logging**: A worker receives log events outside the synchronous core.
- **Persistence**: Redis and PostgreSQL cache backings queue writes to async tasks. DataFusion runs
  catalog query futures on a Tokio runtime.

Async producers send data and execution events through channels. The node runner receives them and
uses the thread‑local `MessageBus` to dispatch them to engine endpoints on the core thread. Each
thread has its own bus instance; channels bridge work from other threads or tasks.

## Framework organization

The Rust workspace groups related behavior into crates under `crates/`. The public package under
`python/nautilus_trader/` provides Python facades and supporting utilities over the Rust
implementation.

### Core and domain

- `core`: Low‑level time, string, serialization, and runtime primitives.
- `model`: Trading domain types, including instruments, accounts, orders, positions, and market
  data.
- `common`: Shared runtime services, including the cache, message bus, clocks, actors, components,
  and logging.
- `serialization`: Schema and encoding support for model and event types.

### Trading and analysis

- `analysis`: Trading performance statistics and analysis.
- `indicators`: Technical indicators.
- `data`: Market‑data engines, aggregation, and data tooling.
- `execution`: Order execution, emulation, and reconciliation primitives.
- `portfolio`: Portfolio accounting and state.
- `risk`: Pre‑trade controls, position sizing, and trading state.
- `trading`: Strategies and execution algorithms.

### Infrastructure and runtimes

- `network` and `cryptography`: Networking clients, transport support, signing, and cryptographic
  providers.
- `infrastructure`, `persistence`, and `event_store`: Database backings, data catalogs, object
  storage, and event‑store integration.
- `system`: The kernel shared by backtest, sandbox, and live
  [environment contexts](#environment-contexts).
- `backtest` and `live`: Environment‑specific engines and nodes.
- `adapters/*`: Venue, broker, data, blockchain, and sandbox integrations.
- `pyo3`: The Python extension aggregator.
- `plugin`, `cli`, and `testkit`: Plugin interfaces, command‑line tools, and test support.

## Code structure

The `crates/` directory contains the Rust implementation. PyO3 collects its Python bindings into
the `nautilus_trader._libnautilus` extension module, and `python/nautilus_trader/` exposes the public
Python facades.

The `nautilus-core` and `nautilus-model` crates retain an optional C FFI for native consumers. Other
workspace crates use Rust APIs or PyO3 bindings.

### Dependency flow

```mermaid
flowchart TB
    subgraph trader["python/nautilus_trader<br/>Python"]
    end

    subgraph bindings["crates/pyo3<br/>PyO3"]
    end

    subgraph core["crates<br/>Rust"]
    end

    trader --> bindings
    bindings --> core
```

### Rust crates

Rust crate manifests declare workspace dependencies and optional feature flags. Features enable
optional functionality without adding it to minimal builds.

Selected direct workspace dependencies are shown below; arrows point to dependencies. The diagram
omits edges that do not clarify the overall direction.

```mermaid
flowchart BT
    subgraph Core["Core and domain"]
        core
        model
        common
        serialization
    end

    subgraph Trading
        trading
        data
        execution
        portfolio
        risk
    end

    subgraph Infrastructure
        network
        cryptography
        persistence
    end

    subgraph Runtime
        system
        live
        backtest
    end

    adapters
    pyo3

    model --> core
    common --> core
    common --> model
    system --> common
    trading --> common
    serialization --> model
    network --> cryptography
    persistence --> serialization
    data --> common
    execution --> common
    portfolio --> common
    risk --> portfolio
    live --> system
    live --> trading
    backtest --> system
    backtest --> persistence
    adapters --> live
    adapters --> network
    pyo3 --> adapters
```

**Crate categories:**

| Category         | Crates                                                                        | Purpose                                                 |
| ---------------- | ----------------------------------------------------------------------------- | ------------------------------------------------------- |
| Core and domain  | `core`, `model`, `common`, `serialization`                                    | Primitives, domain types, shared runtime, and encoding. |
| Trading          | `analysis`, `indicators`, `data`, `execution`, `portfolio`, `risk`, `trading` | Analysis, strategies, engines, and portfolio state.     |
| Infrastructure   | `network`, `cryptography`, `infrastructure`, `persistence`, `event_store`     | Transport, signing, databases, catalogs, and events.    |
| Runtime          | `system`, `live`, `backtest`                                                  | Kernel and environment‑specific nodes.                  |
| Integrations     | `adapters/*`                                                                  | Venue, broker, data, blockchain, and sandbox clients.   |
| Interfaces/tools | `pyo3`, `plugin`, `cli`, `testkit`                                            | Python bindings, plugins, CLI, and test support.        |

**Feature flags:**

| Feature     | Main crates                               | Effect                                                           |
| ----------- | ----------------------------------------- | ---------------------------------------------------------------- |
| `streaming` | `data`, `system`, `live`, `backtest`      | Adds persistence support for catalog streaming.                  |
| `cloud`     | `persistence`                             | Adds AWS, Azure, GCP, and HTTP object‑store backends.            |
| `python`    | Python‑facing crates                      | Adds PyO3 bindings and the transitive features each crate needs. |
| `defi`      | Domain, data, runtime, and binding crates | Adds DeFi and blockchain types and runtime paths.                |

:::note
Source builds require Rust. Prebuilt Python wheels do not require a Rust toolchain at runtime.
:::

### Type safety

The Rust codebase relies on the compiler's guarantees for safe code. Each `unsafe` block explicitly
opts out of those guarantees, so memory and type safety depend on its documented invariants. See the
Rust section of the [Developer Guide](../developer_guide/rust.md).

PyO3 validates bound arguments and converts Rust errors into Python exceptions:

:::info
Passing an incompatible Python value to a typed PyO3 parameter raises a Python exception before the
Rust method body runs.
:::

### Errors and exceptions

API documentation describes expected errors from NautilusTrader and the conditions that produce
them.

:::warning
Python's standard library and third‑party dependencies can also raise exceptions outside those
documented contracts.
:::

### Processes and threads

:::warning[One node per process]
Running multiple `LiveNode` or `BacktestNode` instances **concurrently** in the same process is not
supported because their runtime state is not isolated:

- **Logger mode and timestamps**: The logging subsystem uses global state; backtests switch the
  logging clock between static and real‑time modes.
- **Thread‑local runtime state**: A node installs its message bus, actor and component registries,
  and channel senders for the thread that drives it.
- **Process‑wide runtime state**: The Tokio runtime and logging worker are shared by the process.

Sequential execution of multiple nodes is supported when each node is disposed before the next
one starts. Focused tests exercise sequential node construction and cache‑backed state recovery
across disposed nodes.

For production deployments, add multiple strategies to one `LiveNode` within a process.
For parallel execution or workload isolation, run each node in its own separate process.
:::

### Memory allocation

The event‑driven core allocates and frees small objects at high frequency: message bus dispatch,
order event handling, and order book maintenance all exercise the heap on every event. Default
system allocators handle this pattern poorly; profiling shows allocator overhead approaching half
of hot‑loop time on both the Windows CRT heap and glibc malloc under order‑flow workloads.

The `nautilus` CLI and Python wheels use [mimalloc](https://github.com/microsoft/mimalloc) for Rust
allocations. Backtest engine benchmarks run roughly 3% to 44% faster depending on workload, with
order‑flow heavy paths gaining the most. The trade‑off is a modest increase in resident memory from
mimalloc's segment caching.

A Rust binary links exactly one global allocator, and libraries do not impose one, so the
NautilusTrader crates remain allocator‑neutral. When building directly against the crates,
opt in from your own binary (see the [Rust guide](rust.md#memory-allocator)).

## Related guides

- [Overview](overview.md): High‑level introduction to NautilusTrader.
- [Message Bus](message_bus.md): Core messaging infrastructure.
