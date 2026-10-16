# Backtest APIs and Repeated Runs

NautilusTrader provides a **low-level** `BacktestEngine` API for direct control and a **high-level**
`BacktestNode` API for catalog-backed, configurable runs.

## Choosing an API level

Use the low-level API when:

- The data fits in memory or you will stream batches manually.
- You want to load data from formats other than a Nautilus Parquet catalog.
- You need direct control over venues, instruments, actors, strategies, or execution algorithms.
- You want to rerun the same loaded data with selected components changed.

Use the high-level API when:

- The data lives in a `ParquetDataCatalog`.
- The data requires automatic chunked loading.
- You want one configuration object to describe and identify a catalog-backed run.
- You want each independent run to use a fresh engine.

## Low-level API

The low-level API centers on `BacktestEngine`. Create the engine with a
`BacktestEngineConfig`, then add venues, instruments, components, and data before calling `run()`:

```python
from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig

engine = BacktestEngine(BacktestEngineConfig())
engine.add_venue(...)
engine.add_instrument(instrument)
engine.add_strategy(strategy)
engine.add_data(data)
engine.run()
```

Each instrument ID is registered once. Adding the same definition again is a no-op that keeps the
existing matching engine, so resting orders, book state, and generated IDs are preserved. Adding a
different definition for a registered ID raises an error and leaves the engine unchanged. The
comparison ignores `ts_event` and `ts_init`.

### Loading data

Each `add_data()` call copies its input into an independent stream. The engine orders each stream
by replay timestamp and merges all streams chronologically during the run. Adding one batch per
instrument does not repeatedly sort a cumulative list:

```python
engine.add_data(instrument1_bars)
engine.add_data(instrument2_bars)
engine.add_data(instrument3_bars)
engine.run()
```

**Run readiness**: Keep the default `sort=True` unless a batching workflow deliberately manages
readiness. A call with `sort=False` marks the engine as not ready to run. Call `sort_data()` before
`run()` unless a later `add_data(..., sort=True)` call has restored readiness:

```python
engine.add_data(instrument1_bars, sort=False)
engine.add_data(instrument2_bars, sort=False)
engine.sort_data()
engine.run()
```

`sort_data()` marks the independently ordered streams as ready for replay. It is safe to call more
than once.

**Input ownership**: Clearing or modifying the original Python list after `add_data()` does not
change the loaded stream.

### Streaming batches manually

Use **streaming mode** when the complete dataset does not fit in memory:

```python
engine.add_strategy(strategy)

for batch in data_batches:
    engine.add_data(batch)
    engine.run(streaming=True)
    engine.clear_data()

engine.end()
```

`run(streaming=True)` pauses when the current data is exhausted. It does not finalize the trader or advance
timers beyond that batch. Call `end()` after the final batch. This flushes timers to the last run boundary,
invokes stop handlers, and produces the final result.

The low-level API does not expose a generator-based `add_data_iterator()` method. `BacktestNode` provides
automatic catalog chunking. Direct engine users stream with the loop above.

## High-level API

The high-level API centers on `BacktestNode`. Each `BacktestRunConfig` contains:

- One or more `BacktestVenueConfig` objects.
- One or more `BacktestDataConfig` objects.
- An optional `BacktestEngineConfig`.
- Optional chunk size, time bounds, exception handling, and disposal settings.

Build the node before adding strategies through its run-specific methods:

```python
from decimal import Decimal

from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.backtest import BacktestNode
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.execution import MakerTakerFeeModel
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookType
from nautilus_trader.model import NautilusDataType
from nautilus_trader.model import OmsType

venue = BacktestVenueConfig(
    name="SIM",
    oms_type=OmsType.HEDGING,
    account_type=AccountType.MARGIN,
    book_type=BookType.L1_MBP,
    starting_balances=["1_000_000 USD"],
    fee_model=MakerTakerFeeModel(
        maker_rate=Decimal("0"),
        taker_rate=Decimal("0"),
    ),
)
data = BacktestDataConfig(
    data_type=NautilusDataType.QuoteTick,
    catalog_path="/data/catalog",
    instrument_id=instrument_id,
)
config = BacktestRunConfig(
    venues=[venue],
    data=[data],
    engine=BacktestEngineConfig(),
    chunk_size=100_000,
)

node = BacktestNode([config])
node.build()
node.add_strategy_from_config(config.id, strategy_config)
results = node.run()
```

