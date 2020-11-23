NautilusTrader Documentation
============================

Introduction
------------
Welcome to the documentation for `NautilusTrader`, an open-source, high-performance,
production-grade trading platform. It is hoped that this project gains wide
adoption within the trading community, assisting with safe, reliable and efficient
trading operations.

The platform aims
to be universal, with any REST/FIX/WebSockets API able to be integrated via modular adapters.
Thus the platform can handle high-frequency trading operations for any asset classes
including FX, Equities, Futures, CFDs or Crypto - across multiple venues simultaneously.

Why NautilusTrader?
-------------------
One of the key value propositions of `NautilusTrader` is that it addresses the
challenge of keeping the backtest environment consistent with the production
live trading environment.

Normally research and backtesting may be conducted in
Python (or other suitable language), with trading strategies traditionally then
needing to be reimplemented in C++/C#/Java or other statically typed language(s).
The reasoning here is to enjoy the performance a compiled language can offer,
along with the tooling and support which has made these languages historically
more suitable for large enterprise systems.

The value of `NautilusTrader` here is that this re-implementation step is circumvented, as the
platform was designed from the ground up to hold its own in terms of performance
and quality. Python has simply caught up in performance
(via Cython offering C-level speed) and general tooling, making it a suitable language for implementing
a large system such as this. The benefit here being that a Python native environment
can be offered, suitable for professional quantitative traders and hedge funds.

Why Python?
-----------
Python was originally created decades ago as a simple scripting language with a
clean straight forward syntax. It has since evolved into a fully fledged general
purpose object-oriented programming language.
Not only that, Python has become the `de facto lingua franca` of data science,
machine learning, and artificial intelligence.

The language out of the box is not without its drawbacks however, especially in the context of implementing
large systems. Cython has addressed some of these issues, offering all the advantages
of a statically typed language, embedded into Pythons rich ecosystem of software
libraries and developer/user communities.




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
    api_reference/common
    api_reference/core
    api_reference/data
    api_reference/execution
    api_reference/indicators
    api_reference/model
    api_reference/postgres
    api_reference/redis
    api_reference/serialization
    api_reference/trading

.. toctree::
    :glob:
    :maxdepth: 2
    :caption: Adapter Reference
    :hidden:

    adapter_reference/binance
    adapter_reference/ccxt
    adapter_reference/tda

.. toctree::
    :glob:
    :maxdepth: 2
    :caption: Developer Guide
    :hidden:

    developer_guide/overview
    developer_guide/environment
    developer_guide/coding_standards
    developer_guide/testing
