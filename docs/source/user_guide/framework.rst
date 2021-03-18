Framework
=========

Architectural Overview
----------------------
The package offers a framework comprising of an extensive assortment of modular
components, which can be arranged into a complete trading platform and system.

The platform is structured around a simple ports and adaptors style
architecture, allowing pluggable implementations of key components with a
feature rich yet straight forward API. `Domain Driven Design` (DDD) and message passing
have been central philosophies in the design.

From a high level view - a ``Trader`` can host any number of infinitely customizable
``TradingStrategy`` instances. A central ``Portfolio`` has access to multiple ``Account`` instances,
which can all be queried.

A common ``DataEngine`` and ``ExecutionEngine`` allows asynchronous ingest of any data
and trade events, with the core componentry common to both backtesting and live implementations.

Currently a performant Redis execution database maintains state persistence
(swapped out for an in-memory only implementation for backtesting).
It should be noted that the flexibility of the framework even allows the live trading
Redis database to be plugged into the backtest engine. Interestingly there is
only a 4x performance overhead which speaks to the raw speed of Redis.

**work in progress**
