# Rust

Nautilus has a complete Rust implementation under the `crates/` directory.
You can write actors, strategies, run backtests, and trade live without Python.
The domain model is shared across all paths, and the v2 PyO3 path runs
Python strategies on the Rust engine directly.

:::warning
The Rust API is under active development. Method signatures and trait
requirements may change between releases.
:::

## System implementations

Nautilus has three implementations. Understanding where each stands helps
you choose the right one for your use case.

- **v1 legacy**: Cython/Python classes under `nautilus_trader/`. Fully
  featured with the broadest component coverage.
- **v2 Rust**: Pure Rust under `crates/`. Runs without Python.
- **v2 PyO3**: Python user-components (actors, strategies) running on
  the Rust core via PyO3 bindings. Combines Python convenience with
  Rust engine performance.

### Capability matrix

| Component             | v1 legacy (Cython) | v2 Rust        | v2 PyO3 (Python on Rust) |
|-----------------------|--------------------|----------------|--------------------------|
| Strategy              | ✓                  | ✓              | ✓                        |
| Actor                 | ✓                  | ✓              | ✓                        |
| DataEngine            | ✓                  | ✓              | ✓                        |
| ExecutionEngine       | ✓                  | ✓              | ✓                        |
| RiskEngine            | ✓                  | ✓              | ✓                        |
| BacktestEngine        | ✓                  | ✓              | ✓                        |
| BacktestNode          | ✓                  | ✓              | ✓                        |
| LiveNode              | ✓                  | ✓              | ✓                        |
| OrderEmulator         | ✓                  | ✓              | ✓                        |
| Matching engine       | ✓                  | ✓              | ✓                        |
| Portfolio             | ✓                  | ✓              | ✓                        |
| Accounts              | ✓                  | ✓              | ✓                        |
| Cache                 | ✓                  | ✓              | ✓                        |
| MessageBus            | ✓                  | ✓              | ✓                        |
| Data catalog          | ✓                  | ✓              | ✓                        |
| Indicators            | ✓                  | ✓              | ✓                        |
| Exec algorithms       | TWAP               | TWAP           | TWAP                     |
| Controller            | ✓                  | -              | -                        |
| Tearsheets            | ✓                  | -              | -                        |
| Config serialization  | ✓                  | -              | -                        |

### Adapters

| Adapter             | v1 legacy (Cython) | v2 Rust | v2 PyO3 |
|---------------------|--------------------|---------|---------|
| Architect AX        | ✓                  | ✓       | ✓       |
| Betfair             | ✓                  | ✓       | ✓       |
| Binance             | ✓                  | ✓       | ✓       |
| BitMEX              | ✓                  | ✓       | ✓       |
| Bybit               | ✓                  | ✓       | ✓       |
| Databento           | ✓                  | ✓       | ✓       |
| Deribit             | ✓                  | ✓       | ✓       |
| dYdX                | ✓                  | ✓       | ✓       |
| Hyperliquid         | ✓                  | ✓       | ✓       |
| Interactive Brokers | ✓                  | -       | -       |
| Kraken              | ✓                  | ✓       | ✓       |
| OKX                 | ✓                  | ✓       | ✓       |
| Polymarket          | ✓                  | ✓       | ✓       |
| Sandbox             | ✓                  | ✓       | ✓       |
| Tardis              | ✓                  | ✓       | ✓       |

### Choosing a path

- **v1 legacy** is the most complete today. Use it if you need the
  Controller, tearsheets, Interactive Brokers, or config serialization.
- **v2 Rust** gives native performance without a Python runtime. All core
  trading functionality is available. Use it for latency-sensitive
  deployments or teams that prefer a compiled language.
- **v2 PyO3**: Python user-components (actors, strategies) run on the
  Rust core engine with Rust performance for data processing and
  execution, while keeping the Python authoring experience.

## Project setup

