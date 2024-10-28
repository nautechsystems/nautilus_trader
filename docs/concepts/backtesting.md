# Backtesting

:::info
We are currently working on this guide.
:::

Backtesting with NautilusTrader is a methodical simulation process that replicates trading
activities using a specific system implementation. This system is composed of various components
including the built-in engines, `Cache`, `MessageBus`, `Portfolio`, [Actors](advanced/actors.md), [Strategies](strategies.md), [Execution Algorithms](execution.md),
and other user-defined modules. The entire trading simulation is predicated on a stream of historical data processed by a
`BacktestEngine`. Once this data stream is exhausted, the engine concludes its operation, producing 
detailed results and performance metrics for in-depth analysis.

It's important to recognize that NautilusTrader offers two distinct API levels for setting up and conducting backtests:

- **High-level API**: Uses a `BacktestNode` and configuration objects (`BacktestEngine`s are used internally).
- **Low-level API**: Uses a `BacktestEngine` directly with more "manual" setup.

## Choosing an API level

Consider using the **low-level** API when:

- Your entire data stream can be processed within the available machine resources (e.g., RAM).
- You prefer not to store data in the Nautilus-specific Parquet format.
- You have a specific need or preference to retain raw data in its original format (e.g., CSV, binary, etc.).
- You require fine-grained control over the `BacktestEngine`, such as the ability to re-run backtests on identical datasets while swapping out components (e.g., actors or strategies) or adjusting parameter configurations.

Consider using the **high-level** API when:

- Your data stream exceeds available memory, requiring streaming data in batches.
- You want to leverage the performance and convenience of the `ParquetDataCatalog` for storing data in the Nautilus-specific Parquet format.
- You value the flexibility and functionality of passing configuration objects to define and manage multiple backtest runs across various engines simultaneously.

## Low-level API

The low-level API centers around a `BacktestEngine`, where inputs are initialized and added manually via a Python script.
An instantiated `BacktestEngine` can accept the following:

- Lists of `Data` objects, which are automatically sorted into monotonic order based on `ts_init`.
- Multiple venues, manually initialized.
- Multiple actors, manually initialized and added.
- Multiple execution algorithms, manually initialized and added.

This approach offers detailed control over the backtesting process, allowing you to manually configure each component.

## High-level API

The high-level API centers around a `BacktestNode`, which orchestrates the management of multiple `BacktestEngine` instances,
each defined by a `BacktestRunConfig`. Multiple configurations can be bundled into a list and processed by the node in one run.

Each `BacktestRunConfig` object consists of the following:

- A list of `BacktestDataConfig` objects.
- A list of `BacktestVenueConfig` objects.
- A list of `ImportableActorConfig` objects.
- A list of `ImportableStrategyConfig` objects.
- A list of `ImportableExecAlgorithmConfig` objects.
- An optional `ImportableControllerConfig` object.
- An optional `BacktestEngineConfig` object, with a default configuration if not specified.

## Data

Data provided for backtesting drives the execution flow. Since a variety of data types can be used,
it's crucial that your venue configurations align with the data being provided for backtesting.
Mismatches between data and configuration can lead to unexpected behavior during execution.

:::info
NautilusTrader is primarily designed and optimized for backtesting on order book or tick data, providing the highest
execution granularity and realism. If order book or tick data is unavailable or unsuitable, backtests
can also be run on bar data; however, users should note that this results in a loss of information and detail,
reducing execution precision and realism.
:::


## Venues

When initializing a venue for backtesting, you must specify its internal order `book_type` for execution processing from the following options:

- `L1_MBP`: Level 1 market-by-price (default). Only the top level of the order book is maintained.
- `L2_MBP`: Level 2 market-by-price. Order book depth is maintained, with a single order aggregated per price level.
- `L3_MBO`: Level 3 market-by-order. Order book depth is maintained, with all individual orders tracked as provided by the data.

:::note
The granularity of the data must match the specified order `book_type`. Nautilus cannot generate higher granularity data (L2 or L3) from lower-level data such as quotes, trades, or bars.
:::

:::warning
If you specify `L2_MBP` or `L3_MBO` as the venue’s `book_type`, all non-order book data (such as quotes, trades, and bars) will be ignored for execution processing.
This may cause orders to appear as though they are never filled. We are actively working on improved validation logic to prevent configuration and data mismatches.
:::

:::warning
When providing Level 2 or higher order book data, ensure that the `book_type` is updated to reflect the data's granularity.
Failing to do so will result in data aggregation: L2 data will be reduced to a single order per level, and L1 data will reflect only top-of-book levels.
:::
