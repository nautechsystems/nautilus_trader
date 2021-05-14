Core Concepts
=============

NautilusTrader has been built from the ground up to deliver the
highest quality performance and user experience.

There are two main use cases for this software package.

- Backtesting trading strategies
- Deploying trading strategies live

Backtesting
-----------
There are two main reasons for conducting backtests on historical data;

- Verify the logic of trading strategy implementations
- Getting an indication of likely performance **if the alpha of the strategy remains into the future**.

Backtesting with an event-driven engine such as NautilusTrader is not meant to be the primary
research method for alpha discovery, but merely facilitates the above.

One of the primary benefits of this platform is that the machinery used inside
the backtest engine is identical for live trading, apart from the live versions
of the data and execution engines closer to the periphery of the system (they
add asyncio queues on top of the common implementations), and integrations with
external endpoints through adapters provided with this package (and/or developed
by end users).

This helps ensure consistency when seeking to capitalize on alpha through a large
sample size of trades, as expressed in the logic of the trading strategies.

Only a small amount of example data is available in the ``test`` directory of
the repository - as used in the examples. There are many sources of financial
market data, and it is left to the user to supply this for backtesting purposes.

The platform is extremely flexible and open ended, you could inject dozens of
different datasets into a backtest engine, running them simultaneously with time
being accurately simulated to nanosecond precision.

Trading Live
------------
A ``TradingNode`` hosts a fleet of trading strategies, with data able to be
ingested from multiple data clients, and order execution through multiple
execution clients.

Live deployments can use both demo/paper trading accounts, or real accounts.

Coming soon there will be further discussion of core concepts...

**work in progress**
