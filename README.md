![Nautech Systems](https://github.com/nautechsystems/nautilus_trader/blob/master/docs/artwork/nautech-systems-logo.png?raw=true "logo")

---

# NautilusTrader

![build](https://github.com/nautechsystems/nautilus_trader/workflows/build/badge.svg)
![docs-status](https://img.shields.io/netlify/b89a9b0d-d308-4dd3-9a3e-e9c0845ef175?label=docs)
[![codacy-quality](https://api.codacy.com/project/badge/Grade/a1d3ccf7bccb4483b091975681a5cb23)](https://app.codacy.com/gh/nautechsystems/nautilus_trader?utm_source=github.com&utm_medium=referral&utm_content=nautechsystems/nautilus_trader&utm_campaign=Badge_Grade_Dashboard)
[![codecov](https://codecov.io/gh/nautechsystems/nautilus_trader/branch/master/graph/badge.svg?token=DXO9QQI40H)](https://codecov.io/gh/nautechsystems/nautilus_trader)
![pypi-pythons](https://img.shields.io/pypi/pyversions/nautilus_trader)
![pypi-version](https://img.shields.io/pypi/v/nautilus_trader)
[![Downloads](https://pepy.tech/badge/nautilus-trader)](https://pepy.tech/project/nautilus-trader)
![pypi-format](https://img.shields.io/pypi/format/nautilus_trader?color=blue)
[![code-style: black](https://img.shields.io/badge/code%20style-black-000000.svg)](https://github.com/psf/black)

## Introduction

NautilusTrader is an open-source, high-performance, production-grade algorithmic trading platform,
providing quantitative traders with the ability to backtest portfolios of automated trading strategies
on historical data with an event-driven engine, and also deploy those same strategies live.

NautilusTrader is AI/ML first, designed to deploy models for algorithmic trading strategies developed
using the Python ecosystem - within a highly performant and robust Python native environment.

The platform aims to be universal, with any REST/FIX/WebSocket API able to be integrated via modular
adapters. Thus the platform can handle high-frequency trading operations for any asset classes
including FX, Equities, Futures, Options, CFDs, Crypto and Betting - across multiple venues simultaneously.

## Features

- **Fast:** C-level speed and type safety provided through Cython. Asynchronous networking utilizing uvloop.
- **Reliable:** Redis backed performant state persistence for live implementations.
- **Flexible:** Any FIX, REST or WebSocket API can be integrated into the platform.
- **Backtesting:** Multiple instruments and strategies simultaneously with historical quote tick, trade tick, bar and order book data.
- **Multi-venue:** Multiple venue capabilities facilitate market making and statistical arbitrage strategies.
- **AI Agent Training:** Backtest engine fast enough to be used to train AI trading agents (RL/ES).

## Why NautilusTrader?

One of the key value propositions of NautilusTrader is that it addresses the challenge of keeping
the research/backtest environment consistent with the production live trading environment.

Normally research and backtesting may be conducted in Python (or other suitable language), with
trading strategies traditionally then needing to be reimplemented in C++/C#/Java or other statically
typed language(s). The reasoning here is to enjoy the performance a compiled language can offer,
along with the tooling and support which has made these languages historically more suitable for
large enterprise systems.

The value of NautilusTrader here is that this re-implementation step is circumvented, as the
platform was designed from the ground up to hold its own in terms of performance and quality.

Python has simply caught up in performance (via Cython offering C-level speed) and general tooling,
making it a suitable language for building a large system such as this. The benefit being that a
Python native environment can be offered, suitable for professional quantitative traders and hedge
funds.

## Why Python?

Python was originally created decades ago as a simple scripting language with a clean straight
forward syntax. It has since evolved into a fully fledged general purpose object-oriented
programming language. Not only that, Python has become the _de facto lingua franca_ of data science,
machine learning, and artificial intelligence.

The language out of the box is not without its drawbacks however, especially in the context of
implementing large systems. Cython has addressed a lot of these issues, offering all the advantages
of a statically typed language, embedded into Pythons rich ecosystem of software libraries and
developer/user communities.

## What is Cython?

[Cython](https://cython.org) is a compiled programming language that aims to be a superset of the Python programming
language, designed to give C-like performance with code that is written mostly in Python with
optional additional C-inspired syntax.

The project heavily utilizes Cython to provide static type safety and increased performance
for Python through C [extension modules](https://docs.python.org/3/extending/extending.html). The vast majority of the production Python code is actually
written in Cython, however the libraries can be accessed from both pure Python and Cython.

## Values

- Reliability
- Performance
- Testability
- Modularity
- Maintainability
- Scalability

## Documentation

The documentation for the latest version of the package can be found at:

> https://docs.nautilustrader.io

![Architecture](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/artwork/architecture.png?raw=true "")

## Integrations

| Logo | ID | Status |
|:---:|:---:|:---:|
| [![interactive-brokers](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/artwork/ib-logo.png?raw=true)](https://interactivebrokers.com) | IB | ![status](https://img.shields.io/badge/Integration-in_progress-orange) |
| [![oanda](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/artwork/oanda-logo.png?raw=true)](https://oanda.com/) | OANDA | ![status](https://img.shields.io/badge/Integration-in_progress-orange) |
| [![ccxtpro](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/artwork/ccxtpro-logo.png?raw=true)](https://ccxt.pro/) | CCXT-`exchange_id` | ![status](https://img.shields.io/badge/Integration-testing-yellow) |
| [![binance](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/artwork/binance-logo.png?raw=true)](https://www.binance.com/) | BINANCE | ![status](https://img.shields.io/badge/Integration-planned-gray) |
| [![bitmex](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/artwork/bitmex-logo.png?raw=true)](https://www.bitmex.com/) | BITMEX | ![status](https://img.shields.io/badge/Integration-planned-gray) |
| [![betfair](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/artwork/betfair-logo.png?raw=true)](https://www.betfair.com/) | BETFAIR | ![status](https://img.shields.io/badge/Integration-testing-yellow) |

CCXT Pro is an algorithmic crypto-trading library which currently includes
integrations to 27 crypto exchanges https://github.com/ccxt/ccxt.

The adapter requires the `ccxtpro` package, which in turn requires a license.
See https://ccxt.pro for more information.

| CCXT Pro advanced execution clients |
|:---:|
| BINANCE |
| BITMEX |

Advanced execution clients include additional order management options such as
`post_only`, `hidden`, `reduce_only`, and all the `TimeInForce` options. These
advanced execution clients will be incrementally added to and additional help
from ccxtpro users is welcome!

The other CCXT Pro exchanges are available through the unified API with a more
limited order feature set including simple vanilla MARKET and LIMIT orders.

## Installation

The `master` branch will always reflect the code of the latest release version.

The package is tested against Python 3.7 - 3.9 on 64-bit Windows, MacOS and Linux.
We recommend running the platform with the latest stable version of Python, and
in a virtual environment to isolate the dependencies.

For UNIX machines, [pyenv](https://github.com/pyenv/pyenv) is the recommended tool for handling system wide
Python installations and virtual environments.

Installation can be achieved through _one_ of the following options;

#### From PyPI

To install the latest binary wheel (or sdist package) from PyPI, run:

    pip install -U nautilus_trader

#### From GitHub Release

To install a binary wheel from GitHub, first navigate to the latest release.

> https://github.com/nautechsystems/nautilus_trader/releases/latest/

Download the appropriate `.whl` for your operating system and Python version, then run:

    pip install <file-name>.whl

#### From Source

Installation from source requires Cython to compile the Python C extensions.

1. To install Cython, run:

        pip install -U Cython==3.0a6

2. Then to install NautilusTrader using `pip`, run:

        pip install -U git+https://github.com/nautechsystems/nautilus_trader

    **Or** clone the source with `git`, and install from the projects root directory by running:

        git clone https://github.com/nautechsystems/nautilus_trader
        cd nautilus_trader
        pip install .

Also, from v1.123.0 the following extras are separately available for installation.
- `betfair` for the Betfair integration.
- `ccxt` for the CCXT Pro integration.
- `docs` for building the documentation.
- `ib` for the Interactive Brokers integration.
- `oanda` for the OANDA integration.

For example to install with the `ccxt` extra using pip:

    pip install nautilus_trader[ccxt]

## Examples

Examples of both backtest and live trading launch scripts are available in the `examples` directory.
These can run through PyCharm, or by running:

    python <name_of_script>.py

## Data Types

The following market data types can be requested historically, and also subscribed to as live streams
when available from an exchange/broker, and implemented in an integrations adapter.

- `Instrument`
- `OrderBook` (L1, L2 and L3 if available. Streaming or interval snapshots)
- `QuoteTick`
- `TradeTick`
- `Bar`

The following `PriceType` options can be used for bar aggregations;
- `BID`
- `ASK`
- `MID`
- `LAST`

The following `BarAggregation` options are possible;
- `SECOND`
- `MINUTE`
- `HOUR`
- `DAY`
- `TICK`
- `VOLUME`
- `VALUE` (a.k.a Dollar bars)
- `TICK_IMBALANCE` (TBA)
- `TICK_RUNS` (TBA)
- `VOLUME_IMBALANCE` (TBA)
- `VOLUME_RUNS` (TBA)
- `VALUE_IMBALANCE` (TBA)
- `VALUE_RUNS` (TBA)

The price types and bar aggregations can be combined with step sizes >= 1 in any
way through `BarSpecification` objects. This enables maximum flexibility and now
allows alternative bars to be produced for live trading.

```
# BarSpecification examples
tick_bars   = BarSpecification(100, BarAggregation.TICK, PriceType.LAST)
time_bars   = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
volume_bars = BarSpecification(100, BarAggregation.VOLUME, PriceType.MID)
value_bars  = BarSpecification(1_000_000, BarAggregation.VALUE, PriceType.MID)
```

Bars can be either internally or externally aggregated (alternative bar types are
only available by internal aggregation). External aggregation is normally for
standard bar periods as available from the data client through an integrations
adapter.

Custom data types can also be requested through a users custom handler, and fed
back to the strategies `on_data` method.

## Order Types

The following order types are available (when possible on an exchange);

- `Market`
- `Limit`
- `StopMarket`
- `StopLimit`

More will be added in due course including `MarketIfTouched`, `LimitIfTouched`
and icebergs. Users are invited to open discussion issues to request specific
order types or features.

## Account Types

The following account types are available for both live and backtest environments;

- `Cash` single-currency (base currency).
- `Cash` multi-currency.
- `Margin` single-currency (base currency).
- `Margin` multi-currency.

## Development

For development we recommend using the PyCharm _Professional_ edition IDE, as it interprets Cython
syntax. Alternatively, you could use Visual Studio Code with a Cython extension.

[pyenv](https://github.com/pyenv/pyenv) is the recommended tool for handling Python installations and virtual environments.

[poetry](https://python-poetry.org/) is the preferred tool for handling all Python package and dev dependencies.

[pre-commit](https://pre-commit.com/) is used to automatically run various checks, auto-formatters and linting tools
at commit.

#### Environment Setup

The following steps are for UNIX-like systems, and only need to be completed once.

1. Install `poetry` by running:

        curl -sSL https://raw.githubusercontent.com/python-poetry/poetry/master/get-poetry.py | python -

2. Then install all Python package dependencies, and compile the C extensions by running:

        poetry install

3. Install the `pre-commit` package by running:

        pip install pre-commit

4. Setup the `pre-commit` hook which will then run automatically at commit by running:

        pre-commit install

#### Builds

Following any changes to `.pyx` or `.pxd` files, you can re-compile by running:

    poetry run python build.py

Refer to the [Developer Guide](https://docs.nautilustrader.io/developer_guide/overview.html) for further information.

## Contributing

Even as some issues are marked with the `help wanted` label - this does not imply
that help is _only_ wanted on those issues. The label indicates where 'extra attention'
is needed.

Involvement from the trading community is a goal for this project. All help is welcome!
Developers can open issues on GitHub to discuss proposed enhancements/changes, or
to make bug reports.

Please make all pull requests to the `develop` branch.

Refer to the [CONTRIBUTING.md](https://github.com/nautechsystems/nautilus_trader/blob/master/CONTRIBUTING.md) for further information.

## License

NautilusTrader is licensed under the LGPL v3.0 as found in the [LICENSE](https://github.com/nautechsystems/nautilus_trader/blob/master/LICENSE) file.

Contributors are also required to sign a standard Contributor License Agreement (CLA), which is administered automatically through [CLA Assistant](https://cla-assistant.io/).

---

Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.

> https://nautechsystems.io

![cython](https://github.com/nautechsystems/nautilus_trader/blob/master/docs/artwork/cython-logo.png?raw=true "cython")
