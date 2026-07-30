# Overview

## Introduction

NautilusTrader is an open‑source, production‑grade, Rust‑native engine for multi‑asset,
multi‑venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event‑driven architecture, with Python serving as the control plane for strategy logic,
configuration, and orchestration.

This separation provides the performance and safety of a compiled trading engine with
the flexibility of Python for system composition and strategy development.
Trading systems can also be written entirely in Rust for mission‑critical workloads.

The same execution semantics and deterministic time model operate in both research and
live systems. Strategies deploy from research to production with no code changes,
providing research‑to‑live parity and reducing the divergence that typically introduces
deployment risk.

NautilusTrader is asset‑class‑agnostic. Any venue with a REST API or WebSocket feed can be
integrated through modular adapters. Integrations span centralized and decentralized crypto
exchanges (CEX and DEX), foreign exchange (FX), equities, futures, options, and betting exchanges.

## Features

- **Fast**: Rust core with asynchronous networking using [tokio](https://crates.io/crates/tokio).
- **Reliable**: Rust provides type safety and thread safety, with optional state persistence backed
  by Redis or PostgreSQL.
- **Portable**: Runs on Linux, macOS, and Windows. Deploy using Docker.
- **Flexible**: Modular adapters integrate any REST API or WebSocket feed.
- **Advanced orders**: Time‑in‑force options include `IOC`, `FOK`, `GTC`, `GTD`, `DAY`,
  `AT_THE_OPEN`, and `AT_THE_CLOSE`. The domain model also supports conditional triggers,
  `post-only`, `reduce-only`, iceberg, and `OCO`, `OUO`, and `OTO` contingency orders. Venue
  support varies by adapter.
- **Customizable**: User‑defined components, or assemble entire systems from scratch using the
  [cache](cache.md) and [message bus](message_bus.md).
- **Backtesting**: Run multiple venues, instruments, and strategies simultaneously using
  historical quotes, trades, bars, order books, and custom data with nanosecond resolution.
- **Live**: Identical strategy implementations between research and live deployment.
- **Multi‑venue**: Run market‑making and cross‑venue strategies across multiple venues simultaneously.
- **AI training**: High‑throughput simulation supports workloads such as training AI trading agents
  with reinforcement learning (RL) or evolutionary strategies (ES).

## Why NautilusTrader?

Trading strategy research typically happens in Python using vectorized approaches, while
production trading systems are built separately using event‑driven architectures in
compiled languages.

NautilusTrader removes this separation.

A Rust‑native core provides a deterministic event‑driven runtime for both research and live
execution, while Python serves as the control plane. The same architecture, execution
semantics, and time model operate across both environments, allowing strategies to move
from research to production without reimplementation.

Python bindings for the Rust‑native v2 runtime are provided via [PyO3](https://pyo3.rs).
The legacy Cython v1 core remains supported during the v2 release‑candidate phase. Installing
an official prebuilt Python wheel does not require a Rust toolchain.

## Use cases

NautilusTrader supports three main use cases:

- Backtest trading systems on historical data (`backtest`).
- Simulate trading systems with real‑time data and virtual execution (`sandbox`).
- Deploy trading systems live on real or paper accounts (`live`).

NautilusTrader provides backtest and live node implementations for both Python and Rust.
The sandbox adapter supplies simulated execution for a `sandbox` environment.

:::note

- Examples use these node implementations unless stated otherwise.
- A trading strategy is one component of an end‑to‑end trading system, which also includes
  application and infrastructure layers.

:::

## Distributed

The platform integrates into larger distributed systems. The
[external message bus](message_bus.md#encoding) supports JSON and MessagePack payloads, plus
Cap'n Proto and Simple Binary Encoding (SBE) for schema‑covered market data. Apache Arrow and
Parquet provide columnar interchange and persistence through the
[data catalog](data/index.md#data-catalog). Format support varies by payload type.

## Common core

The common system core is used by all node
[environment contexts](architecture.md#environment-contexts): `backtest`, `sandbox`, and `live`.
User‑defined actors, strategies, and execution algorithms use the same lifecycle across these
contexts.

## Backtesting

Feed data to a `BacktestEngine` either directly or through a higher‑level `BacktestNode` and
`ParquetDataCatalog`, then run the data through the system with nanosecond resolution. See
[Backtesting](backtesting/) for the APIs and execution model.

## Live trading

A `LiveNode` ingests data and events from multiple data and execution clients, supporting
demo, paper, and real accounts. The Rust‑native node, including its PyO3 interface, runs the kernel
event loop on the calling thread, while asynchronous I/O and background tasks use a shared
multi‑threaded Tokio runtime. The legacy Python `TradingNode` coordinates asynchronous clients
through an [asyncio event loop](https://docs.python.org/3/library/asyncio-eventloop.html) and can use
[uvloop](https://github.com/MagicStack/uvloop) on Linux and macOS. See [Live Trading](live.md) for
lifecycle, reconciliation, and risk considerations.

## Domain model

The trading domain model includes [value types](value_types.md) such as `Price` and `Quantity`,
plus [orders](orders/) and [positions](positions.md) that aggregate events to determine state.

## Timestamps

NautilusTrader represents system timestamps as UNIX nanoseconds. Its standard ISO 8601
(RFC 3339) formatter uses UTC and preserves all nine fractional digits. A millisecond formatter
preserves three fractional digits for selected displays, such as good‑till‑date (GTD) expiry times.

A timestamp string consists of:

- Full date component always present: `YYYY-MM-DD`.
- `T` separator between date and time components.
- Nine fractional digits for nanosecond output, or three for millisecond output.
- UTC timezone designated by the `Z` suffix.

Example: `2024-01-05T15:30:45.123456789Z`

For the complete specification, refer to
[RFC 3339: Date and Time on the Internet](https://datatracker.ietf.org/doc/html/rfc3339).

## UUIDs

The `UUID4` value type provides random Universally Unique Identifier (UUID) version 4 values for
events, commands, reports, and other internal messages. It uses the `uuid` crate to validate
version and variant bits when parsing strings.

A valid UUID v4 under RFC 9562 consists of:

- 32 hexadecimal digits displayed in 5 groups.
- Groups separated by hyphens: `8-4-4-4-12` format.
- Version 4 designation (indicated by the third group starting with "4").
- IETF variant designation (indicated by the fourth group starting with "8", "9", "a", or "b").

Example: `2d89666b-1a1e-4a75-b193-4eb3b454c757`

For the complete specification, see
[RFC 9562: Universally Unique Identifiers (UUIDs)](https://www.rfc-editor.org/rfc/rfc9562.html).

## Data types

NautilusTrader defines the following built‑in market and reference data types. Availability for
historical requests and live subscriptions depends on the provider and adapter. See
[Data](data/index.md) for their fields and behavior.

- `OrderBookDelta` (single order book change)
- `OrderBookDeltas` (container type)
- `OrderBookDepth10` (fixed depth of 10 levels per side)
- `QuoteTick`
- `TradeTick`
- `Bar`
- `MarkPriceUpdate`
- `IndexPriceUpdate`
- `FundingRateUpdate`
- `OptionGreeks`
- `Instrument`
- `InstrumentStatus`
- `InstrumentClose`

Use [custom data](custom_data.md) for application‑specific types.

The following `PriceType` options select granular data for internal bar aggregation:

- `BID`
- `ASK`
- `MID`
- `LAST`

`BID`, `ASK`, and `MID` use `QuoteTick` data, while `LAST` uses `TradeTick` data.
Composite bar types aggregate smaller bars instead.

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
- `VALUE` (also known as dollar bars)
- `RENKO` (price‑based bricks)
- `TICK_IMBALANCE`
- `TICK_RUNS`
- `VOLUME_IMBALANCE`
- `VOLUME_RUNS`
- `VALUE_IMBALANCE`
- `VALUE_RUNS`

All listed aggregations are implemented for internal aggregation.
Information‑driven aggregations require `TradeTick` data.

A `BarSpecification` combines a price type, aggregation method, and positive step size.
Fixed‑subunit time bars have divisibility limits; see [Bar types](data/index.md#bar-types) for the
validation rules. Internal aggregation can run during live trading when the required input data is
available.

## Account types

The [accounting engine](accounting.md#account-types) supports the following configurations in both
live and backtest environments:

- `Cash` single‑currency (base currency)
- `Cash` multi‑currency
- `Margin` single‑currency (base currency)
- `Margin` multi‑currency
- `Betting` single‑currency

## Order types

The [order model](orders/) supports the following types, subject to venue adapter support:

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

The following fixed‑point value types are backed by either 128‑bit or 64‑bit raw integers,
depending on the [precision mode](../getting_started/installation.md#precision-mode) used during
compilation.

- `Price`
- `Quantity`
- `Money`

Official Python wheels use high‑precision mode on Linux and macOS, and standard‑precision mode on
Windows. Pure Rust builds default to standard precision unless the `high-precision` feature is
enabled.

### High-precision mode (128-bit)

When the `high-precision` feature flag is **enabled**, values use the specification:

| Type       | Raw backing | Max precision | Min value           | Max value          |
| :--------- | :---------- | :------------ | :------------------ | :----------------- |
| `Price`    | `i128`      | 16            | -17,014,118,346,046 | 17,014,118,346,046 |
| `Money`    | `i128`      | 16            | -17,014,118,346,046 | 17,014,118,346,046 |
| `Quantity` | `u128`      | 16            | 0                   | 34,028,236,692,093 |

### Standard-precision mode (64-bit)

When the `high-precision` feature flag is **disabled**, values use the specification:

| Type       | Raw backing | Max precision | Min value      | Max value      |
| :--------- | :---------- | :------------ | :------------- | :------------- |
| `Price`    | `i64`       | 9             | -9,223,372,036 | 9,223,372,036  |
| `Money`    | `i64`       | 9             | -9,223,372,036 | 9,223,372,036  |
| `Quantity` | `u64`       | 9             | 0              | 18,446,744,073 |
