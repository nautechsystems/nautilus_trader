# Tutorials

Step-by-step walkthroughs demonstrating specific features and workflows.

:::info
Each tutorial is a Jupytext percent-format Python file in the docs
[tutorials directory](https://github.com/nautechsystems/nautilus_trader/tree/develop/docs/tutorials).
You can run them directly as scripts or open them as notebooks with Jupytext.
:::

## Recommended order

New to NautilusTrader? Work through these in sequence:

1. [Quickstart](../getting_started/quickstart) - run your first backtest in five minutes
   with synthetic data
2. [Backtest (low-level API)](../getting_started/backtest_low_level) - direct
   `BacktestEngine` usage with real market data and execution algorithms
3. [Backtest (high-level API)](../getting_started/backtest_high_level) - config-driven
   backtesting with `BacktestNode` and the Parquet data catalog
4. [Loading external data](loading_external_data) - load CSV or other external data
   into the `ParquetDataCatalog`
5. [Backtest with FX bar data](backtest_fx_bars) - FX bar backtesting with rollover
   interest simulation
6. Pick a topic-specific tutorial below

## Backtesting

| Tutorial                                                                        | Description                                        | Data          |
|:--------------------------------------------------------------------------------|:---------------------------------------------------|:--------------|
| [Backtest with FX bar data](backtest_fx_bars)                                   | EMA cross on FX bars with rollover simulation      | Bundled       |
| [Backtest with order book depth data (Binance)](backtest_orderbook_binance)     | Order book imbalance strategy on depth data        | User-provided |
| [Backtest with order book depth data (Bybit)](backtest_orderbook_bybit)         | Order book imbalance strategy on depth data        | User-provided |

## Data workflows

| Tutorial                                                     | Description                                       | Data              |
|:-------------------------------------------------------------|:--------------------------------------------------|:------------------|
| [Loading external data](loading_external_data)               | Load external data into the `ParquetDataCatalog`  | User-provided     |
| [Data catalog with Databento](data_catalog_databento)        | Set up a catalog with Databento schemas           | Databento API key |

## Strategy patterns

| Tutorial                                                                                   | Description                                  | Data              |
|:-------------------------------------------------------------------------------------------|:---------------------------------------------|:------------------|
| [Mean reversion with proxy FX data (AX Exchange)](fx_mean_reversion_ax)                    | Bollinger Band mean reversion on EURUSD-PERP | TrueFX proxy      |
| [Gold perpetual book imbalance (AX Exchange)](gold_book_imbalance_ax)                      | Order book imbalance on XAU-PERP             | Databento API key |
| [Grid market making with a deadman's switch (BitMEX)](grid_market_maker_bitmex)            | Grid MM with server-side safety on XBTUSD    | Tardis.dev        |
| [On-chain grid market making with short-term orders (dYdX)](grid_market_maker_dydx)        | Grid MM on dYdX v4 perpetuals                | User-provided     |

:::tip

- **Latest**: docs built from the `master` branch for stable releases.
  See <https://nautilustrader.io/docs/latest/tutorials/>.
- **Nightly**: docs built from the `nightly` branch for experimental features.
  See <https://nautilustrader.io/docs/nightly/tutorials/>.

:::
