NautilusTrader Documentation
============================

***UNDER CONSTRUCTION***

Introduction
------------
Welcome to the documentation for `NautilusTrader`, an open-source, high-performance,
production-grade trading platform. It is hoped that this project gains wide
adoption within the trading community, assisting with safe, reliable and efficient
trading operations - utilizing the latest advanced technologies. The platform aims
to be universal, with any REST/FIX/WebSockets API able to be integrated via modular adapters.
Thus the platform can handle high-frequency trading operations for any asset classes
including FX, Equities, Futures, CFDs or Crypto.

One of the key value propositions of `NautilusTrader` is that it addresses the
challenge of keeping the backtest environment consistent with the production
live trading environment. Normally research and backtesting may be conducted in
Python (or other suitable language), with trading strategies traditionally then
needing to be reimplemented in C++/C#/Java or another statically typed language.
The reasoning here is to enjoy the performance a compiled language can offer,
along with the tooling and support which has made these languages historically
more suitable for large enterprise systems.

The value here is that this re-implementation step is circumvented, as the
platform was designed from the ground up to hold its own in terms of performance
and enterprise grade quality. Python has simply caught right up on performance
(via Cython) and general tooling, making it a suitable language for implementing
a large system such as this. The benefit being that a Python native environment
with C level speed can now be offered to professional quantitative traders and
hedge funds, to meet their rigorous standards.

What exactly does `production-grade` or `enterprise-grade` mean?
Python was originally created decades ago as a simple scripting language with a
clean straight forward syntax. It has since evolved into a fully fledged general
purpose object-oriented programming language.
Not only that, Python has become the `de facto lingua franca` of data science,
machine learning, and artificial intelligence. The language (out of the box) is
not without its drawbacks however, especially in the context of implementing
a large enterprise type system such as that offered with the `NautilusTrader`
package. Cython has addressed some of these issues, offering all the advantages
of a statically typed language, embedded into Pythons rich ecosystem of software
libraries and developer/user communities.

Architectural Overview
----------------------
The package offers a framework comprising of an extensive assortment of modular
components, which can be arranged into a complete trading platform and system.

The platform is structured around a simple ports and adapters style
architecture, allowing pluggable implementations of key components with a
feature rich yet straight forward API. Domain driven design and message passing
have been central philosophies in the design. From a high level
view - a `Trader` can host any number of infinitely customizable
`TradingStrategies`. A central `Portfolio` has access to `Account`s . A common
`DataEngine` and `ExecutionEngine` then allow asynchronous ingest of any data
and trade events with the core componentry common to both backtesting and live
implementations. Currently a performant `Redis` execution database maintains
state persistence (swapped out for an in-memory only implementation for backtesting).
It should be noted that the flexibility of the framework even allows the live trading
`Redis` database to be plugged into the backtest engine. Interestingly there is
only a 2x performance overhead which speaks to the raw speed of `Redis` and the
platform itself.

To be continued (WIP)...



Index
-----
* :ref:`genindex`
* :ref:`search`

.. toctree::
    :maxdepth: 2
    :caption: Getting Started
    :hidden:

    getting_started/installation

.. toctree::
    :maxdepth: 2
    :caption: Module API
    :hidden:

    analysis
    backtest
    common
    core
    data
    execution
    indicators
    model
    postgres
    redis
    serialization
    trading

.. toctree::
    :maxdepth: 2
    :caption: Adapters
    :hidden:

    adapters.binance
    adapters.bitmex
    adapters.ccxt
    adapters.tda