The Nautilus crates are published to
[crates.io](https://crates.io/crates/nautilus-backtest). Add them to your
`Cargo.toml`:

```toml
[dependencies]
nautilus-backtest = "0.59"
nautilus-common = "0.59"
nautilus-execution = "0.59"
nautilus-model = { version = "0.59", features = ["stubs"] }
nautilus-trading = { version = "0.59", features = ["examples"] }

anyhow = "1"
log = "0.4"
```

For live trading, add the live crate and the adapter for your venue:

```toml
[dependencies]
nautilus-live = "0.59"
nautilus-okx = "0.59"
```

To track the latest development branch, point all Nautilus dependencies at the
same git source to avoid type mismatches between crates.io and git versions:

```toml
[dependencies]
nautilus-backtest = { git = "https://github.com/nautechsystems/nautilus_trader.git", branch = "develop" }
nautilus-common = { git = "https://github.com/nautechsystems/nautilus_trader.git", branch = "develop" }
nautilus-execution = { git = "https://github.com/nautechsystems/nautilus_trader.git", branch = "develop" }
nautilus-model = { git = "https://github.com/nautechsystems/nautilus_trader.git", branch = "develop", features = ["stubs"] }
nautilus-trading = { git = "https://github.com/nautechsystems/nautilus_trader.git", branch = "develop", features = ["examples"] }
```

The minimum supported Rust version (MSRV) is **1.96.0**.

### Feature flags

| Flag             | Crate               | Effect                                                        |
|------------------|---------------------|---------------------------------------------------------------|
| `high-precision` | `nautilus-model`    | 16-digit fixed precision (default is 9). Required for crypto. |
| `stubs`          | `nautilus-model`    | Test instrument stubs (`audusd_sim`, etc.).                   |
| `examples`       | `nautilus-trading`  | Example strategies (`EmaCross`, `GridMarketMaker`).           |
| `streaming`      | `nautilus-backtest` | Catalog‑based data streaming via `BacktestNode`.              |
| `defi`           | `nautilus-model`    | DeFi data types. Implies `high-precision`.                    |

:::tip
Standard 9-digit precision handles most traditional finance instruments.
Enable `high-precision` for crypto venues where prices can have many decimal
places (e.g. `0.00000001`).
:::

## Actors

An actor receives market data, custom data/signals, and system events but does
not manage orders. Implement the `DataActor` trait and use `nautilus_actor!` to
wire your `DataActorCore` field into the runtime contract. Your type
implements or derives `Debug`; the macro supplies the native runtime wiring.
User code normally uses the `DataActor` facade methods for subscriptions,
cache access, and clock access.

### Handler methods

Override any handler on the `DataActor` trait to receive the corresponding
data or event. All handlers have default no-op implementations, so you only
override what you need.

| Handler                | Receives                  |
|------------------------|---------------------------|
| `on_start`             | Actor started.            |
| `on_stop`              | Actor stopped.            |
| `on_quote`             | `QuoteTick`               |
| `on_trade`             | `TradeTick`               |
| `on_bar`               | `Bar`                     |
| `on_book_deltas`       | `OrderBookDeltas`         |
| `on_book`              | `OrderBook` (at interval) |
| `on_instrument`        | `InstrumentAny`           |
| `on_mark_price`        | `MarkPriceUpdate`         |
| `on_index_price`       | `IndexPriceUpdate`        |
| `on_funding_rate`      | `FundingRateUpdate`       |
| `on_option_greeks`     | `OptionGreeks`            |
| `on_option_chain`      | `OptionChainSlice`        |
| `on_instrument_status` | `InstrumentStatus`        |
| `on_order_filled`      | `OrderFilled`             |
| `on_order_canceled`    | `OrderCanceled`           |
| `on_time_event`        | `TimeEvent`               |

For a step-by-step walkthrough, see the
[Write an Actor (Rust)](../how_to/write_rust_actor.md) how-to guide.
For a complete example, see
[`BookImbalanceActor`](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/trading/src/examples/actors/imbalance).

## Strategies

A strategy extends an actor with order management. Implement `DataActor` for
data handling and use `nautilus_strategy!` to wire your `StrategyCore` field
into the strategy runtime contract. `StrategyCore` stores the runtime strategy
state; normal strategy logic reaches it through facade methods on `self`.
Runtime registration requires the native wiring generated by the macro, but
normal strategy logic uses `Strategy` methods and the facade methods on `self`.

### Order management

The `Strategy` trait provides order methods through the facade:

| Method                | Action                                    |
|-----------------------|-------------------------------------------|
| `submit_order`        | Submit a new order to the venue.          |
| `submit_order_list`   | Submit a list of contingent orders.       |
| `modify_order`        | Modify price, quantity, or trigger price. |
| `cancel_order`        | Cancel a specific order.                  |
| `cancel_orders`       | Cancel a filtered set of orders.          |
| `cancel_all_orders`   | Cancel all orders for an instrument.      |
| `close_position`      | Close a position with a market order.     |
| `close_all_positions` | Close all open positions.                 |

The `OrderApi` (accessed via `self.order()`) builds orders and order lists:

- `generate_client_order_id`
- `generate_order_list_id`
- `market`
- `limit`
- `stop_market`
- `stop_limit`
- `market_to_limit`
- `market_if_touched`
- `limit_if_touched`
- `trailing_stop_market`
- `trailing_stop_limit`
- `bracket`
- `create_list`

### Core wiring macros

Rust actors, strategies, and execution algorithms keep their runtime core as a
struct field. The macros tell the traits where that field lives.

| Macro                                          | Core field               | Generates                       |
|------------------------------------------------|--------------------------|---------------------------------|
| `nautilus_actor!(Type)`                        | `DataActorCore`          | Runtime wiring.                 |
| `nautilus_strategy!(Type)`                     | `StrategyCore`           | Runtime wiring and `Strategy`.  |
| `nautilus_execution_algorithm!(Type, { ... })` | `ExecutionAlgorithmCore` | Runtime wiring and algorithm.   |

The macros expect a field named `core`; pass a field name as the second
argument when needed. They do not make the actor, strategy, or `StrategyCore`
deref to runtime internals.
The execution algorithm macro takes an `on_order()` implementation block because
that method defines the algorithm's required order handling.
Normal code uses facade methods such as:

- `actor_id()`
- `trader_id()`
- `is_registered()`
- `config()`
- `strategy_id()`
- `clock()`
- `cache()`
- `order()`
- `portfolio()`

### Native traits

Use facade methods by default:

- `actor_id()`
- `trader_id()`
- `is_registered()`
- `config()`
- `strategy_id()`
- `clock()`
- `cache()`
- `order()`
- `portfolio()`

`DataActorNative`, `StrategyNative`, and `ExecutionAlgorithmNative` are for
native-only access below that facade. This section documents host integration
and explicit latency-sensitive native Rust code, not the portable authoring
path.

| Authoring path            | Native traits?   | Normal API                          |
|---------------------------|------------------|-------------------------------------|
| Native Rust binary        | Only when needed | `Strategy` and `DataActor` facades. |
| Rust launched from Python | Only when needed | Same as native Rust.                |
| Python‑authored component | No               | Facades only.                       |
| Plug‑in‑compatible code   | No               | Facades only.                       |

Native traits expose borrowed core state, `Rc<RefCell<_>>`, and runtime
references. Use them when native Rust code intentionally accepts those borrow
rules for an explicit latency-sensitive path or host integration. Engine,
runtime, registration, PyO3, testkit, and plug-in host code can import
`DataActorNative`, `StrategyNative`, or `ExecutionAlgorithmNative` when they
need actor-core, strategy-core, or execution-algorithm-core access. Do not use
them in ordinary portable actor, strategy, or execution algorithm logic,
Python-authored components, or plug-in-compatible code, because those types do
not cross those boundaries.

Choose the smallest native handle and keep each borrow scoped. Use `order()`
for normal strategy order construction. Reach for
`order_factory()` only when native code needs the raw mutable factory borrow.

#### `DataActorNative` methods

| Native method | Return shape             | Use when                        |
|---------------|--------------------------|---------------------------------|
| `core()`      | `&DataActorCore`         | Read actor internals.           |
| `core_mut()`  | `&mut DataActorCore`     | Mutate actor internals.         |
| `clock_mut()` | `RefMut<'_, dyn Clock>`  | Need a mutable clock borrow.    |
| `clock_rc()`  | `Rc<RefCell<dyn Clock>>` | Store or pass the shared clock. |
| `cache_ref()` | `Ref<'_, Cache>`         | Need short live‑cache reads.    |
| `cache_rc()`  | `Rc<RefCell<Cache>>`     | Mutate, store, or pass cache.   |

#### `StrategyNative` methods

| Native method         | Return shape                 | Use when                          |
|-----------------------|------------------------------|-----------------------------------|
| `strategy_core()`     | `&StrategyCore`              | Read strategy internals.          |
| `strategy_core_mut()` | `&mut StrategyCore`          | Mutate strategy internals.        |
| `order_factory()`     | `RefMut<'_, OrderFactory>`   | Need raw mutable factory borrow.  |
| `order_factory_rc()`  | `Rc<RefCell<OrderFactory>>`  | Store or pass the factory.        |
| `portfolio_rc()`      | `Rc<RefCell<Portfolio>>`     | Store or pass the portfolio.      |

#### `ExecutionAlgorithmNative` methods

| Native method               | Return shape                   | Use when                              |
|-----------------------------|--------------------------------|---------------------------------------|
| `exec_algorithm_core()`     | `&ExecutionAlgorithmCore`      | Read execution algorithm internals.   |
| `exec_algorithm_core_mut()` | `&mut ExecutionAlgorithmCore`  | Mutate execution algorithm internals. |

For a step-by-step walkthrough, see the
[Write a Strategy (Rust)](../how_to/write_rust_strategy.md) how-to guide.
For complete examples, see
[`EmaCross`](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/trading/src/examples/strategies/ema_cross)
and
[`GridMarketMaker`](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/trading/src/examples/strategies/grid_mm).

### Running Rust components

Rust strategies and actors can run through three paths. The examples
below use strategies, but the same pattern applies to actors via
`add_actor` (pure Rust) and `add_native_actor` (from Python).

#### Pure Rust

Write your strategy and `main` function in Rust, then build a standalone
binary with `cargo build`. This path requires no Python runtime.

```rust
let strategy = GridMarketMaker::new(config);
node.add_strategy(strategy)?;
node.run().await?;
```

See [Run Live Trading (Rust)](../how_to/run_rust_live_trading.md) for a
full walkthrough.

#### Native config from Python

Pass a type name and config to `add_native_strategy` to register a
built-in Rust strategy from Python. The Rust side constructs the strategy
and registers it with the engine. Python provides the configuration; all
execution happens in Rust.

```python
from nautilus_trader.core.nautilus_pyo3.trading import GridMarketMakerConfig

config = GridMarketMakerConfig(
    instrument_id=InstrumentId.from_str("BTC-USDT-SWAP.OKX"),
    max_position=Quantity.from_str("10.0"),
    trade_size=Quantity.from_str("0.1"),
    num_levels=5,
    grid_step_bps=15,
)

node.add_native_strategy("GridMarketMaker", config)
```

Built-in strategy configs:

| Config                         | Strategy                 |
|--------------------------------|--------------------------|
| `CompositeMarketMakerConfig`   | `CompositeMarketMaker`   |
| `DeltaNeutralVolConfig`        | `DeltaNeutralVol`        |
| `EmaCrossConfig`               | `EmaCross`               |
| `ExecTesterConfig`             | `ExecTester`             |
| `GridMarketMakerConfig`        | `GridMarketMaker`        |
| `HurstVpinDirectionalConfig`   | `HurstVpinDirectional`   |

Built-in actor configs (via `add_native_actor`):

| Config                     | Actor                 |
|----------------------------|-----------------------|
| `BookImbalanceActorConfig` | `BookImbalanceActor`  |
| `DataTesterConfig`         | `DataTester`          |

Users who compile from source can add their own native components to this
path. Add a `#[pyclass]` config, a `register_*` function, and a match arm
in `native_strategy_register` or `native_actor_register`. The component
then works from Python without PyO3 wrappers on the type itself.

#### Plugin loading

Use `add_plugin` or `LiveNodeConfig.plugins` for Rust components built as
`cdylib` crates. The plug-in manifest supplies the component kind, so the
host needs only the library path, manifest type name, and instance config.

## Backtesting

For annotated walkthroughs of both APIs, see the
[Run a Backtest (Rust)](../how_to/run_rust_backtest.md) how-to guide.

### `BacktestEngine` (low-level API)

Construct the engine, add venues and instruments, load data, register
strategies, and run. See the full working example:

```bash
cargo run -p nautilus-backtest --features examples --example engine-ema-cross
```

Source:
[`crates/backtest/examples/engine_ema_cross.rs`](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/backtest/examples/engine_ema_cross.rs)

### `BacktestNode` (high-level API)

Loads data from a `ParquetDataCatalog` and supports streaming in
configurable chunk sizes. Requires the `streaming` feature on
`nautilus-backtest`. See the full working example:

```bash
cargo run -p nautilus-backtest --features examples,streaming --example node-ema-cross
```

Source:
[`crates/backtest/examples/node_ema_cross.rs`](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/backtest/examples/node_ema_cross.rs)

## Live trading

For an annotated walkthrough, see the
[Run Live Trading (Rust)](../how_to/run_rust_live_trading.md) how-to guide.

The `LiveNode` connects to real venues through adapter clients. The builder
pattern configures data and execution clients, then `run()` starts the async
event loop. Each adapter provides its own factory and config types.

| Adapter        | Example                                                  |
|----------------|----------------------------------------------------------|
| Architect AX   | `crates/adapters/architect_ax/examples/`                 |
| Betfair        | `crates/adapters/betfair/examples/`                      |
| Binance        | `crates/adapters/binance/examples/`                      |
| BitMEX         | `crates/adapters/bitmex/examples/`                       |
| Blockchain     | `crates/adapters/blockchain/examples/`                   |
| Bybit          | `crates/adapters/bybit/examples/`                        |
| Databento      | `crates/adapters/databento/examples/`                    |
| Deribit        | `crates/adapters/deribit/examples/`                      |
| dYdX           | `crates/adapters/dydx/examples/`                         |
| Hyperliquid    | `crates/adapters/hyperliquid/examples/`                  |
| Kraken         | `crates/adapters/kraken/examples/`                       |
| OKX            | `crates/adapters/okx/examples/`                          |
| Polymarket     | `crates/adapters/polymarket/examples/`                   |
| Sandbox        | `crates/adapters/sandbox/examples/`                      |
| Tardis         | `crates/adapters/tardis/examples/`                       |

Most adapters include `node_data_tester.rs` and `node_exec_tester.rs`
examples. These test data requests, streaming, and order execution
against live venues.

## Related guides

- [Write an Actor (Rust)](../how_to/write_rust_actor.md) - Step-by-step actor walkthrough.
- [Write a Strategy (Rust)](../how_to/write_rust_strategy.md) - Step-by-step strategy walkthrough.
- [Run a Backtest (Rust)](../how_to/run_rust_backtest.md) - BacktestEngine and BacktestNode usage.
- [Run Live Trading (Rust)](../how_to/run_rust_live_trading.md) - LiveNode setup and venue connection.
- [Architecture](architecture.md) - System design and data/execution flow.
- [Actors](actors.md) - Actor concepts (applies to both Python and Rust).
- [Strategies](strategies.md) - Strategy concepts and handler reference.
- [Events](events.md) - Event types and handler dispatch.
- [Backtesting](backtesting.md) - Backtest concepts and matching engine behavior.
