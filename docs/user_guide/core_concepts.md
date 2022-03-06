# Core Concepts

NautilusTrader has been built from the ground up to deliver optimal
performance with a high quality user experience, within the bounds of a robust Python native environment. There are two main use cases for this software package:

- Backtesting trading strategies
- Deploying trading strategies live

## System Architecture
From a high level architectural view, it's important to understand that the platform has been designed to run efficiently 
on a single thread, for both backtesting and live trading. A lot of research and testing
resulted in arriving at this design, as it was found the overhead of context switching between threads
didn't pay off in better performance.

For live trading, extremely high performance (benchmarks pending) can be achieved running asynchronously on a single [event loop](https://docs.python.org/3/library/asyncio-eventloop.html), 
especially leveraging the [uvloop](https://github.com/MagicStack/uvloop) implementation (available for Linux and macOS only).

```{note}
Of interest is the LMAX exchange architectire, which achieves award winning performance running on
a single thread. You can read about their _disruptor_ pattern based architecture in [this interesting article](https://martinfowler.com/articles/lmax.html) by Martin Fowler.
```

When considering the logic of how your trading will work within the system boundary, you can expect each component to consume messages
in a predictable synchronous way (_similar_ to the [actor model](https://en.wikipedia.org/wiki/Actor_model)).

## Trading Live
A `TradingNode` can host a fleet of trading strategies, with data able to be ingested from multiple data clients, and order execution handled through multiple execution clients.
Live deployments can use both demo/paper trading accounts, or real accounts.

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
- `SECOND`
- `MINUTE`
- `HOUR`
- `DAY`
- `TICK`
- `VOLUME`
- `VALUE` (a.k.a Dollar bars)
- `TICK_IMBALANCE`
- `TICK_RUNS`
- `VOLUME_IMBALANCE`
- `VOLUME_RUNS`
- `VALUE_IMBALANCE`
- `VALUE_RUNS`

The price types and bar aggregations can be combined with step sizes >= 1 in any way through `BarSpecification`. 
This enables maximum flexibility and now allows alternative bars to be produced for live trading.

## Account Types
The following account types are available for both live and backtest environments;

- `Cash` single-currency (base currency).
- `Cash` multi-currency.
- `Margin` single-currency (base currency).
- `Margin` multi-currency.

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
