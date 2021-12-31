# Core Concepts

NautilusTrader has been built from the ground up to deliver the highest quality 
performance and user experience. There are two main use cases for this software package.

- Backtesting trading strategies.
- Deploying trading strategies live.

## Backtesting
In our opinion, are two main reasons for conducting backtests on historical data;
Verify the logic of trading strategy implementations.
Getting an indication of likely performance if the alpha of the strategy remains into the future.
Backtesting with an event-driven engine such as NautilusTrader is not intended to be the primary research method for alpha discovery, however it can facilitate this.
One of the primary benefits of this platform is that the core machinery used inside the BacktestEngine is identical to the live trading system. This helps to ensure consistency between backtesting and live trading performance, when seeking to capitalize on alpha signals through a large sample size of trades, as expressed in the logic of the trading strategies.
Only a small amount of example data is available in the tests/test_kit/data directory of the repository - as used in the examples. There are many sources of financial market and other data, and it is left to the user to source this for backtesting purposes.
The platform is extremely flexible and open ended, you could inject dozens of different datasets into a BacktestEngine and run them simultaneously - with time being accurately simulated to nanosecond precision.

## Trading Live
A TradingNode hosts a fleet of trading strategies, with data able to be ingested from multiple data clients, and order execution through multiple execution clients.
Live deployments can use both demo/paper trading accounts, or real accounts.
Coming soon there will be further discussion of core concepts…

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
- `TICK_IMBALANCE` (TBA)
- `TICK_RUNS` (TBA)
- `VOLUME_IMBALANCE` (TBA)
- `VOLUME_RUNS` (TBA)
- `VALUE_IMBALANCE` (TBA)
- `VALUE_RUNS` (TBA)

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

- `Market`
- `Limit`
- `StopMarket`
- `StopLimit`

More will be added in due course including `MarketIfTouched`, and `LimitIfTouched`. 
Users are invited to open discussion issues to request specific order types or features.
