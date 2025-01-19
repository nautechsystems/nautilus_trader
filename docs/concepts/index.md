# Concepts

Explore the foundational concepts of NautilusTrader through the following guides.

## [Overview](overview.md)

The overview guide covers the main features and use cases for the platform.

## [Architecture](architecture.md)

The architecture guide dives deep into the foundational principles, structures, and designs that underpin
the platform. Whether you're a developer, system architect, or just curious about the inner workings
of NautilusTrader.

## [Strategies](strategies.md)

The heart of the NautilusTrader user experience is in writing and working with
trading strategies. The **Strategies** guide covers how to implement trading strategies for the platform.

## [Instruments](instruments.md)

The instrument definitions provide the specification for any tradable asset/contract.

## [Data](data.md)

The NautilusTrader platform defines a range of built-in data types crafted specifically to represent
a trading domain

## [Execution](execution.md)

NautilusTrader can handle trade execution and order management for multiple strategies and venues
simultaneously (per instance). Several interacting components are involved in execution, making it
crucial to understand the possible flows of execution messages (commands and events).

## [Orders](orders.md)

The orders guide provides more details about the available order types for the platform, along with
the execution instructions supported for each.

## [Cache](cache.md)

The Cache is a central in-memory database, that automatically stores and manages all trading-related data.

## [Message Bus](message_bus.md)

The core communication system enabling decoupled messaging patterns between components, including
point-to-point, publish/subscribe, and request/response.

## [Logging](logging.md)

The platform provides logging for both backtesting and live trading using a high-performance logger implemented in Rust.

## [Backtesting](backtesting.md)

Backtesting with NautilusTrader is a methodical simulation process that replicates trading
activities using a specific system implementation.

## [Live trading](live.md)

Live trading in NautilusTrader enables traders to deploy their backtested strategies in real-time
without any code changes. This seamless transition ensures consistency and reliability, though there
key differences between backtesting and live trading.

## [Adapters](adapters.md)

The NautilusTrader design allows for integrating data providers and/or trading venues
through adapter implementations, these can be found in the top level `adapters` subpackage.

## [Advanced](advanced/index.md)

Here you will find more detailed documentation and examples covering the more advanced
features and functionality of the platform.

:::note
The [API Reference](../api_reference/index.md) documentation should be considered the source of truth
for the platform. If there are any discrepancies between concepts described here and the API Reference,
then the API Reference should be considered the correct information. We are working to ensure that
concepts stay up-to-date with the API Reference and will be introducing doc tests in the near future
to help with this.
:::
