# Overview

## Introduction

NautilusTrader is an open-source, high-performance, production-grade algorithmic trading platform,
providing quantitative traders with the ability to backtest portfolios of automated trading strategies
on historical data with an event-driven engine, and also deploy those same strategies live, with no code changes.

The platform is *AI-first*, designed to develop and deploy algorithmic trading strategies within a highly performant
and robust Python-native environment. This helps to address the parity challenge of keeping the Python research/backtest
environment consistent with the production live trading environment.

NautilusTrader's design, architecture, and implementation philosophy prioritizes software correctness and safety at the
highest level, with the aim of supporting Python-native, mission-critical, trading system backtesting
and live deployment workloads.

The platform is also universal and asset-class-agnostic — with any REST API or WebSocket stream able to be integrated via modular
adapters. It supports high-frequency trading across a wide range of asset classes and instrument types
including FX, Equities, Futures, Options, Crypto, DeFi, and Betting — enabling seamless operations across multiple venues simultaneously.

## Features

- **Fast**: Core is written in Rust with asynchronous networking using [tokio](https://crates.io/crates/tokio).
- **Reliable**: Rust-powered type- and thread-safety, with optional Redis-backed state persistence.
- **Portable**: OS independent, runs on Linux, macOS, and Windows. Deploy using Docker.
- **Flexible**: Modular adapters mean any REST API or WebSocket stream can be integrated.
- **Advanced**: Time in force `IOC`, `FOK`, `GTC`, `GTD`, `DAY`, `AT_THE_OPEN`, `AT_THE_CLOSE`, advanced order types and conditional triggers. Execution instructions `post-only`, `reduce-only`, and icebergs. Contingency orders including `OCO`, `OUO`, `OTO`.
- **Customizable**: Add user-defined custom components, or assemble entire systems from scratch leveraging the [cache](cache.md) and [message bus](message_bus.md).
- **Backtesting**: Run with multiple venues, instruments and strategies simultaneously using historical quote tick, trade tick, bar, order book and custom data with nanosecond resolution.
- **Live**: Use identical strategy implementations between backtesting and live deployments.
- **Multi-venue**: Multiple venue capabilities facilitate market-making and statistical arbitrage strategies.
- **AI Training**: Backtest engine fast enough to be used to train AI trading agents (RL/ES).

![Nautilus](https://github.com/nautechsystems/nautilus_trader/blob/develop/assets/nautilus-art.png?raw=true "nautilus")
> *nautilus - from ancient Greek 'sailor' and naus 'ship'.*
>
> *The nautilus shell consists of modular chambers with a growth factor which approximates a logarithmic spiral.
> The idea is that this can be translated to the aesthetics of design and architecture.*

## Why NautilusTrader?

- **Highly performant event-driven Python**: Native binary core components.
- **Parity between backtesting and live trading**: Identical strategy code.
- **Reduced operational risk**: Enhanced risk management functionality, logical accuracy, and type safety.
- **Highly extendable**: Message bus, custom components and actors, custom data, custom adapters.

Traditionally, trading strategy research and backtesting might be conducted in Python
using vectorized methods, with the strategy then needing to be reimplemented in a more event-driven way
using C++, C#, Java or other statically typed language(s). The reasoning here is that vectorized backtesting code cannot
express the granular time and event dependent complexity of real-time trading, where compiled languages have
proven to be more suitable due to their inherently higher performance, and type safety.

One of the key advantages of NautilusTrader here, is that this reimplementation step is now circumvented - as the critical core components of the platform
have all been written entirely in [Rust](https://www.rust-lang.org/) or [Cython](https://cython.org/).
This means we're using the right tools for the job, where systems programming languages compile performant binaries,
with CPython C extension modules then able to offer a Python-native environment, suitable for professional quantitative traders and trading firms.

## Use cases

There are three main use cases for this software package:

- Backtest trading systems on historical data (`backtest`).
- Simulate trading systems with real-time data and virtual execution (`sandbox`).
- Deploy trading systems live on real or paper accounts (`live`).

The project's codebase provides a framework for implementing the software layer of systems which achieve the above. You will find
the default `backtest` and `live` system implementations in their respectively named subpackages. A `sandbox` environment can
be built using the sandbox adapter.

:::note

- All examples will utilize these default system implementations.
- We consider trading strategies to be subcomponents of end-to-end trading systems, these systems
include the application and infrastructure layers.

:::

## Distributed

The platform is designed to be easily integrated into a larger distributed system.
To facilitate this, nearly all configuration and domain objects can be serialized using JSON, MessagePack or Apache Arrow (Feather) for communication over the network.

## Common core

The common system core is utilized by all node [environment contexts](/concepts/architecture.md#environment-contexts) (`backtest`, `sandbox`, and `live`).
User-defined `Actor`, `Strategy` and `ExecAlgorithm` components are managed consistently across these environment contexts.

## Backtesting

Backtesting can be achieved by first making data available to a `BacktestEngine` either directly or via
a higher level `BacktestNode` and `ParquetDataCatalog`, and then running the data through the system with nanosecond resolution.

## Live trading

A `TradingNode` can ingest data and events from multiple data and execution clients.
Live deployments can use both demo/paper trading accounts, or real accounts.

For live trading, a `TradingNode` can ingest data and events from multiple data and execution clients.
The platform supports both demo/paper trading accounts and real accounts. High performance can be achieved by running
asynchronously on a single [event loop](https://docs.python.org/3/library/asyncio-eventloop.html),
with the potential to further boost performance by leveraging the [uvloop](https://github.com/MagicStack/uvloop) implementation (available for Linux and macOS).

## Domain model

The platform features a comprehensive trading domain model that includes various value types such as
`Price` and `Quantity`, as well as more complex entities such as `Order` and `Position` objects,
which are used to aggregate multiple events to determine state.

## Timestamps

All timestamps within the platform are recorded at nanosecond precision in UTC.

Timestamp strings follow ISO 8601 (RFC 3339) format with either 9 digits (nanoseconds) or 3 digits (milliseconds) of decimal precision,
(but mostly nanoseconds) always maintaining all digits including trailing zeros.
These can be seen in log messages, and debug/display outputs for objects.

A timestamp string consists of:

- Full date component always present: `YYYY-MM-DD`.
- `T` separator between date and time components.
- Always nanosecond precision (9 decimal places) or millisecond precision (3 decimal places) for certain cases such as GTD expiry times.
- Always UTC timezone designated by `Z` suffix.

Example: `2024-01-05T15:30:45.123456789Z`

For the complete specification, refer to [RFC 3339: Date and Time on the Internet](https://datatracker.ietf.org/doc/html/rfc3339).

## UUIDs

The platform uses Universally Unique Identifiers (UUID) version 4 (RFC 4122) for unique identifiers.
Our high-performance implementation leverages the `uuid` crate for correctness validation when parsing from strings,
ensuring input UUIDs comply with the specification.

A valid UUID v4 consists of:

- 32 hexadecimal digits displayed in 5 groups.
- Groups separated by hyphens: `8-4-4-4-12` format.
- Version 4 designation (indicated by the third group starting with "4").
- RFC 4122 variant designation (indicated by the fourth group starting with "8", "9", "a", or "b").

Example: `2d89666b-1a1e-4a75-b193-4eb3b454c757`

For the complete specification, refer to [RFC 4122: A Universally Unique IIdentifier (UUID) URN Namespace](https://datatracker.ietf.org/doc/html/rfc4122).

## Data types

The following market data types can be requested historically, and also subscribed to as live streams when available from a venue / data provider, and implemented in an integrations adapter.

- `OrderBookDelta` (L1/L2/L3)
- `OrderBookDeltas` (container type)
- `OrderBookDepth10` (fixed depth of 10 levels per side)
- `QuoteTick`
- `TradeTick`
- `Bar`
- `Instrument`
- `InstrumentStatus`
- `InstrumentClose`

The following `PriceType` options can be used for bar aggregations:

- `BID`
- `ASK`
- `MID`
- `LAST`

## Bar aggregations

The following `BarAggregation` methods are available:

- `MILLISECOND`
- `SECOND`
- `MINUTE`
- `HOUR`
- `DAY`
- `WEEK`
- `MONTH`
- `YEAR`
- `TICK`
- `VOLUME`
- `VALUE` (a.k.a Dollar bars)
- `RENKO` (price-based bricks)
- `TICK_IMBALANCE`
- `TICK_RUNS`
- `VOLUME_IMBALANCE`
- `VOLUME_RUNS`
- `VALUE_IMBALANCE`
- `VALUE_RUNS`

Currently implemented aggregations:

- `MILLISECOND`
- `SECOND`
- `MINUTE`
- `HOUR`
- `DAY`
- `WEEK`
- `MONTH`
- `YEAR`
- `TICK`
- `VOLUME`
- `VALUE`
- `RENKO`

Aggregations listed above that are not repeated in the implemented list are planned but not yet available.

The price types and bar aggregations can be combined with step sizes >= 1 in any way through a `BarSpecification`.
This enables maximum flexibility and now allows alternative bars to be aggregated for live trading.

## Account types

The following account types are available for both live and backtest environments:

- `Cash` single-currency (base currency)
- `Cash` multi-currency
- `Margin` single-currency (base currency)
- `Margin` multi-currency
- `Betting` single-currency

## Order types

The following order types are available (when possible on a venue):

- `MARKET`
- `LIMIT`
- `STOP_MARKET`
- `STOP_LIMIT`
- `MARKET_TO_LIMIT`
- `MARKET_IF_TOUCHED`
- `LIMIT_IF_TOUCHED`
- `TRAILING_STOP_MARKET`
- `TRAILING_STOP_LIMIT`

## Value types

The following value types are backed by either 128-bit or 64-bit raw integer values, depending on the
[precision mode](../getting_started/installation.md#precision-mode) used during compilation.

- `Price`
- `Quantity`
- `Money`

### High-precision mode (128-bit)

When the `high-precision` feature flag is **enabled** (default), values use the specification:

| Type         | Raw backing | Max precision | Min value           | Max value          |
|:-------------|:------------|:--------------|:--------------------|:-------------------|
| `Price`      | `i128`      | 16            | -17,014,118,346,046 | 17,014,118,346,046 |
| `Money`      | `i128`      | 16            | -17,014,118,346,046 | 17,014,118,346,046 |
| `Quantity`   | `u128`      | 16            | 0                   | 34,028,236,692,093 |

### Standard-precision mode (64-bit)

When the `high-precision` feature flag is **disabled**, values use the specification:

| Type         | Raw backing | Max precision | Min value           | Max value          |
|:-------------|:------------|:--------------|:--------------------|:-------------------|
| `Price`      | `i64`       | 9             | -9,223,372,036      | 9,223,372,036      |
| `Money`      | `i64`       | 9             | -9,223,372,036      | 9,223,372,036      |
| `Quantity`   | `u64`       | 9             | 0                   | 18,446,744,073     |
