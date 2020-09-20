![Nautech Systems](https://github.com/nautechsystems/nautilus_trader/blob/master/docs/artwork/ns-logo.png?raw=true "logo")

---

# NautilusTrader

[![codacy-quality](https://api.codacy.com/project/badge/Grade/a1d3ccf7bccb4483b091975681a5cb23)](https://app.codacy.com/gh/nautechsystems/nautilus_trader?utm_source=github.com&utm_medium=referral&utm_content=nautechsystems/nautilus_trader&utm_campaign=Badge_Grade_Dashboard)
![build](https://github.com/nautechsystems/nautilus_trader/workflows/build/badge.svg)
![pypi-pythons](https://img.shields.io/pypi/pyversions/nautilus_trader)
![pypi-version](https://img.shields.io/pypi/v/nautilus_trader)
![pypi-downloads](https://img.shields.io/pypi/dm/nautilus_trader)

**BETA**

- **The API is still in a state of flux with potential breaking changes**
- **There is currently a large effort to develop improved documentation.**

## Introduction

NautilusTrader is an algorithmic trading platform allowing quantitative traders
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
Note that initial compilation may take several minutes due to the quantity of extensions.

    $ python setup.py build_ext --inplace

NautilusTrader has been open-sourced from working production code, and forms
part of a larger distributed system. The messaging API can interface with the Nautilus platform
where `Data` and `Execution` services implemented with C# .NET Core allow this trading framework
to integrate with `FIX4.4` connections for data ingestion and trade management.

> https://github.com/nautechsystems/Nautilus

## Features

- **Fast:** C level speed and type safety provided through Cython. ZeroMQ message transport, MsgPack wire serialization.
- **Flexible:** Any FIX or REST broker API can be integrated into the platform, with no changes to your strategy scripts.
- **Distributed:** Pluggable into distributed system architectures due to the efficient message passing API.
- **Backtesting:** Multiple instruments and strategies simultaneously with historical tick and/or bar data.
- **AI Agent Training:** Backtest engine fast enough to be used to train AI trading agents (RL/ES).
- **Teams Support:** Support for teams with many trader machines. Suitable for professional algorithmic traders or hedge funds.
- **Cloud Enabled:** Flexible deployment schemas - run with data and execution services embedded on a single machine, or deploy across many machines in a networked or cloud environment.
- **Encryption:** Built-in Curve encryption support with ZeroMQ. Run trading machines remote from co-located data and execution services.

## Values

- Reliability
- Testability
- Performance
- Modularity
- Maintainability
- Scalability

## Installation

Please ensure pyzmq >=19.0.1 is installed as some C definitions are required in
order to compile nautilus_trader. This is due to accessing zmq sockets at the raw C level.

Stable version;

    $ pip install nautilus_trader

Latest version (pre-release);

    $ pip install --pre nautilus_trader

## Development

[Development Documentation](docs/development/)
[CONTRIBUTING.md](https://github.com/nautechsystems/nautilus_trader/blob/master/CONTRIBUTING.md)

We recommend the PyCharm Professional edition IDE as it interprets Cython syntax.
Unfortunately the Community edition will not interpret Cython syntax.

> https://www.jetbrains.com/pycharm/

To run the tests, first compile the C extensions for the package. Note that
initial compilation may take several minutes due to the quantity of extensions.

    $ python setup.py build_ext --inplace

All tests can be run via the `run_tests.sh` script, or through pytest.

## Support

Please direct all questions, comments or bug reports to info@nautechsystems.io

Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.

> https://nautechsystems.io

![cython](https://github.com/nautechsystems/nautilus_trader/blob/master/docs/artwork/cython-logo.png?raw=true "cython")
