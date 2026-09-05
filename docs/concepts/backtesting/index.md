# Backtesting

Backtesting simulates trading against historical data using the same core system components used in
live trading: built-in engines, the `Cache`, the [MessageBus](../message_bus.md), `Portfolio`,
[Actors](../actors.md), [Strategies](../strategies.md),
[Execution Algorithms](../execution/algorithms.md), and user-defined modules.

A `BacktestEngine` processes a stream of historical data. When the stream is exhausted, the engine
produces results and performance metrics for analysis. NautilusTrader offers two API levels for
backtesting:

| API level  | Use when                                                                |
| ---------- | ----------------------------------------------------------------------- |
| High-level | You want `BacktestNode`, config objects, data catalogs, and batch runs. |
| Low-level  | You want direct `BacktestEngine` control and manual component setup.    |

The pages in this section describe the current Rust backtest engine and its Python package-root
API.

## Reading guide

The generated sidebar may sort these pages alphabetically. Use this order when reading the section
end to end:

| Step | Page                                                    | Use it for                                      |
| ---- | ------------------------------------------------------- | ----------------------------------------------- |
| 1    | [APIs and repeated runs](apis-and-runs.md)              | Choose API level, load data, and run batches.   |
| 2    | [Data and venues](data-and-venues.md)                   | Match data granularity with venue `book_type`.  |
| 3    | [Execution flow](execution-flow.md)                     | Understand sequencing, timers, and trade IDs.   |
| 4    | [Fill prices and matching](fill-prices-and-matching.md) | Understand deterministic matching behavior.     |
| 5    | [Trade execution](trade-execution.md)                   | Use trade ticks, aggressor sides, and queues.   |
| 6    | [Bar execution](bar-execution.md)                       | Use bars, OHLC sequencing, and bar timing.      |
| 7    | [Fill models](fill-models.md)                           | Configure slippage and probabilistic fills.     |
| 8    | [Accounts and margin](accounts-and-margin.md)           | Configure funding, balances, and margin models. |

## Related guides

- [Strategies](../strategies.md): Develop strategies to backtest.
- [Visualization](../visualization.md): Generate tearsheets from backtest results.
- [Reports](../reports.md): Analyze backtest performance data.
