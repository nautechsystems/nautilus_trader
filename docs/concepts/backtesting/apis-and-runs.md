# Backtest APIs and repeated runs

## Choosing an API level

Consider using the **low-level** API when:

- Your entire data stream can be processed within the available machine resources (e.g., RAM).
- You prefer not to store data in the Nautilus-specific Parquet format.
- You have a specific need or preference to retain raw data in its original format (e.g., CSV,
  binary, etc.).
- You require fine-grained control over the `BacktestEngine`, such as the ability to re-run
  backtests on identical datasets while swapping out components (e.g., actors or strategies) or
  adjusting parameter configurations.

Consider using the **high-level** API when:

- Your data stream exceeds available memory, requiring streaming data in batches.
- You want the performance and convenience of the `ParquetDataCatalog` for storing data in the
  Nautilus-specific Parquet format.
- You value the flexibility and functionality of passing configuration objects to define and manage
  multiple backtest runs across various engines simultaneously.

## Low-level API

The low-level API centers around a `BacktestEngine`, where inputs are initialized and added manually
via a Python script. An instantiated `BacktestEngine` can accept the following:

- Lists of `Data` objects, which are automatically sorted into monotonic order based on `ts_init`.
- Multiple venues, manually initialized.
- Multiple actors, manually initialized and added.
- Multiple execution algorithms, manually initialized and added.

This approach offers detailed control over the backtesting process, allowing you to manually
configure each component.

### Loading large datasets efficiently

When working with large amounts of data across multiple instruments, the way you load data can
significantly impact performance.

#### The performance consideration

By default, `BacktestEngine.add_data()` sorts the entire data stream (existing data + newly added
data) on each call when `sort=True` (the default). This means:

- First call with 1M bars: sorts 1M bars.
- Second call with 1M bars: sorts 2M bars.
- Third call with 1M bars: sorts 3M bars.
- And so on...

This repeated sorting of increasingly large datasets can become a bottleneck when loading data for
multiple instruments.

#### Optimization strategies

**Strategy 1: Defer sorting until the end (recommended for multiple instruments)**

```python
from nautilus_trader.backtest.engine import BacktestEngine

engine = BacktestEngine()

# Setup venue and instruments
engine.add_venue(...)
engine.add_instrument(instrument1)
engine.add_instrument(instrument2)
engine.add_instrument(instrument3)

# Load all data WITHOUT sorting on each call
engine.add_data(instrument1_bars, sort=False)
engine.add_data(instrument2_bars, sort=False)
engine.add_data(instrument3_bars, sort=False)

# Sort once at the end - much more efficient!
engine.sort_data()

# Now run your backtest
engine.add_strategy(strategy)
engine.run()
```

**Strategy 2: Collect and add in a single batch**

```python
# Collect all data first
all_bars = []
all_bars.extend(instrument1_bars)
all_bars.extend(instrument2_bars)
all_bars.extend(instrument3_bars)

# Add once with sorting
engine.add_data(all_bars, sort=True)
```

**Strategy 3: Use streaming API for very large datasets**

For datasets that don't fit in memory, there are two streaming approaches:

**Automatic chunking** - supply a generator that yields batches. The engine pulls chunks lazily
during a single `run()` call:

```python
def data_generator():
    # Yield chunks of data (each chunk is a list of Data objects)
    yield load_chunk_1()
    yield load_chunk_2()
    yield load_chunk_3()


engine.add_data_iterator(
    data_name="my_data_stream",
    generator=data_generator(),
)

engine.run()  # Chunks are consumed on-demand
```

**Manual chunking** - load and run each batch yourself. This is the pattern used internally by
`BacktestNode` and gives full control over batch boundaries:

```python
engine.add_strategy(strategy)

for batch in data_batches:
    engine.add_data(batch)
    engine.run(streaming=True)
    engine.clear_data()

engine.end()  # Finalize: flushes remaining timers, stops engines, produces results
```

:::note
In streaming mode, timer advancement stops when data exhausts for each batch. Timers scheduled past
the last data point (e.g. bar aggregation intervals) are deferred until more data arrives or `end()`
is called, which flushes up to the `end` boundary from the last `run()` call.
:::

:::tip[Performance impact]
For a backtest with 10 instruments, each with 1M bars:

- Sorting on each call: ~10 sorts of increasing size (1M, 2M, 3M, ... 10M bars).
- Sorting once at the end: 1 sort of 10M bars.

The deferred sorting approach can be **significantly faster** for large datasets.
:::

### Data loading contract

The `BacktestEngine` enforces important invariants to ensure data integrity:

**Requirements:**

