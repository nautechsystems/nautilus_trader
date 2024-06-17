# Getting Started

To get started with NautilusTrader you will need the following:
- A Python environment with the `nautilus_trader` package installed
- A way to launch Python scripts for backtesting and/or live trading (either from the command line, or Jupyter notebook etc)

## [Installation](installation.md)
The **Installation** guide will help to ensure that NautilusTrader is properly installed on your machine.

## [Quickstart](quickstart.md)
The **Quickstart** provides a step-by-step walk through for setting up your first backtest.

## Backtesting API levels

Backtesting involves running simulated trading systems on historical data.

To get started backtesting with NautilusTrader you need to first understand the two different API
levels which are provided, and which one may be more suitable for your intended use case.

:::info
For more information on which API level to choose, refer to the [Backtesting](../concepts/backtesting.md) guide.
:::

### [Backtest (low-level API)](backtest_low_level.md)
This tutorial runs through how to load raw data (external to Nautilus) using data loaders and wranglers,
and then use this data with a `BacktestEngine` to run a single backtest.

### [Backtest (high-level API)](backtest_high_level.md)
This tutorial runs through how to load raw data (external to Nautilus) into the data catalog,
and then use this data with a `BacktestNode` to run a single backtest.
