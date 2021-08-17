![Nautech Systems](https://github.com/nautechsystems/nautilus_trader/blob/master/docs/artwork/nautech-systems-logo.png?raw=true "logo")

---

# NautilusTrader

![build](https://github.com/nautechsystems/nautilus_trader/workflows/build/badge.svg)
![docs](https://github.com/nautechsystems/nautilus_trader/workflows/docs/badge.svg)
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

The platform aims to be universal - with any REST/WebSocket/FIX API able to be integrated via modular
adapters. Thus the platform can handle high-frequency trading operations for any asset classes
including FX, Equities, Futures, Options, CFDs, Crypto and Betting - across multiple venues simultaneously.

## Features

- **Fast:** C-level speed and type safety provided through Cython. Asynchronous networking utilizing uvloop.
- **Reliable:** Redis backed performant state persistence for live implementations.
- **Flexible:** Any REST/WebSocket/FIX API can be integrated into the platform.
- **Backtesting:** Multiple instruments and strategies simultaneously with historical quote tick, trade tick, bar and order book data.
- **Multi-venue:** Multiple venue capabilities facilitate market making and statistical arbitrage strategies.
- **AI Agent Training:** Backtest engine fast enough to be used to train AI trading agents (RL/ES).

## Documentation

> https://docs.nautilustrader.io

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
for Python through [C extension modules](https://docs.python.org/3/extending/extending.html). The vast majority of the production code is actually
written in Cython, however the libraries can be accessed from both pure Python and Cython.

## Architecture Quality Attributes

- Reliability
- Testability
- Performance
- Modularity
- Maintainability
- Deployability

![Architecture](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/artwork/architecture.png?raw=true "")

## Integrations

NautilusTrader is designed to work with modular adapters which provide integrations with data
publishers and/or trading venues (exchanges/brokers).

Refer to the [integrations](https://docs.nautilustrader.io/integrations) documentation.

## Installation

To install the latest binary wheel (or sdist package) from PyPI:

    pip install -U nautilus_trader

Refer to the [installation guide](https://docs.nautilustrader.io/getting-started/installation).

## Examples

Examples of both backtest and live trading launch scripts are available in the `examples` directory.

## Development

We aim to make the developer experience for this hybrid codebase of Cython and Python
as pleasant as possible. Please refer to the [developer guide](https://docs.nautilustrader.io/developer-guide) for helpful information.

## Contributing

Involvement from the trading community is a goal for this project. All help is welcome!
Developers can open issues on GitHub to discuss proposed enhancements/changes, or
to make bug reports.

Refer to the [CONTRIBUTING.md](https://github.com/nautechsystems/nautilus_trader/blob/master/CONTRIBUTING.md) for further information.

Please make all pull requests to the `develop` branch.

## Release schedule

NautilusTrader is currently following a bi-weekly release schedule.

## License

NautilusTrader is licensed under the LGPL v3.0 as found in the [LICENSE](https://github.com/nautechsystems/nautilus_trader/blob/master/LICENSE) file.

Contributors are also required to sign a standard Contributor License Agreement (CLA), which is administered automatically through [CLA Assistant](https://cla-assistant.io/).

---

Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.

> https://nautechsystems.io

![cython](https://github.com/nautechsystems/nautilus_trader/blob/master/docs/artwork/cython-logo.png?raw=true "cython")
