# <img src="https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/_images/nautilus-trader-logo.png" width="500">

[![codacy-quality](https://api.codacy.com/project/badge/Grade/a1d3ccf7bccb4483b091975681a5cb23)](https://app.codacy.com/gh/nautechsystems/nautilus_trader?utm_source=github.com&utm_medium=referral&utm_content=nautechsystems/nautilus_trader&utm_campaign=Badge_Grade_Dashboard)
[![codecov](https://codecov.io/gh/nautechsystems/nautilus_trader/branch/master/graph/badge.svg?token=DXO9QQI40H)](https://codecov.io/gh/nautechsystems/nautilus_trader)
![pythons](https://img.shields.io/pypi/pyversions/nautilus_trader)
![pypi-version](https://img.shields.io/pypi/v/nautilus_trader)
![pypi-format](https://img.shields.io/pypi/format/nautilus_trader?color=blue)
[![Downloads](https://pepy.tech/badge/nautilus-trader)](https://pepy.tech/project/nautilus-trader)
[![discord](https://img.shields.io/discord/924497682343550976?color=768AD4&label=discord)](https://discord.gg/AUWVs3XaCS)

| Branch    | Version | Status |
|:----------|:--------|:-------|
| `master`  | ![version](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fnautechsystems%2Fnautilus_trader%2Fmaster%2Fversion.json) | [![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml) |
| `develop` | ![version](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fnautechsystems%2Fnautilus_trader%2Fdevelop%2Fversion.json) | [![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=develop)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml) |

| Platform         | Rust      | Python |
|:-----------------|:----------|:-------|
| Linux (x86_64)   | `1.58.1+` | `3.8+` |
| macOS (x86_64)   | `1.58.1+` | `3.8+` |
| Windows (x86_64) | `1.58.1+` | `3.8+` |

- **Website:** https://nautilustrader.io
- **Docs:** https://docs.nautilustrader.io
- **Support:** [support@nautilustrader.io](mailto:support@nautilustrader.io)

## Introduction

NautilusTrader is an open-source, high-performance, production-grade algorithmic trading platform,
providing quantitative traders with the ability to backtest portfolios of automated trading strategies
on historical data with an event-driven engine, and also deploy those same strategies live.

The platform is 'AI-first', designed to deploy models for algorithmic trading strategies developed
using the Python ecosystem - within a highly performant and robust Python native environment.
This helps to address the challenge of keeping the research/backtest environment consistent with the production
live trading environment.

NautilusTraders design, architecture and implementation philosophy holds software correctness and safety at the
highest level, with the aim of supporting Python native, mission-critical, trading system backtesting
and live deployment workloads.

The platform is also universal and asset class agnostic - with any REST, WebSocket or FIX API able to be integrated via modular
adapters. Thus, it can handle high-frequency trading operations for any asset classes
including FX, Equities, Futures, Options, CFDs, Crypto and Betting - across multiple venues simultaneously.

## Features

- **Fast:** C-level speed through Cython. Asynchronous networking with [uvloop](https://github.com/MagicStack/uvloop).
- **Reliable:** Type safety through Cython. Redis backed performant state persistence.
- **Flexible:** OS independent, runs on Linux, macOS, Windows. Deploy using Docker.
- **Integrated:** Modular adapters mean any REST, WebSocket, or FIX API can be integrated.
- **Advanced:** Time-in-force options `GTD`, `IOC`, `FOK` etc, advanced order types and triggers, `post-only`, `reduce-only`, and icebergs. Contingency order lists including `OCO`, `OTO` etc.
- **Backtesting:** Run with multiple venues, instruments and strategies simultaneously using historical quote tick, trade tick, bar, order book and custom data with nanosecond resolution.
- **Live:** Use identical strategy implementations between backtesting and live deployments.
- **Multi-venue:** Multiple venue capabilities facilitate market making and statistical arbitrage strategies.
- **AI Agent Training:** Backtest engine fast enough to be used to train AI trading agents (RL/ES).
- **Distributed:** Run backtests synchronously or as a graph distributed across a [dask](https://dask.org/) cluster.

![Alt text](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/_images/nautilus-art.png?raw=true "nautilus")
> *nautilus - from ancient Greek 'sailor' and naus 'ship'.*
>
> *The nautilus shell consists of modular chambers with a growth factor which approximates a logarithmic spiral.
> The idea is that this can be translated to the aesthetics of design and architecture.*

## Why NautilusTrader?

Traditionally, trading strategy research and backtesting might be conducted in Python (or other suitable language), with
the models and/or strategies then needing to be reimplemented in C, C++, C#, Java or other statically
typed language(s). The reasoning here is to utilize the performance and type safety a compiled language can offer,
which has historically made these languages more suitable for large trading systems.

The value of NautilusTrader here is that this reimplementation step is circumvented - as the critical core components of the platform
have all been written entirely in Cython. Because Cython can generate efficient C code (which then compiles to C extension modules as native binaries),
Python can effectively be used as a high-performance systems programming language - with the benefit being that a Python native environment can be offered which is suitable for
professional quantitative traders and trading firms.

## Why Python?

Python was originally created decades ago as a simple scripting language with a clean straight
forward syntax. It has since evolved into a fully fledged general purpose object-oriented
programming language. Based on the TIOBE index, Python is currently the most popular programming language in the world.
Not only that, Python has become the _de facto lingua franca_ of data science, machine learning, and artificial intelligence.

The language out of the box is not without its drawbacks however, especially in the context of
implementing large performance-critical systems. Cython has addressed a lot of these issues, offering all the advantages
of a statically typed language, embedded into Pythons rich ecosystem of software libraries and
developer/user communities.

## What is Cython?

[Cython](https://cython.org) is a compiled programming language which aims to be a superset of the Python programming
language, designed to give C-like performance with code that is written in Python - with
optional C-inspired syntax.

The project heavily utilizes Cython to provide static type safety and increased performance
for Python through [C extension modules](https://docs.python.org/3/extending/extending.html). The vast majority of the production code is actually
written in Cython, however the libraries can be accessed from both pure Python and Cython.

## What is Rust?

[Rust](https://www.rust-lang.org/) is a multi-paradigm programming language designed for performance and safety, especially safe
concurrency. Rust is blazingly fast and memory-efficient (comparable to C and C++) with no runtime or
garbage collector. It can power mission-critical systems, run on embedded devices, and easily
integrates with other languages.

Rust’s rich type system and ownership model guarantees memory-safety and thread-safety deterministically —
eliminating many classes of bugs at compile-time.

The project increasingly utilizes Rust for core performance-critical components. Python language binding is handled through
Cython, with static libraries linked at compile-time before the wheel binaries are packaged, so a user
does not need to have Rust installed to run NautilusTrader. In the future as more Rust code is introduced,
[PyO3](https://pyo3.rs/v0.15.1/) will be leveraged for easier Python bindings. It is expected that eventually all Cython will
be eliminated from the codebase.

## Architecture (data flow)

![Architecture](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/_images/architecture-overview.png?raw=true "architecture")

## Quality Attributes

- Reliability
- Performance
- Modularity
- Testability
- Maintainability
- Deployability

## Integrations

NautilusTrader is designed in a modular way to work with 'adapters' which provide
connectivity to data publishers and/or trading venues - converting their raw API
into a unified interface. The following integrations are currently supported:

| Name                                                    | ID      | Type                    | Status                                                  | Docs                                                              |
|:--------------------------------------------------------|:--------|:------------------------|:--------------------------------------------------------|:------------------------------------------------------------------|
[Betfair](https://betfair.com)                            | BETFAIR | Sports Betting Exchange | ![status](https://img.shields.io/badge/beta-yellow)     | [Guide](https://docs.nautilustrader.io/integrations/betfair.html) |
[Binance](https://binance.com)                            | BINANCE | Crypto Exchange         | ![status](https://img.shields.io/badge/beta-yellow)     | [Guide](https://docs.nautilustrader.io/integrations/binance.html) |
[Binance US](https://binance.us)                          | BINANCE | Crypto Exchange         | ![status](https://img.shields.io/badge/beta-yellow)     | [Guide](https://docs.nautilustrader.io/integrations/binance.html) |
[Binance Futures](https://www.binance.com/en/futures)     | BINANCE | Crypto Exchange         | ![status](https://img.shields.io/badge/building-orange) | [Guide](https://docs.nautilustrader.io/integrations/binance.html) |
[FTX](https://ftx.com)                                    | FTX     | Crypto Exchange         | ![status](https://img.shields.io/badge/beta-yellow)     | [Guide](https://docs.nautilustrader.io/integrations/ftx.html)     |
[FTX US](https://ftx.us)                                  | FTX     | Crypto Exchange         | ![status](https://img.shields.io/badge/beta-yellow)     | [Guide](https://docs.nautilustrader.io/integrations/ftx.html)     |
[Interactive Brokers](https://www.interactivebrokers.com) | IB      | Brokerage (multi-venue) | ![status](https://img.shields.io/badge/building-orange) | [Guide](https://docs.nautilustrader.io/integrations/ib.html)      |

Refer to the [Integrations](https://docs.nautilustrader.io/integrations/index.html) documentation for further details.

## Installation

### From PyPI

We recommend running the platform with the latest stable version of Python, and in a virtual environment to isolate the dependencies.

To install the latest binary wheel from PyPI:

    pip install -U nautilus_trader

To install `numpy` and `scipy` on ARM architectures such as MacBook Pro M1 / Apple Silicon, [this stackoverflow thread](https://stackoverflow.com/questions/65745683/how-to-install-scipy-on-apple-silicon-arm-m1)
is useful.

### From Source
Installation from source requires the latest stable `rustc` and `cargo` to compile the Rust libraries.
For the Python part, it's possible to install from source using `pip` if you first install the build dependencies
as specified in the `pyproject.toml`. However, we highly recommend installing using [poetry](https://python-poetry.org/) as below.

1. Install [rustup](https://rustup.rs/) (the Rust toolchain installer):
   - Linux and macOS:
       ```
       curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
       ```
   - Windows:
       - Download and install [`rustup-init.exe`](https://win.rustup.rs/x86_64)
       - Install "Desktop development with C++" with [Build Tools for Visual Studio 2019](https://visualstudio.microsoft.com/thank-you-downloading-visual-studio/?sku=BuildTools&rel=16)

2. Enable `cargo` in the current shell:
   - Linux and macOS:
       ```
       source $HOME/.cargo/env
       ```
   - Windows:
     - Start a new PowerShell

3. Install poetry (or follow the installation guide on their site):

       curl -sSL https://raw.githubusercontent.com/python-poetry/poetry/master/get-poetry.py | python -

4. Clone the source with `git`, and install from the projects root directory:

       git clone https://github.com/nautechsystems/nautilus_trader
       cd nautilus_trader
       poetry install --no-dev

Refer to the [Installation Guide](https://docs.nautilustrader.io/getting_started/installation.html) for other options and further details.

## Versioning and releases

NautilusTrader is currently following a bi-weekly beta release schedule.
The API is becoming more stable, however breaking changes are still possible between releases.
Documentation of these changes in the release notes are made on a best-effort basis.

The `master` branch will always reflect the source code for the latest released version.

The `develop` branch is normally very active with frequent commits, we aim to maintain a stable 
passing build on this branch.

The current roadmap has a goal of achieving a stable API for a `2.x` version. From this
point we will follow a more formal process for releases, with deprecation periods for any API changes.

## Makefile

A `Makefile` is provided to automate most installation and build tasks. It provides the following targets:
- `make install` -- Installs the package using poetry.
- `make build` -- Runs the Cython build script.
- `make clean` -- Cleans all none source artifacts from the repository.
- `make docs` -- Builds the documentation HTML using Sphinx.
- `make pre-commit` -- Runs the pre-commit checks over all files.

## Examples

Indicators and strategies can be developed in both Python and Cython (although if performance and latency sensitivity is import we recommend Cython).
The below are some examples of this:
- [indicator](/examples/indicators/ema_py.py) example written in Python.
- [indicator](/nautilus_trader/indicators/) examples written in Cython.
- [strategy](/nautilus_trader/examples/strategies/) examples written in both Python and Cython.
- [backtest](/examples/backtest/) examples using a `BacktestEngine` directly.

## Interactive Backtest Example

[Binder](https://mybinder.org) can launch Jupyter Notebooks from source repositories.
You can access a complete running backtest example through Binder here!
*Please note that currently Binder needs to build the platform from scratch which
can take a significant amount of time. We are working to optimize this.*

[![Binder](https://mybinder.org/badge_logo.svg)](https://mybinder.org/v2/gh/nautechsystems/nautilus_trader/develop?filepath=docs/getting_started/quick_start.ipynb)

## Minimal Strategy

The following is a minimal EMA Cross strategy example which just uses bar data.
While trading strategies can become very advanced with this platform, it's still possible to put
together simple strategies. First inherit from the `TradingStrategy` base class, then only the
methods which are required by the strategy need to be implemented.

```python
class EMACross(TradingStrategy):
    """
    A simple moving average cross example strategy.

    When the fast EMA crosses the slow EMA then enter a position at the market
    in that direction.

    Cancels all orders and flattens all positions on stop.
    """

    def __init__(self, config: EMACrossConfig):
        super().__init__(config)

        # Configuration
        self.instrument_id = InstrumentId.from_str(config.instrument_id)
        self.bar_type = BarType.from_str(config.bar_type)
        self.trade_size = Decimal(config.trade_size)

        # Create the indicators for the strategy
        self.fast_ema = ExponentialMovingAverage(config.fast_ema_period)
        self.slow_ema = ExponentialMovingAverage(config.slow_ema_period)

        self.instrument: Optional[Instrument] = None  # Initialized in on_start

    def on_start(self):
        """Actions to be performed on strategy start."""
        # Get instrument
        self.instrument = self.cache.instrument(self.instrument_id)

        # Register the indicators for updating
        self.register_indicator_for_bars(self.bar_type, self.fast_ema)
        self.register_indicator_for_bars(self.bar_type, self.slow_ema)

        # Get historical data
        self.request_bars(self.bar_type)

        # Subscribe to live data
        self.subscribe_bars(self.bar_type)

    def on_bar(self, bar: Bar):
        """Actions to be performed when the strategy receives a bar."""
        # BUY LOGIC
        if self.fast_ema.value >= self.slow_ema.value:
            if self.portfolio.is_flat(self.instrument_id):
                self.buy()
            elif self.portfolio.is_net_short(self.instrument_id):
                self.flatten_all_positions(self.instrument_id)
                self.buy()
        # SELL LOGIC
        elif self.fast_ema.value < self.slow_ema.value:
            if self.portfolio.is_flat(self.instrument_id):
                self.sell()
            elif self.portfolio.is_net_long(self.instrument_id):
                self.flatten_all_positions(self.instrument_id)
                self.sell()

    def buy(self):
        """Users simple buy method (example)."""
        order: MarketOrder = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(self.trade_size),
        )

        self.submit_order(order)

    def sell(self):
        """Users simple sell method (example)."""
        order: MarketOrder = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(self.trade_size),
        )

        self.submit_order(order)

    def on_stop(self):
        """Actions to be performed when the strategy is stopped."""
        # Cleanup orders and positions
        self.cancel_all_orders(self.instrument_id)
        self.flatten_all_positions(self.instrument_id)

        # Unsubscribe from data
        self.unsubscribe_bars(self.bar_type)

    def on_reset(self):
        """Actions to be performed when the strategy is reset."""
        # Reset indicators here
        self.fast_ema.reset()
        self.slow_ema.reset()

```

## Development

We aim to provide the most pleasant developer experience possible for this hybrid codebase of Python, Cython and Rust.
Please refer to the [Developer Guide](https://docs.nautilustrader.io/developer_guide/index.html) for helpful information.

## Contributing

Involvement from the trading community is a goal for this project, all help is welcome!
Developers can open [issues](https://github.com/nautechsystems/nautilus_trader/issues) on GitHub to discuss proposed enhancements, changes, or
to make bug reports. Questions and more general thoughts are best directed to a [discussions](https://github.com/nautechsystems/nautilus_trader/discussions) thread.

Refer to the [CONTRIBUTING.md](https://github.com/nautechsystems/nautilus_trader/blob/master/CONTRIBUTING.md) for further information.

Please make all pull requests to the `develop` branch.

## Community
Chat with contributors and active users of NautilusTrader on our [Discord](https://discord.gg/AUWVs3XaCS) server!

## License

NautilusTrader is licensed under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

Contributors are also required to sign a standard Contributor License Agreement (CLA), which is administered automatically through [CLA Assistant](https://cla-assistant.io/).

---

Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
https://nautechsystems.io

![nautechsystems](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/_images/ns-logo.png?raw=true "nautechsystems")
<img src="https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/_images/ferris.png" width="128">
