# NautilusTrader Documentation

Welcome to the official documentation for NautilusTrader!

NautilusTrader is an open-source, high-performance, production-grade algorithmic trading platform,
providing quantitative traders with the ability to backtest portfolios of automated trading strategies
on historical data with an event-driven engine, and also deploy those same strategies live, with no code changes.

The platform is 'AI-first', designed to develop and deploy algorithmic trading strategies within a highly performant 
and robust Python native environment. This helps to address the parity challenge of keeping the Python research/backtest 
environment, consistent with the production live trading environment.

NautilusTraders design, architecture and implementation philosophy holds software correctness and safety at the
highest level, with the aim of supporting Python native, mission-critical, trading system backtesting
and live deployment workloads.

The platform is also universal and asset class agnostic - with any REST, WebSocket or FIX API able to be integrated via modular
adapters. Thus, it can handle high-frequency trading operations for any asset classes
including FX, Equities, Futures, Options, CFDs, Crypto and Betting - across multiple venues simultaneously.

## Features

- **Fast:** C-level speed through Rust and Cython. Asynchronous networking with [uvloop](https://github.com/MagicStack/uvloop)
- **Reliable:** Type safety through Rust and Cython. Redis backed performant state persistence
- **Flexible:** OS independent, runs on Linux, macOS, Windows. Deploy using Docker
- **Integrated:** Modular adapters mean any REST, WebSocket, or FIX API can be integrated
- **Advanced:** Time in force `IOC`, `FOK`, `GTD`, `AT_THE_OPEN`, `AT_THE_CLOSE`, advanced order types and conditional triggers. Execution instructions `post-only`, `reduce-only`, and icebergs. Contingency order lists including `OCO`, `OTO`
- **Backtesting:** Run with multiple venues, instruments and strategies simultaneously using historical quote tick, trade tick, bar, order book and custom data with nanosecond resolution
- **Live:** Use identical strategy implementations between backtesting and live deployments
- **Multi-venue:** Multiple venue capabilities facilitate market making and statistical arbitrage strategies
- **AI Agent Training:** Backtest engine fast enough to be used to train AI trading agents (RL/ES)

![Nautilus](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/_images/nautilus-art.png?raw=true "nautilus")
> *nautilus - from ancient Greek 'sailor' and naus 'ship'.*
>
> *The nautilus shell consists of modular chambers with a growth factor which approximates a logarithmic spiral.
> The idea is that this can be translated to the aesthetics of design and architecture.*

## Why NautilusTrader?

- **Highly performant event-driven Python** - native binary core components
- **Parity between backtesting and live trading** - identical strategy code
- **Reduced operational risk** - risk management functionality, logical correctness and type safety
- **Highly extendable** - message bus, custom components and actors, custom data, custom adapters

Traditionally, trading strategy research and backtesting might be conducted in Python (or other suitable language)
using vectorized methods, with the strategy then needing to be reimplemented in a more event-drive way
using C++, C#, Java or other statically typed language(s). The reasoning here is that vectorized backtesting code cannot
express the granular time and event dependent complexity of real-time trading, where compiled languages have
proven to be more suitable due to their inherently higher performance, and type safety.

One of the key advantages of NautilusTrader here, is that this reimplementation step is now circumvented - as the critical core components of the platform
have all been written entirely in Rust or Cython. This means we're using the right tools for the job, where systems programming languages compile performant binaries, 
with CPython C extension modules then able to offer a Python native environment, suitable for professional quantitative traders and trading firms.

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
written in Cython, however the libraries can be accessed from both Python and Cython.

## What is Rust?

[Rust](https://www.rust-lang.org/) is a multi-paradigm programming language designed for performance and safety, especially safe
concurrency. Rust is blazingly fast and memory-efficient (comparable to C and C++) with no runtime or
garbage collector. It can power mission-critical systems, run on embedded devices, and easily
integrates with other languages.

Rust’s rich type system and ownership model guarantees memory-safety and thread-safety deterministically —
eliminating many classes of bugs at compile-time.

The project increasingly utilizes Rust for core performance-critical components. Python language binding is handled through
Cython, with static libraries linked at compile-time before the wheel binaries are packaged, so a user
does not need to have Rust installed to run NautilusTrader. In the future as more Rust code is introduced,
[PyO3](https://pyo3.rs/latest) will be leveraged for easier Python bindings.

## Architecture Quality Attributes

- Reliability
- Performance
- Modularity
- Testability
- Maintainability
- Deployability

![Architecture](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/_images/architecture-overview.png?raw=true "architecture")

```{eval-rst}
.. toctree::
   :maxdepth: 1
   :glob:
   :titlesonly:
   :hidden:

   getting_started/index.md
   concepts/index.md
   guides/index.md
   integrations/index.md
   api_reference/index.md
   developer_guide/index.md

```
