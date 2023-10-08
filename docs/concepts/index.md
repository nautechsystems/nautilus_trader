# Concepts

```{eval-rst}
.. toctree::
   :maxdepth: 1
   :glob:
   :titlesonly:
   :hidden:
   
   architecture.md
   strategies.md
   instruments.md
   adapters.md
   orders.md
   execution.md
   logging.md
   advanced/index.md
```

Welcome to NautilusTrader!

It's important to note that the [API Reference](../api_reference/index.md) documentation should be 
considered the source of truth for the platform. If there are any discrepancies between concepts described here
and the API Reference, then the API Reference should be considered the correct information. We are 
working to ensure that concepts stay up-to-date with the API Reference and will be introducing 
doc tests in the near future to help with this.

```{note}
The terms "NautilusTrader", "Nautilus" and "platform" are used interchageably throughout the documentation.
```

There are three main use cases for this software package:

- Backtesting trading systems with historical data (`backtest`)
- Testing trading systems with real-time data and simulated execution (`sandbox`)
- Deploying trading systems with real-time data and executing on venues with real (or paper) accounts (`live`)

The projects codebase provides a framework for implementing the software layer of systems which achieve the above. You will find
the default `backtest` and `live` system implementations in their respectively named subpackages. A `sandbox` environment can
be built using the sandbox adapter.

```{note}
All examples will utilize these default system implementations.
```

```{note}
We consider trading strategies to be subcomponents of end-to-end trading systems, these systems
include the application and infrastructure layers.
```

## Distributed
The platform is designed to be easily integrated into a larger distributed system. 
To facilitate this, nearly all configuration and domain objects can be serialized using JSON, MessagePack or Apache Arrow (Feather) for communication over the network.

## Common core
The common system core is utilized by both the backtest, sandbox, and live trading nodes. 
User-defined Actor and Strategy components are managed consistently across these environment contexts.

## Backtesting
Backtesting can be achieved by first making data available to a `BacktestEngine` either directly or via
a higher level `BacktestNode` and `ParquetDataCatalog`, and then running the data through the system with nanosecond resolution.

## Live trading
A `TradingNode` can ingest data and events from multiple data and execution clients. 
Live deployments can use both demo/paper trading accounts, or real accounts.

For live trading, a `TradingNode` can ingest data and events from multiple data and execution clients. 
The system supports both demo/paper trading accounts and real accounts. High performance can be achieved by running 
asynchronously on a single [event loop](https://docs.python.org/3/library/asyncio-eventloop.html), 
with the potential to further boost performance by leveraging the [uvloop](https://github.com/MagicStack/uvloop) implementation (available for Linux and macOS).

## Domain model
The platform features a comprehensive trading domain model that includes various value types such as 
`Price` and `Quantity`, as well as more complex entities such as `Order` and `Position` objects, 
which are used to aggregate multiple events to determine state.

### Data Types
The following market data types can be requested historically, and also subscribed to as live streams when available from a data publisher, and implemented in an integrations adapter.
- `OrderBookDelta`
- `OrderBookDeltas` (L1/L2/L3)
- `Ticker`
- `QuoteTick`
- `TradeTick`
- `Bar`
- `Instrument`
- `VenueStatus`
- `InstrumentStatus`
- `InstrumentClose`

The following PriceType options can be used for bar aggregations;
- `BID`
- `ASK`
- `MID`
- `LAST`

The following BarAggregation options are possible;
- `MILLISECOND`
- `SECOND`
- `MINUTE`
- `HOUR`
- `DAY`
- `WEEK`
- `MONTH`
- `TICK`
- `VOLUME`
- `VALUE` (a.k.a Dollar bars)
- `TICK_IMBALANCE`
- `TICK_RUNS`
- `VOLUME_IMBALANCE`
- `VOLUME_RUNS`
- `VALUE_IMBALANCE`
- `VALUE_RUNS`

The price types and bar aggregations can be combined with step sizes >= 1 in any way through a `BarSpecification`. 
This enables maximum flexibility and now allows alternative bars to be aggregated for live trading.

### Account Types
The following account types are available for both live and backtest environments;

- `Cash` single-currency (base currency)
- `Cash` multi-currency
- `Margin` single-currency (base currency)
- `Margin` multi-currency
- `Betting` single-currency

### Order Types
The following order types are available (when possible on an exchange);

- `MARKET`
- `LIMIT`
- `STOP_MARKET`
- `STOP_LIMIT`
- `MARKET_TO_LIMIT`
- `MARKET_IF_TOUCHED`
- `LIMIT_IF_TOUCHED`
- `TRAILING_STOP_MARKET`
- `TRAILING_STOP_LIMIT`

