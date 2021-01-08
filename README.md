![Nautech Systems](https://github.com/nautechsystems/nautilus_trader/blob/master/docs/artwork/nautech-systems-logo.png?raw=true "logo")

---

# NautilusTrader

[![codacy-quality](https://api.codacy.com/project/badge/Grade/a1d3ccf7bccb4483b091975681a5cb23)](https://app.codacy.com/gh/nautechsystems/nautilus_trader?utm_source=github.com&utm_medium=referral&utm_content=nautechsystems/nautilus_trader&utm_campaign=Badge_Grade_Dashboard)
![build](https://github.com/nautechsystems/nautilus_trader/workflows/build/badge.svg)
[![Documentation Status](https://readthedocs.org/projects/nautilus-trader/badge/?version=latest)](https://nautilus-trader.readthedocs.io/en/latest/?badge=latest)
[![codecov](https://codecov.io/gh/nautechsystems/nautilus_trader/branch/master/graph/badge.svg?token=DXO9QQI40H)](https://codecov.io/gh/nautechsystems/nautilus_trader)
![pypi-pythons](https://img.shields.io/pypi/pyversions/nautilus_trader)
![pypi-version](https://img.shields.io/pypi/v/nautilus_trader)
[![Downloads](https://pepy.tech/badge/nautilus-trader)](https://pepy.tech/project/nautilus-trader)
![pypi-format](https://img.shields.io/pypi/format/nautilus_trader)
[![code-style: black](https://img.shields.io/badge/code%20style-black-000000.svg)](https://github.com/psf/black)

**BETA**

- **The API is under heavy construction with constant breaking changes**
- **There is currently a large effort to develop improved documentation**

![WIP](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/artwork/under-construction.png?raw=true "")

## Introduction

_NautilusTrader_ is an open-source, high-performance, production-grade algorithmic trading platform,
providing quantitative traders with the ability to backtest portfolios of automated trading strategies
on historical data with an event-driven engine, and also deploy those same strategies live.

The platform aims to be universal, with any REST/FIX/WebSockets API able to be integrated via modular adapters.
Thus the platform can handle high-frequency trading operations for any asset classes including FX, Equities,
Futures, Options, CFDs and Crypto - across multiple venues simultaneously.

## Cython
The project heavily utilizes Cython, which provides static type safety and performance through C extension modules.
The libraries can be accessed from both pure Python and Cython.

Cython is a compiled programming language that aims to be a superset of the
Python programming language, designed to give C-like performance with code that
is written mostly in Python with optional additional C-inspired syntax.

> https://cython.org

## Documentation

The documentation for the latest version of the package is available at _readthedocs_.

> https://nautilus-trader.readthedocs.io

![Architecture](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/artwork/architecture.png?raw=true "")

## Features

- **Fast:** C-level speed and type safety provided through Cython. Asynchronous networking utilizing uvloop.
- **Reliable:** Redis backed performant state persistence for live implementations.
- **Flexible:** Any FIX, REST or WebSockets API can be integrated into the platform.
- **Backtesting:** Multiple instruments and strategies simultaneously with historical quote tick, trade tick and bar data.
- **Multi-venue:** Multiple venue capabilities allows market making and statistical arbitrage strategies.
- **AI Agent Training:** Backtest engine fast enough to be used to train AI trading agents (RL/ES).

## Values

- Reliability
- Testability
- Performance
- Modularity
- Maintainability
- Scalability

## Installation

The latest version is tested against Python 3.7 - 3.9 on Linux and MacOS.
Users are encouraged to use the latest stable version of Python.

We recommend you setup a virtual environment to isolate the dependencies.

To install the latest package from PyPI, run:

    pip install -U nautilus_trader

Alternatively, to install from source using pip, run:

    pip install .

## CCXT Pro Integration
An integration adapter for CCXT Pro is currently under development.
The adapter requires the `ccxtpro` package, which in turn requires a license.

See https://ccxt.pro for more information.


## Development

We recommend the PyCharm _Professional_ edition IDE as it interprets Cython syntax.

> https://www.jetbrains.com/pycharm/

You could also use Visual Studio Code with a Cython plugin.

[Poetry](https://python-poetry.org/) is the preferred tool for handling all package and dev dependencies.

To install all dependencies and compile the C extensions, run:

    poetry install

Following any changes to `.pyx` and `.pxd` files, you can recompile by running:

    python build.py

Refer to the [Developer Guide](https://nautilus-trader.readthedocs.io/en/latest/developer_guide/overview.html) for further information.

## Contributing

Involvement from the trading community is a goal for this project. All help is welcome!
Developers can open issues on GitHub to discuss proposed enhancements/changes, or
to make bug reports.

Please make all pull requests to the `develop` branch.

Refer to the [CONTRIBUTING.md](https://github.com/nautechsystems/nautilus_trader/blob/master/CONTRIBUTING.md) for further information.

## License

_NautilusTrader_ is licensed under the LGPL v3.0 as found in the [LICENSE](https://github.com/nautechsystems/nautilus_trader/blob/master/LICENSE) file.

Contributors are also required to sign a standard Contributor License Agreement (CLA), which is administered automatically through CLAassistant.

---

Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.

> https://nautechsystems.io

![cython](https://github.com/nautechsystems/nautilus_trader/blob/master/docs/artwork/cython-logo.png?raw=true "cython")
