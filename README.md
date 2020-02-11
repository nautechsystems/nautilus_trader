# Nautilus Trader

## Features
* **Fast:** C level speed and type safety provided through Cython. ZeroMQ message transport, MsgPack wire serialization.
* **Flexible:** Any FIX or REST broker API can be integrated into the platform, with no changes to your strategy scripts.
* **Backtesting:** Multiple instruments and strategies simultaneously with historical tick and/or bar data.
* **AI Agent Training:** Backtest engine fast enough to be used to train AI trading agents (RL/ES).
* **Teams Support:** Support for teams with many trader boxes. Suitable for professional algorithmic traders or hedge funds.
* **Cloud Enabled:** Flexible deployment schemas - run with data and execution services embedded on a single box, or deploy across many boxes in a networked or cloud environment.
* **Encryption:** Curve encryption support for ZeroMQ. Run trading boxes remote from co-located data and execution services.

[API Documentation](https://nautechsystems.io/nautilus/api)

## Installation
To install via pip run the below command;

    $ pip install -U git+https://github.com/nautechsystems/nautilus_trader

## Development
[Development Documentation](docs/development)

To run the tests, first compile the C extensions for the package;

    $ python setup.py build_ext --inplace

All tests can be run via the `run_tests.py` script, or through pytest.

## Support
Please direct all questions, comments or bug reports to info@nautechsystems.io

![Alt text](docs/artwork/cython-logo-small.png "cython")

![Alt text](docs/artwork/nautechsystems_logo_small.png?raw=true "logo")

Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.

> https://nautechsystems.io
