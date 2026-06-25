# Tutorials

Step-by-step walkthroughs demonstrating specific features and workflows.

:::info
Most Python tutorials are Jupytext percent-format files in the docs
[tutorials directory](https://github.com/nautechsystems/nautilus_trader/tree/develop/docs/tutorials).
You can run those directly as scripts or open them as notebooks with Jupytext.
Rust tutorials use the commands shown on their pages.
:::

:::tip

- **Latest**: docs built from the `master` branch for stable releases.
  See <https://nautilustrader.io/docs/latest/tutorials/>.
- **Nightly**: docs built from the `nightly` branch for experimental features.
  See <https://nautilustrader.io/docs/nightly/tutorials/>.

:::

## Recommended order

New to NautilusTrader? Work through these in sequence:

1. [Quickstart](../getting_started/quickstart) - run your first backtest in five minutes
   with synthetic data
2. [Backtest (low-level API)](../getting_started/backtest_low_level) - direct
   `BacktestEngine` usage with real market data and execution algorithms
3. [Backtest (high-level API)](../getting_started/backtest_high_level) - config-driven
   backtesting with `BacktestNode` and the Parquet data catalog
4. [Loading external data][loading_external_data] - load CSV or other external data
   into the `ParquetDataCatalog` (how-to guide)
5. [Backtest with FX bar data][backtest_fx_bars] - FX bar backtesting with rollover
   interest simulation
6. Pick a topic-specific tutorial below

## Backtesting

| Tutorial                                                                            | Description                                    | Data          |
|:------------------------------------------------------------------------------------|:-----------------------------------------------|:--------------|
| [Backtest with FX Bar Data][backtest_fx_bars]                                       | EMA cross on FX bars with rollover simulation. | Bundled       |
| [Backtest with Order Book Depth Data (Binance)][backtest_orderbook_binance]         | Order book imbalance strategy on depth data.   | User‑provided |
| [Backtest with Order Book Depth Data (Bybit)][backtest_orderbook_bybit]             | Order book imbalance strategy on depth data.   | User‑provided |

## Data workflows

For task-oriented data recipes, see the [how-to guides](../how_to/):

| Guide                                                                               | Description                                       | Data              |
|:------------------------------------------------------------------------------------|:--------------------------------------------------|:------------------|
| [Loading external data][loading_external_data]                                      | Load external data into the `ParquetDataCatalog`. | User‑provided     |
| [Data catalog with Databento][data_catalog_databento]                               | Set up a catalog with Databento schemas.          | Databento API key |

## Strategy patterns

| Tutorial                                                                            | Description                                       | Data              |
|:------------------------------------------------------------------------------------|:--------------------------------------------------|:------------------|
| [Mean Reversion with Proxy FX Data (AX Exchange)](fx_mean_reversion_ax)             | Bollinger Band mean reversion on EURUSD‑PERP.     | TrueFX proxy      |
| [Gold Perpetual Book Imbalance (AX Exchange)](gold_book_imbalance_ax)               | Order book imbalance on XAU‑PERP.                 | Databento API key |
| [Grid Market Making with a Deadman's Switch (BitMEX)](grid_market_maker_bitmex)     | Grid MM with server‑side safety on XBTUSD.        | Tardis.dev        |
| [On‑Chain Grid Market Making with Short‑Term Orders (dYdX)](grid_market_maker_dydx) | Grid MM on dYdX v4 perpetuals.                    | User‑provided     |

## Options

| Tutorial                                                                            | Description                                       | Data              |
|:------------------------------------------------------------------------------------|:--------------------------------------------------|:------------------|
| [Options Data and Greeks (Bybit)](options_data_bybit)                               | Stream Greeks and option chain snapshots.         | Live API          |
| [Delta‑Neutral Options Strategy (Bybit)](delta_neutral_options_bybit)               | Short strangle with perpetual delta hedging.      | Live API          |
| [Delta‑Neutral Options Strategy (Derive)](delta_neutral_options_derive)             | Derive ETH strangle hedger with premium entry.    | Live API          |

## Rust

| Tutorial                                                                                    | Description                                          | Data                |
|:--------------------------------------------------------------------------------------------|:-----------------------------------------------------|:--------------------|
| [Book Imbalance Backtest (Betfair)](backtest_book_imbalance_betfair)                        | Book imbalance actor on Betfair L2 data.             | User‑provided       |
| [Composite Market Making on Lighter RWA with Databento DBEQ NVDA](lighter_rwa_composite_mm) | Signal‑skewed MM on NVDA‑PERP.                       | Databento + Lighter |
| [Hurst/VPIN Directional Strategy (Kraken Futures)](hurst_vpin_kraken)                       | Regime‑filtered informed‑flow strategy on PF_XBTUSD. | Tardis.dev          |

[backtest_fx_bars]: https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/tutorials/backtest_fx_bars.py
[backtest_orderbook_binance]: https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/tutorials/backtest_orderbook_binance.py
[backtest_orderbook_bybit]: https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/tutorials/backtest_orderbook_bybit.py
[loading_external_data]: https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/how_to/loading_external_data.py
[data_catalog_databento]: https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/how_to/data_catalog_databento.py
