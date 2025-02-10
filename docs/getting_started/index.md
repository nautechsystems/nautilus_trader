# Getting Started

To get started with NautilusTrader you will need the following:
- A Python environment with the `nautilus_trader` package installed.
- A way to launch Python scripts for backtesting and/or live trading (either from the command line, or Jupyter notebook etc).

## [Installation](installation.md)
The **Installation** guide will help to ensure that NautilusTrader is properly installed on your machine.

## [Quickstart](quickstart.md)
The **Quickstart** provides a step-by-step walk through for setting up your first backtest.

## Examples in repository

The examples presented in the [online documentation](https://nautilustrader.io/docs/latest/) cover only a portion of all
available examples. For a complete collection, we recommend downloading the [GitHub repository](https://github.com/nautechsystems/nautilus_trader).

The following table lists example locations ordered by recommended learning progression:

| Directory                   | Contains                                                                                                                    |
|:----------------------------|:----------------------------------------------------------------------------------------------------------------------------|
| `/examples`                 | Fully runnable self-contained examples.                                                                                     |
| `/docs/tutorials`           | Various examples in form of Jupyter notebooks.                                                                              |
| `/docs/concepts`            | Contains numerous small code snippets that provide an overview of available features, but examples are mostly not runnable. |
| `/nautilus_trader/examples` | Example implementations of basic strategies + indicators (in pure python) + algorithms.                                     |
| `/tests/unit_tests`         | Unit-tests can be useful when looking for specific implementation details not covered in the examples.                      |

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

## Running in docker
Alternatively, a self-contained dockerized Jupyter notebook server is available for download, which does not require any setup or
installation. This is the fastest way to get up and running to try out NautilusTrader. Bear in mind that any data will be
deleted when the container is deleted.

- To get started, install docker:
  - Go to [docker.com](https://docs.docker.com/get-docker/) and follow the instructions
- From a terminal, download the latest image
  - `docker pull ghcr.io/nautechsystems/jupyterlab:nightly --platform linux/amd64`
- Run the docker container, exposing the jupyter port:
  - `docker run -p 8888:8888 ghcr.io/nautechsystems/jupyterlab:nightly`
- Open your web browser to `localhost:{port}`
  - http://localhost:8888

:::info
NautilusTrader currently exceeds the rate limit for Jupyter notebook logging (stdout output),
this is why `log_level` in the examples is set to `ERROR`. If you lower this level to see
more logging then the notebook will hang during cell execution. A fix is currently
being investigated which involves either raising the configured rate limits for
Jupyter, or throttling the log flushing from Nautilus.

- https://github.com/jupyterlab/jupyterlab/issues/12845
- https://github.com/deshaw/jupyterlab-limit-output
:::
