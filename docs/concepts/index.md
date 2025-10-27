# Concepts

Concept guides introduce and explain the foundational ideas, components, and best practices that underpin the NautilusTrader platform.
These guides are designed to provide both conceptual and practical insights, helping you navigate the system's architecture, strategies, data management, execution flow, and more.
Explore the following guides to deepen your understanding and make the most of NautilusTrader.

## [Overview](overview.md)

The **Overview** guide covers the main features and intended use cases for the platform.

## [Architecture](architecture.md)

The **Architecture** guide dives deep into the foundational principles, structures, and designs that underpin
the platform. It is ideal for developers, system architects, or anyone curious about the inner workings of NautilusTrader.

## [Actors](actors.md)

The `Actor` serves as the foundational component for interacting with the trading system.
The **Actors** guide covers capabilities and implementation specifics.

## [Strategies](strategies.md)

The `Strategy` is at the heart of the NautilusTrader user experience when writing and working with
trading strategies. The **Strategies** guide covers how to implement strategies for the platform.

## [Instruments](instruments.md)

The **Instruments** guide covers the different instrument definition specifications for tradable assets and contracts.

## [Data](data.md)

The NautilusTrader platform defines a range of built-in data types crafted specifically to represent
a trading domain. The **Data** guide covers working with both built-in and custom data.

## [Execution](execution.md)

NautilusTrader can handle trade execution and order management for multiple strategies and venues
simultaneously (per instance). The **Execution** guide covers components involved in execution, as
well as the flow of execution messages (commands and events).

## [Orders](orders.md)

The **Orders** guide provides more details about the available order types for the platform, along with
the execution instructions supported for each. Advanced order types and emulated orders are also covered.

## [Positions](positions.md)

The **Positions** guide explains how positions work in NautilusTrader, including their lifecycle,
aggregation from order fills, profit and loss calculations, and the important concept of position
snapshotting for netting OMS configurations.

## [Cache](cache.md)

The `Cache` is a central in-memory data store for managing all trading-related data.
The **Cache** guide covers capabilities and best practices of the cache.

## [Message Bus](message_bus.md)

The `MessageBus` is the core communication system enabling decoupled messaging patterns between components,
including point-to-point, publish/subscribe, and request/response.
The **Message Bus** guide covers capabilities and best practices of the `MessageBus`.

## [Portfolio](portfolio.md)

The `Portfolio` serves as the central hub for managing and tracking all positions across active strategies for the trading node or backtest.
It consolidates position data from multiple instruments, providing a unified view of your holdings, risk exposure, and overall performance.
Explore this section to understand how NautilusTrader aggregates and updates portfolio state to support effective trading and risk management.

## [Reports](reports.md)

The **Reports** guide covers the reporting capabilities in NautilusTrader, including execution reports,
portfolio analysis reports, PnL accounting considerations, and how reports are used for backtest
post-run analysis.

## [Logging](logging.md)

The platform provides logging for both backtesting and live trading using a high-performance logger implemented in Rust.

## [Backtesting](backtesting.md)

Backtesting with NautilusTrader is a methodical simulation process that replicates trading
activities using a specific system implementation.

## [Visualization](visualization.md)

The **Visualization** guide covers the interactive tearsheet system for analyzing backtest
results, including available charts, themes, customization options, and how to create
custom visualizations using the extensible chart registry.

## [Live Trading](live.md)

Live trading in NautilusTrader enables traders to deploy their backtested strategies in real-time
without any code changes. This seamless transition ensures consistency and reliability, though there
are key differences between backtesting and live trading.

## [Adapters](adapters.md)

The NautilusTrader design allows for integrating data providers and/or trading venues through adapter implementations.
The **Adapters** guide covers requirements and best practices for developing new integration adapters for the platform.

:::note
The [API Reference](../api_reference/index.md) documentation should be considered the source of truth
for the platform. If there are any discrepancies between concepts described here and the API Reference,
then the API Reference should be considered the correct information. We are working to ensure that
concepts stay up-to-date with the API Reference and will be introducing doc tests in the near future
to help with this.
:::