- All data must be sorted before calling `run()`.
- When using `sort=False`, you **must** call `sort_data()` before running.
- The engine validates this and raises `RuntimeError` if unsorted data is detected.
- Calling `sort_data()` multiple times is safe (idempotent).

**Safety guarantees:**

- Data lists are always copied internally to prevent external mutations from affecting engine state.
- You can safely clear or modify data lists after passing them to `add_data()`.
- Adding data with `sort=True` makes it immediately available for backtesting.

This design ensures data integrity while enabling performance optimizations for large datasets.

## High-level API

The high-level API centers around a `BacktestNode`, which orchestrates the management of multiple
`BacktestEngine` instances, each defined by a `BacktestRunConfig`. Multiple configurations can be
bundled into a list and processed by the node in one run.

Each `BacktestRunConfig` object consists of the following:

- A list of `BacktestDataConfig` objects.
- A list of `BacktestVenueConfig` objects.
- A list of `ImportableActorConfig` objects.
- A list of `ImportableStrategyConfig` objects.
- A list of `ImportableExecAlgorithmConfig` objects.
- An optional `ImportableControllerConfig` object.
- An optional `BacktestEngineConfig` object, with a default configuration if not specified.

## Shutdown on error

Set `BacktestEngineConfig.shutdown_on_error=True` so that a Rust error log ends the backtest run.
The Rust logger records the first `log::error!` emitted after the kernel starts, and the kernel
converts that trigger into a `ShutdownSystem` command the next time the backtest loop checks for
shutdown.

The shutdown request follows the normal backtest stop path. It stops the trader and engines, then
returns the backtest results collected up to the shutdown point. It does not abort the process. For
final `on_stop` and command-settling behavior, see
[shutdown semantics](execution-flow.md#shutdown-semantics).

```python
from nautilus_trader.backtest import BacktestEngineConfig

config = BacktestEngineConfig(shutdown_on_error=True)
```

Error logs suppressed by component filters or `bypass_logging=True` still request shutdown. The
trigger is cleared and re-armed when a new kernel run starts, so a process can run another backtest
without reinitializing the logging system. Shutdown-on-error observes Rust `log` records, not Python
`logging.error(...)` calls.

## Repeated runs

When conducting multiple backtest runs, it's important to understand how components reset to avoid
unexpected behavior.

### Resetting BacktestEngine

The `.reset()` method returns engine state and loaded component state to their **initial value**. It
keeps loaded components, data, instruments, and venues registered.

**What gets reset:**

- All trading state (orders, positions, account balances).
- Loaded actors, strategies, and execution algorithms are reset in place.
- Engine counters and timestamps.

**What persists:**

- Data added via `.add_data()` (use `.clear_data()` to remove).
- Instruments (must match the persisted data).
- Venue configurations.
- Loaded actors, strategies, and execution algorithms.

**Instrument handling:**

For `BacktestEngine`, instruments persist across resets by default (because data persists and
instruments must match data). This is configured via `CacheConfig.drop_instruments_on_reset=False`
in the default `BacktestEngineConfig`.

### Approaches for multiple backtest runs

There are two main approaches for running multiple backtests:

#### Use BacktestNode for production

The high-level API is designed for multiple backtest runs with different configurations:

```python
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.config import BacktestRunConfig

# Define multiple run configurations
configs = [
    BacktestRunConfig(...),  # Run 1
    BacktestRunConfig(...),  # Run 2
    BacktestRunConfig(...),  # Run 3
]

# Execute all runs
node = BacktestNode(configs=configs)
results = node.run()
```

Each run gets a fresh engine with clean state - no reset() needed.

#### Use BacktestEngine.reset

For fine-grained control with the low-level API:

```python
from nautilus_trader.backtest.engine import BacktestEngine

engine = BacktestEngine()

# Setup once
engine.add_venue(...)
engine.add_instrument(ETHUSDT)
engine.add_data(data)

# Run 1
engine.add_strategy(strategy1)
engine.run()

# Reset and run 2 with the same loaded strategy
engine.reset()
engine.run()

# Reset and run 3 with a different strategy
engine.reset()
engine.clear_strategies()
engine.add_strategy(strategy2)
engine.run()
```

:::note
Instruments and data persist across resets by default for `BacktestEngine`, making parameter
optimizations straightforward.
:::

:::tip[Best practices]

- **For production backtesting:** Use `BacktestNode` with configuration objects.
- **For parameter optimizations:** Use `BacktestEngine.reset()` to keep data and instruments,
  then call `clear_strategies()` before adding a replacement strategy instance.
- **For quick experiments:** Either approach works - choose based on individual use case.

:::
