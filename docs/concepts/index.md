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

## Value Types

The immutable numeric types (`Price`, `Quantity`, `Money`) used throughout the platform,
including their arithmetic behavior, precision handling, and type-specific constraints.

## Data

Built-in data types for the trading domain, and how to work with custom data.

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

## Live Trading

Deploying backtested strategies in real-time without code changes, and the key differences
between backtesting and live trading.

## Adapters

Requirements and best practices for developing integration adapters for data providers and trading venues.

:::note
The [API Reference](../api_reference/index.md) is the source of truth for the platform.
If there are discrepancies between these guides and the API Reference, the API Reference is correct.
:::
