NautilusTrader Documentation
============================

Introduction
------------
Welcome to the documentation for NautilusTrader!

NautilusTrader is an open-source, high-performance, production-grade algorithmic trading platform,
providing quantitative traders with the ability to backtest portfolios of automated trading strategies
on historical data with an event-driven engine, and also deploy those same strategies live.

NautilusTrader is AI/ML first, designed to deploy models for algorithmic trading strategies developed
using the Python ecosystem - within a highly performant and robust Python native environment.

The platform aims to be universal, with any REST/FIX/WebSocket API able to be integrated via modular
adapters. Thus the platform can handle high-frequency trading operations for any asset classes
including FX, Equities, Futures, Options, CFDs, Crypto and Betting - across multiple venues simultaneously.

- **Fast:** C-level speed and type safety provided through Cython. Asynchronous networking utilizing uvloop.
- **Reliable:** Redis backed performant state persistence for live implementations.
- **Flexible:** Any FIX, REST or WebSocket API can be integrated into the platform.
- **Backtesting:** Multiple instruments and strategies simultaneously with historical quote tick, trade tick, bar and order book data.
- **Multi-venue:** Multiple venue capabilities facilitate market making and statistical arbitrage strategies.
- **AI Agent Training:** Backtest engine fast enough to be used to train AI trading agents (RL/ES).

Why NautilusTrader?
-------------------
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

Why Python?
-----------
Python was originally created decades ago as a simple scripting language with a
clean straight forward syntax. It has since evolved into a fully fledged general
purpose object-oriented programming language.
Not only that, Python has become the `de facto lingua franca` of data science,
machine learning, and artificial intelligence.

The language out of the box is not without its drawbacks however, especially in the context of implementing
large systems. Cython has addressed a lot of these issues, offering all the advantages
of a statically typed language, embedded into Pythons rich ecosystem of software
libraries and developer/user communities.

What is Cython?
---------------
Cython is a compiled programming language that aims to be a superset of the Python programming
language, designed to give C-like performance with code that is written mostly in Python with
optional additional C-inspired syntax.

The project heavily utilizes Cython to provide static type safety and increased performance
for Python through C extension modules. The vast majority of the production Python code is actually
written in Cython, however the libraries can be accessed from both pure Python and Cython.

Values
------
- Reliability
- Performance
- Testability
- Modularity
- Maintainability
- Scalability




Index
-----
* :ref:`genindex`
* :ref:`search`


.. toctree::
    :glob:
    :maxdepth: 2
    :caption: Getting Started
    :hidden:

    getting_started/installation
    getting_started/core_concepts

.. toctree::
    :glob:
    :maxdepth: 2
    :caption: User Guide
    :hidden:

    user_guide/backtesting
    user_guide/writing_strategies
    user_guide/writing_indicators
    user_guide/deploying_live
    user_guide/framework

.. toctree::
    :glob:
    :maxdepth: 2
    :caption: API Reference
    :hidden:

    api_reference/analysis
    api_reference/backtest
    api_reference/cache
    api_reference/common
    api_reference/core
    api_reference/data
    api_reference/execution
    api_reference/indicators
    api_reference/infrastructure
    api_reference/live
    api_reference/model
    api_reference/msgbus
    api_reference/risk
    api_reference/serialization
    api_reference/trading

.. toctree::
    :glob:
    :maxdepth: 2
    :caption: Integrations
    :hidden:

    integrations/ib
    integrations/ccxtpro
    integrations/binance
    integrations/bitmex
    integrations/betfair

.. toctree::
    :glob:
    :maxdepth: 2
    :caption: Developer Guide
    :hidden:

    developer_guide/overview
    developer_guide/environment
    developer_guide/coding_standards
    developer_guide/testing
    developer_guide/developing_adapters
    developer_guide/packaged_data
