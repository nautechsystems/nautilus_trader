![Nautech Systems](https://github.com/nautechsystems/nautilus_trader/blob/master/docs/artwork/nautech-systems-logo.png?raw=true "logo")

---

# NautilusTrader

[![codacy-quality](https://api.codacy.com/project/badge/Grade/a1d3ccf7bccb4483b091975681a5cb23)](https://app.codacy.com/gh/nautechsystems/nautilus_trader?utm_source=github.com&utm_medium=referral&utm_content=nautechsystems/nautilus_trader&utm_campaign=Badge_Grade_Dashboard)
![build](https://github.com/nautechsystems/nautilus_trader/workflows/build/badge.svg)
[![Documentation Status](https://readthedocs.org/projects/nautilus-trader/badge/?version=latest)](https://nautilus-trader.readthedocs.io/en/latest/?badge=latest)
![pypi-pythons](https://img.shields.io/pypi/pyversions/nautilus_trader)
![pypi-version](https://img.shields.io/pypi/v/nautilus_trader)
[![Downloads](https://pepy.tech/badge/nautilus-trader)](https://pepy.tech/project/nautilus-trader)
[![code-style: black](https://img.shields.io/badge/code%20style-black-000000.svg)](https://github.com/psf/black)

**BETA**

- **The API is under heavy construction with constant breaking changes**
- **There is currently a large effort to develop improved documentation**

![WIP](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/artwork/under-construction.png?raw=true "")

## Introduction

NautilusTrader is a high-performance algorithmic trading platform allowing quantitative traders
the ability to backtest portfolios of automated trading strategies on historical
data with an event-driven engine, and also trade those strategies live in a
production grade environment. The project heavily utilizes Cython to provide
type safety and performance through C extension modules. The libraries can be
accessed from both pure Python and Cython.

Cython is a compiled programming language that aims to be a superset of the
Python programming language, designed to give C-like performance with code that
is written mostly in Python with optional additional C-inspired syntax.

> https://cython.org

To run code or tests from the source code, first compile the C extensions for the package.

    $ python setup.py build_ext --inplace

## Features

- **Fast:** C level speed and type safety provided through Cython. Asynchronous networking with uvloop.
- **Reliable:** Redis or Postgres backed performant state persistence for the live `ExecutionEngine`.
- **Flexible:** Any FIX or REST API can be integrated into the platform, with no changes to your strategy scripts.
- **Backtesting:** Multiple instruments and strategies simultaneously with historical tick and/or bar data.
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

Latest version;

    $ pip install nautilus_trader

Development version (pre-release);

    $ pip install git+https://github.com/nautechsystems/nautilus_trader.git@develop

## Development

[Development Documentation](docs/development/)

[CONTRIBUTING.md](https://github.com/nautechsystems/nautilus_trader/blob/master/CONTRIBUTING.md)

We recommend the PyCharm Professional edition IDE as it interprets Cython syntax.
Unfortunately the Community edition will not interpret Cython syntax.

> https://www.jetbrains.com/pycharm/

To run code or tests from the source code, first compile the C extensions for the package.

    $ python setup.py build_ext --inplace

All tests can be run via the `run_tests.sh` script, or through pytest.

## Code Style

_Black_ is a PEP-8 compliant opinionated formatter.

> https://github.com/psf/black

We philosophically agree with _Black_, however it does not currently run over
Cython code. So you could say we are "handcrafting towards" _Blacks_ stylistic conventions.

---

Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.

> https://nautechsystems.io

![cython](https://github.com/nautechsystems/nautilus_trader/blob/master/docs/artwork/cython-logo.png?raw=true "cython")
