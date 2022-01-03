# Introduction

Welcome to the documentation for NautilusTrader!

NautilusTrader is an open-source, high-performance, production-grade algorithmic trading platform,
providing quantitative traders with the ability to backtest portfolios of automated trading strategies
on historical data with an event-driven engine, and also deploy those same strategies live.

The platform is 'AI-first', designed to deploy models for algorithmic trading strategies developed
using the Python ecosystem - within a highly performant and robust Python native environment.
This helps to address the challenge of keeping the research/backtest environment consistent with the production
live trading environment.

NautilusTraders design, architecture and implementation philosophy holds software correctness and safety at the
highest level, with the aim of supporting Python native, mission-critical, trading system backtesting
and live deployment workloads.

The platform is also universal and asset class agnostic - with any REST, WebSocket or FIX API able to be integrated via modular
adapters. Thus, it can handle high-frequency trading operations for any asset classes
including FX, Equities, Futures, Options, CFDs, Crypto and Betting - across multiple venues simultaneously.

## Features

- **Fast:** C-level speed through Cython. Asynchronous networking with `uvloop`.
- **Reliable:** Type safety through Cython. Redis backed performant state persistence.
- **Flexible:** OS independent, runs on Linux, macOS, Windows. Deploy using Docker.
- **Integrated:** Modular adapters mean any REST, WebSocket, or FIX API can be integrated.
- **Advanced:** Time-in-force options `GTD`, `IOC`, `FOK` etc, advanced order types and triggers, `post-only`, `reduce-only`, and icebergs. Contingency order lists including `OCO`, `OTO` etc.
- **Backtesting:** Run with multiple venues, instruments and strategies simultaneously using historical quote tick, trade tick, bar, order book and custom data with nanosecond resolution.
- **Live:** Use identical strategy implementations between backtesting and live deployments.
- **Multi-venue:** Multiple venue capabilities facilitate market making and statistical arbitrage strategies.
- **AI Agent Training:** Backtest engine fast enough to be used to train AI trading agents (RL/ES).
- **Distributed:** Run backtests synchronously or as a graph distributed across a `dask` cluster.

![Nautilus](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/artwork/nautilus-art.png?raw=true "nautilus")
> *nautilus - from ancient Greek 'sailor' and naus 'ship'.*
>
> *The nautilus shell consists of modular chambers with a growth factor which approximates a logarithmic spiral.
> The idea is that this can be translated to the aesthetics of design and architecture.*

## Why NautilusTrader?

Traditionally, trading strategy research and backtesting might be conducted in Python (or other suitable language), with
the models and/or strategies then needing to be reimplemented in C, C++, C#, Java or other statically
typed language(s). The reasoning here is to utilize the performance and type safety a compiled language can offer,
which has historically made these languages more suitable for large trading systems.

The value of NautilusTrader here is that this reimplementation step is circumvented - as the critical core components of the platform
have all been written entirely in Cython. Because Cython can generate efficient C code (which then compiles to C extension modules as native binaries),
Python can effectively be used as a high-performance systems programming language - with the benefit being that a Python native environment can be offered which is suitable for
professional quantitative traders and trading firms.

## Why Python?

Python was originally created decades ago as a simple scripting language with a clean straight
forward syntax. It has since evolved into a fully fledged general purpose object-oriented
programming language. Based on the TIOBE index, Python is currently the most popular programming language in the world.
Not only that, Python has become the _de facto lingua franca_ of data science, machine learning, and artificial intelligence.

The language out of the box is not without its drawbacks however, especially in the context of
implementing large performance-critical systems. Cython has addressed a lot of these issues, offering all the advantages
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
- Performance
- Testability
- Modularity
- Maintainability
- Deployability

![Architecture](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/artwork/architecture-overview.png?raw=true "architecture")
