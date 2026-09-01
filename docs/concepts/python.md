# Python

NautilusTrader provides a Python control surface over the Rust core through PyO3. Use this guide to
understand which runtime owns each part of the system, where Python code executes, and which Python
interfaces form the supported public contract.

For native Rust applications, see [Rust](rust.md). For installation and supported Python versions,
see [Installation](../getting_started/installation.md).

## Runtime model

The Python package combines Python facades under `nautilus_trader` with the compiled
`nautilus_trader._libnautilus` extension. Prebuilt wheels contain the extension and do not require a
Rust toolchain at runtime.

| Layer              | Responsibility                                                                                 |
| ------------------ | ---------------------------------------------------------------------------------------------- |
| Python application | Configuration, composition, user components, analysis, and integration with Python services.   |
| PyO3 bindings      | Type conversion, argument validation, exceptions, and ownership-safe wrappers over Rust state. |
| Rust core          | Domain types, engines, nodes, cache, portfolio, message bus, adapters, and persistence.        |

Python objects such as `Cache` and `Portfolio` are wrappers over Rust-owned state. Nodes and engines
keep their internal runtime objects private and expose bounded inspection and control methods. This
preserves one source of state while allowing Python code to configure the system and inspect its
results.

## User components

Python user components subclass the public PyO3 base classes and override their documented
callbacks:

| Component            | Use                                                                           |
| -------------------- | ----------------------------------------------------------------------------- |
| `DataActor`          | Subscribe to data, handle events, and run non-trading workflows.              |
| `Strategy`           | Implement trading decisions and submit orders.                                |
| `ExecutionAlgorithm` | Split or schedule routed orders through the execution engine.                 |
| `Controller`         | Create and manage actors and strategies through `ImportableControllerConfig`. |

Application code constructs configs, registers official adapter factories, and adds components to
`BacktestNode`, `BacktestEngine`, or `LiveNode`. Rust remains responsible for routing, engine state,
order management, accounting, and venue clients.

Callbacks execute synchronously on the event-processing thread and must return promptly.
Blocking I/O, model inference, or long calculations delay market-data handling and order execution.
Offload that work to an executor or another process. See
[Configure a live trading node](../how_to/configure_live_trading.md) for the live-trading rule.

## Async execution

Rust adapter networking runs on Tokio. Python async libraries run on an asyncio event loop; PyO3
does not turn Python coroutines into Tokio tasks.

`LiveNode` supports two execution modes:

| Method        | Execution context      | Signal owner     | Completion                                                  |
| ------------- | ---------------------- | ---------------- | ----------------------------------------------------------- |
| `run()`       | Calling thread; blocks | `LiveNode`       | Returns after coordinated shutdown finishes.                |
| `run_async()` | Python host loop       | Host application | Resolves after the same coordinated shutdown path finishes. |

`run_async()` lets an asyncio or ASGI application host the node on its existing loop. It drives the
same Rust lifecycle as `run()` and leaves `SIGINT` and `SIGTERM` handling to the host. Compatibility
is tested with the default asyncio loop, uvloop, and an ASGI lifespan managed by Uvicorn. The Python
wheel does not install uvloop or Uvicorn; applications supply their chosen loop and server. An ASGI
application whose lifespan constructs a node must run with one worker and without hot reload.

Capture `node.cache`, `node.portfolio`, and `node.handle()` before starting `run_async()`. The
coroutine owns the node until it finishes, while the captured objects remain usable. Stop a hosted
run through `LiveNodeHandle.stop()`, then await the run task for complete shutdown. Cancellation
requests the same graceful shutdown before it propagates. Call `node.dispose()` after the task
finishes to release the node's resources. A host must wait for the handle to report `Running` before
reporting startup complete, supervise the run task for its lifetime, and fail the service if the
task completes unexpectedly.

See [Hosted event loops](live.md#hosted-event-loops) for the lifecycle, cancellation, fairness, and
cache-backing contract.

## Public API contract

The generated type stubs under `python/nautilus_trader/` define the supported Python surface. They
record public classes, methods, properties, parameters, and return types from the Rust binding
sources. The [Python API reference](../api_reference/index.md) renders the same public modules and
their documentation.

A runtime attribute absent from the generated stubs is not part of the supported contract. PyO3
validates bound arguments before Rust code runs and maps fallible operations to Python exceptions.
Code should handle the documented exception type instead of depending on an internal Rust error
representation.

Generated stubs are source-derived artifacts. Binding changes update the Rust source and regenerate
the stubs; the checked-in `.pyi` files are not independent API definitions.

:::warning[Side enum compatibility aliases]
`OrderSide.NO_ORDER_SIDE` and `PositionSide.NO_POSITION_SIDE` remain available as compatibility
aliases for `None`. They are not enum members and may be removed in a future version. Use `None`
for optional side values.
:::

## Ownership and lifecycle

Rust ownership remains visible at node boundaries:

- `BacktestNode` keeps its engines internal. Preserve an engine after a run with
  `dispose_on_completion=False`, then inspect it through the node's cache, portfolio, statistics,
  and report methods.
- `LiveNode.run_async()` lends the node to its coroutine. State access through the node raises during
  the run, while `is_running` and `handle()` remain available. A `dispose()` call during the run is a
  no-op, not a deferred request. Call it again after the run finishes; objects captured before the
  run remain available until then.
- Concurrent `LiveNode` or `BacktestNode` instances in one process are not supported because their
  runtime state is not isolated. Dispose one node before starting the next, or use separate processes
  for parallel execution.

These boundaries prevent Python references from exposing mutable engine internals or creating
multiple owners for the same runtime state.

## Support boundaries

Official adapters are implemented in Rust and exposed through Python configs, factories, clients,
and data types under `nautilus_trader.adapters`. Their integration guides define the supported venue
capabilities.

:::note[Custom adapter support]
The public Python API does not yet define an interface for implementing an out-of-tree adapter
entirely in Python. Official adapters remain usable from Python. Custom venue integrations
currently use the Rust adapter traits. An out-of-tree Python adapter surface is planned; see
[issue 4694](https://github.com/nautechsystems/nautilus_trader/issues/4694).
:::

Hosted `LiveNode` execution enables Python services to share an asyncio loop with a node. It does
not by itself add a custom Python adapter interface.

## Choosing Python or Rust

Use Python when application composition, rapid strategy development, analysis tools, or integration
with the Python ecosystem matters. Use Rust when the application must run without a Python runtime
or needs native traits and direct crate-level control.

Both paths use the same Rust domain model and engines. The
[Rust capability matrix](rust.md#capability-matrix) shows which components and official adapters
are exposed through each path.

## Related guides

- [Architecture](architecture.md) - Core components, threading, and dependency flow.
- [Rust](rust.md) - Native Rust APIs and runtime use.
- [Live trading](live.md) - LiveNode lifecycle and hosted event loops.
- [Backtesting](backtesting/) - Backtest engines, nodes, data, and venues.
- [Adapters](adapters.md) - Official adapter configuration and routing.
- [Migration from v1](../../MIGRATION_V2.md) - Python API changes and migration boundaries.
