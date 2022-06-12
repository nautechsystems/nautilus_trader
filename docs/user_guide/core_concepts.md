# Core Concepts

NautilusTrader has been built from the ground up to deliver optimal
performance with a high quality user experience, within the bounds of a robust Python native environment. There are two main use cases for this software package:

- Backtesting trading strategies
- Deploying trading strategies live

The projects codebase provides a framework for implementing systems to achieve the above. You will find
the default `backtest` and `live` system implementations in their respectively named subpackages. All examples
will also either utilize the default backtest or live system implementations.

## Trading Live
A `TradingNode` can host a fleet of trading strategies, with data able to be ingested from multiple data clients, and order execution handled through multiple execution clients.
Live deployments can use both demo/paper trading accounts, or real accounts.

For live trading, extremely high performance (benchmarks pending) can be achieved running asynchronously on a single [event loop](https://docs.python.org/3/library/asyncio-eventloop.html), 
especially leveraging the [uvloop](https://github.com/MagicStack/uvloop) implementation (available for Linux and macOS only).

## Data Types
The following market data types can be requested historically, and also subscribed to as live streams when available from a data publisher, and implemented in an integrations adapter.
- `OrderBookDelta`
- `OrderBookDeltas` (L1/L2/L3)
- `OrderBookSnapshot` (L1/L2/L3)
- `QuoteTick`
- `TradeTick`
- `Bar`
- `Instrument`

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

## Account Types
The following account types are available for both live and backtest environments;

- `Cash` single-currency (base currency)
- `Cash` multi-currency
- `Margin` single-currency (base currency)
- `Margin` multi-currency

## Order Types
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