Fees belong to the venue configuration, not to instruments. Each venue needs an explicit `fee_model`,
including an explicit zero-rate model for a frictionless baseline. Two accounts can trade the same instrument
under different schedules without changing the instrument definition.

Set `chunk_size` to a value in `[1, 1_000_000]` to enable streaming. Leave it as `None` to load all
data at once.

`BacktestNode` also provides methods for adding actors and built-in strategies to a built run.

### Order book delta replay

`BacktestNode` replays `OrderBookDelta` catalog data as `OrderBookDeltas` batches by default in both
one-shot and streaming runs. It groups each instrument's deltas in catalog order and closes each
batch at the `F_LAST` delta, so the batch holds one complete book event.

Each batch takes its flags, sequence, and timestamps from its closing delta and replays at that
delta's `ts_init`.

Batching changes how the backtest processes book events:

- **Matching**: The matching engine applies a whole event before it matches orders. Thus, an order cannot fill
  against a book state that exists only partway through an event. Fills can differ from individual delta
  replay.
- **Callbacks**: Strategies receive each event in one `on_book_deltas` callback, even when the data
  engine's `buffer_deltas` is disabled.
- **Chunk size**: In streaming runs, `chunk_size` counts each batch as one replay item, so a chunk
  can hold more rows than `chunk_size`.

Each event must end with an `F_LAST` delta. If a query ends with an instrument's deltas still awaiting that
closing delta, the run fails with an error that names the instrument.

Time bounds and `filter_expr` select individual rows. Choose them to keep events complete: a bound that cuts
off the start of an event goes undetected.

Set `batch_deltas=False` to replay each delta individually:

```python
data = BacktestDataConfig(
    data_type=NautilusDataType.OrderBookDelta,
    catalog_path="/data/catalog",
    instrument_id=instrument_id,
    batch_deltas=False,
)
```

## Shutdown on error

Set `BacktestEngineConfig.shutdown_on_error=True` to request a normal shutdown when the Rust logger
emits an error record:

```python
from nautilus_trader.config import BacktestEngineConfig

config = BacktestEngineConfig(shutdown_on_error=True)
```

The backtest loop observes the request, stops the trader and engines, and returns results collected
up to that point. It does not abort the process.

With `shutdown_on_error=True`:

- Rust logger error records still request shutdown when component filters or `bypass_logging=True`
  suppress them.
- Python `logging.error(...)` calls do not request shutdown.

The trigger resets when a new kernel run starts. For final `on_stop` and command-settling behavior,
see [shutdown semantics](execution-flow.md#shutdown-semantics).

## Repeated runs

`BacktestEngine.reset()` returns trading state and loaded component state to their initial values.

**Cleared state**:

- Orders, positions, and account balances.
- Component runtime state.
- Engine counters and timestamps.

**Retained data and registrations**:

- Data added through `add_data()`.
- Registered instruments and venues, including venue configuration.
- Registered actors, strategies, and execution algorithms.

Instruments stay loaded because the default backtest cache configuration sets
`drop_instruments_on_reset=False`.

### Use fresh nodes for independent runs

`BacktestNode` accepts one `BacktestRunConfig`. To run independent configurations, create and
dispose one node at a time:

```python
configs = [
    BacktestRunConfig(...),
    BacktestRunConfig(...),
    BacktestRunConfig(...),
]
results = []

for config in configs:
    node = BacktestNode([config])
    try:
        results.extend(node.run())
    finally:
        node.dispose()
```

Build each node and register its strategies before `run()` when they are not supplied by a
controller.

### Reuse loaded data for parameter runs

Use `reset()` when runs should share loaded data and venue setup:

```python
engine = BacktestEngine(BacktestEngineConfig())
engine.add_venue(...)
engine.add_instrument(instrument)
engine.add_data(data)

engine.add_strategy(strategy1)
engine.run()

engine.reset()
engine.run()

engine.reset()
engine.clear_strategies()
engine.add_strategy(strategy2)
engine.run()
```

Call `clear_strategies()` before replacing a strategy instance. Use `clear_data()` only when the
next run should load a different dataset.
