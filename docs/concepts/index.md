# Concepts

These guides explain the core components, architecture, and design of NautilusTrader.

## Overview

Main features and intended use cases for the platform.

## Architecture

The principles, structures, and designs that underpin the platform.

## Actors

The `Actor` is the base component for interacting with the trading system.
Covers capabilities and implementation details.

## Strategies

How to implement trading strategies using the `Strategy` component.

## Instruments

Instrument definitions for tradable assets and contracts.

## Synthetics

User-defined instruments whose prices are computed by evaluating a numeric expression over component instrument prices.

## Value Types

The immutable numeric types (`Price`, `Quantity`, `Money`) used throughout the platform,
including their arithmetic behavior, precision handling, and type-specific constraints.

## Data

Built-in data types for the trading domain, and how to work with custom data.

## Events

The event types that drive the system: order events, position events, account
events, and time events. Covers handler dispatch, the causal chain from order
fills to position events, and tracing orders to positions.

## Options

Option instrument types, venue-provided Greeks streaming, option chain subscriptions
with strike range filtering, and snapshot aggregation.

## Greeks

Option Greeks (delta, gamma, vega, theta) from two paths: venue-provided real-time
Greeks via the Rust/PyO3 `OptionGreeks` type, and the local `GreeksCalculator` for
Black-Scholes computation with shock scenarios, beta weighting, and portfolio aggregation.

## Custom Data

How the custom data system works across Python and Rust: registration, persistence,
Arrow encoding, and runtime routing through actors and strategies.

## Order Book

The high-performance order book, own order tracking, filtered views for net liquidity, and binary market support.

## Execution

Trade execution and order management across multiple strategies and venues simultaneously (per instance),
including the components involved and the flow of execution messages (commands and events).

## Orders

Available order types, supported execution instructions, advanced order types, and emulated orders.

## Positions

Position lifecycle, aggregation from order fills, PnL calculations, and position snapshotting
for netting OMS configurations.

## Cache

The `Cache` is the central in-memory store for all trading-related data.
Covers capabilities and best practices.

## Message Bus

The `MessageBus` enables decoupled messaging between components, supporting point-to-point,
publish/subscribe, and request/response patterns.

## Accounting

Account types (cash, margin, betting), the `AccountBalance` and `MarginBalance`
data model, the per-instrument vs account-wide margin scopes, the strategy query
API, built-in margin models, and the adapter convention across live venues.

## Portfolio

The `Portfolio` tracks all positions across strategies and instruments, providing a unified view
of holdings, risk exposure, and performance.

## Reports

Execution reports, portfolio analysis, PnL accounting, and backtest post-run analysis.

## Logging

High-performance logging for both backtesting and live trading, implemented in Rust.

## Backtesting

Running simulated trading on historical data using a specific system implementation.

## Visualization

Interactive tearsheets for analyzing backtest results, including charts, themes,
customization options, and custom visualizations via the extensible chart registry.

## Configuration

How config structs work across Python and Rust: default resolution, the `T` vs `Option<T>`
convention, builder patterns, and common fields shared across adapters and engines.

## Live Trading

Deploying backtested strategies in real-time without code changes, and the key differences
between backtesting and live trading.

## Adapters

Requirements and best practices for developing integration adapters for data providers and trading venues.

## Rust

Writing actors, strategies, and running backtests and live trading in pure Rust
using the `crates/` implementation directly.

## Deterministic Simulation Testing (DST)

The determinism contract for seed-replayable execution, the source-level seams that implement
it, the pre-commit hook that enforces it, and the known scope boundaries.

:::note
If there are discrepancies between these guides and the API reference, the API reference is correct.
:::
