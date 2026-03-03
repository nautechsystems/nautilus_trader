# Getting Started

To get started with NautilusTrader, you will need:

- A Python 3.12–3.14 environment with the `nautilus_trader` package installed.
- A way to run Python scripts or Jupyter notebooks for backtesting and/or live trading.

## Installation

How to install NautilusTrader on your machine.

## Quickstart

The **Quickstart** provides a step-by-step walk through for setting up your first backtest.

## Examples in repository

The [online documentation](https://nautilustrader.io/docs/latest/) shows just a subset of examples. For the full set, see this repository on GitHub.

The following table lists example locations ordered by recommended learning progression:

| Directory                   | Contains                                                                                                                    |
|:----------------------------|:----------------------------------------------------------------------------------------------------------------------------|
| [examples/](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples)                 | Fully runnable, self-contained Python examples.                                                                                     |
| [docs/tutorials/](../tutorials/)           | Jupyter notebook tutorials demonstrating common workflows.                                                                              |
| [docs/concepts/](../concepts/)            | Concept guides with concise code snippets illustrating key features. |
| [nautilus_trader/examples/](../../nautilus_trader/examples/) | Pure-Python examples of basic strategies, indicators, and execution algorithms.                                     |
| [tests/unit_tests/](../../tests/unit_tests/)         | Unit tests covering core functionality and edge cases.                      |

## Backtesting API levels

NautilusTrader provides two different API levels for backtesting:

| API Level      | Description                           | Characteristics                                                                                                                                                                                                                                                                                                                                                        |
|:---------------|:--------------------------------------|:-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| High-Level API | Uses `BacktestNode` and `TradingNode` | Recommended for production: easier transition to live trading; requires a Parquet-based data catalog. |
| Low-Level API  | Uses `BacktestEngine`                 | Intended for library development: no live-trading path; direct component access; may encourage non-live-compatible patterns. |

:::warning[One node per process]
Running multiple `BacktestNode` or `TradingNode` instances concurrently in the same process is not supported due to global singleton state.
Sequential execution with proper disposal between runs is supported.

See [Processes and threads](../concepts/architecture.md#processes-and-threads) for details.
:::

See the [Backtesting](../concepts/backtesting.md) guide for help choosing an API level.

### Backtest (low-level API)

This tutorial runs through how to load raw data (external to Nautilus) using data loaders and wranglers,
and then use this data with a `BacktestEngine` to run a single backtest.

### Backtest (high-level API)

This tutorial runs through how to load raw data (external to Nautilus) into the data catalog,
and then use this data with a `BacktestNode` to run a single backtest.

## Running in docker

Alternatively, you can download a self-contained dockerized Jupyter notebook server, which requires no setup or
installation. This is the fastest way to get up and running to try out NautilusTrader. Note that deleting the container will also delete any data.

- To get started, install docker:
  - Go to [Docker installation guide](https://docs.docker.com/get-docker/) and follow the instructions.
- From a terminal, download the latest image:
  - `docker pull ghcr.io/nautechsystems/jupyterlab:nightly --platform linux/amd64`
- Run the docker container, exposing the Jupyter port:
  - `docker run -p 8888:8888 ghcr.io/nautechsystems/jupyterlab:nightly`
- Open your web browser to `localhost:{port}`:
  - <http://localhost:8888>

:::warning
Examples use `log_level="ERROR"` because Nautilus logging exceeds Jupyter's stdout rate limit,
causing notebooks to hang at lower log levels.
:::
