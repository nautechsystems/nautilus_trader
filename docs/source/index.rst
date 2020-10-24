NautilusTrader Documentation
============================

***UNDER CONSTRUCTION***

Introduction
------------
Welcome to the documentation for `NautilusTrader`, an open-source, high-performance,
production-grade trading platform. It is hoped that this project gains wide
adoption within the trading community, assisting with safe, reliable and efficient
trading operations utilizing the latest advanced technologies.

The package offers a framework comprising of an extensive assortment of modular
components, which can be arranged into a complete trading platform and system.

Overview
--------
The platform has been designed with a simple modular ports and adapters style
architecture, allowing pluggable implementations of key components with a
consistent API.

What exactly is `production-grade`? Python was originally created decades ago as
a simple scripting language with a clean straight forward syntax. It has since
evolved into a fully fledged general purpose object-oriented programming language.
Not only that, Python has become the `de facto lingua franca` of data science,
machine learning, and artificial intelligence. The language (out of the box) is
not without its drawbacks however, especially in the context of implementing
a large enterprise type system such as that offered with the `NautilusTrader`
package. Cython has addressed some of these issues, offering all the advantages
of a statically typed language, embedded into Pythons rich ecosystem of software
libraries and developer/user communities.

Index
-----
* :ref:`genindex`
* :ref:`search`

.. toctree::
    :maxdepth: 2
    :caption: Getting Started
    :hidden:

    rst/getting_started/installation

.. toctree::
    :maxdepth: 2
    :caption: Module API
    :hidden:

    rst/modules/analysis
    rst/modules/backtest
    rst/modules/common
    rst/modules/core
    rst/modules/data
    rst/modules/execution
    rst/modules/indicators
    rst/modules/model
    rst/modules/postgres
    rst/modules/redis
    rst/modules/serialization
    rst/modules/trading

.. toctree::
    :maxdepth: 2
    :caption: Adapters
    :hidden:

    rst/adapters/adapters.binance
    rst/adapters/adapters.bitmex
    rst/adapters/adapters.ccxt
    rst/adapters/adapters.tda
