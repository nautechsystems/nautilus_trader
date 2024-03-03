# Databento

```{warning}
We are currently working on this integration guide - consider it incomplete.
```

NautilusTrader provides an adapter for integrating with the Databento API and [Databento Binary Encoding (DBN)](https://docs.databento.com/knowledge-base/new-users/dbn-encoding) format data.

The capabilities of this adapter include:
- Loading historical data from DBN files on disk into Nautilus objects for backtesting and writing to the data catalog
- Requesting historical data which is converted to Nautilus objects to support live trading and backtesting
- Subscribing to real-time data feeds which is converted to Nautilus objects to support live trading and sandbox environments

```{tip}
[Databento](https://databento.com/signup) currently offers 125 USD in free data credits (historical data only) for new account sign-ups.

With careful requests, this is more than enough for testing and evaluation purposes.
It's recommended you make use of the [/metadata.get_cost](https://docs.databento.com/api-reference-historical/metadata/metadata-get-cost) endpoint.
```

## Overview

The integrations implementation takes the [databento-rs](https://crates.io/crates/databento) crate as a dependency,
which is the official Rust client library provided by Databento. There are actually no Databento Python dependencies.

The following adapter classes are available:
- `DatabentoDataLoader` which allows loading Databento Binary Encoding (DBN) data from disk
- `DatabentoInstrumentProvider` which integrates with the Databento API (HTTP) to provide latest or historical instrument definitions
- `DatabentoHistoricalClient` which integrates with the Databento API (HTTP) for historical market data requests
- `DatabentoLiveClient` which integrates with the Databento API (raw TCP) for subscribing to real-time data feeds
- `DatabentoDataClient` providing a `LiveMarketDataClient` implementation for running a trading node in real time

```{note}
There is no optional extra installation for `databento`, at this stage the core components of the adapter are compiled
as static libraries and linked during the build by default.
```

## Documentation

Databento provides extensive documentation for users https://docs.databento.com/knowledge-base/new-users.
It's recommended you also refer to this documentation in conjunction with this Nautilus integration guide.

## Databento Binary Encoding (DBN)

The integration provides a decoder which can convert DBN format data to Nautilus objects.
You can read more about the DBN format [here](https://docs.databento.com/knowledge-base/new-users/dbn-encoding).

The same Rust implemented decoder is used for:
- Loading and decoding DBN files from disk
- Decoding historical and live data in real-time

## Supported schemas

The following Databento schemas are supported by NautilusTrader:

| Databento schema | Nautilus type                |
|------------------|------------------------------|
| MBO              | `OrderBookDelta`             |
| MBP_1            | `QuoteTick` + `TradeTick`    |
| MBP_10           | `OrderBookDepth10`           |
| TBBO             | `QuoteTick` + `TradeTick`    |
| TRADES           | `TradeTick`                  |
| OHLCV_1S         | `Bar`                        |
| OHLCV_1M         | `Bar`                        |
| OHLCV_1H         | `Bar`                        |
| OHLCV_1D         | `Bar`                        |
| DEFINITION       | `Instrument` (various types) |
| IMBALANCE        | `DatabentoImbalance` (under development)  |
| STATISTICS       | `DatabentoStatistics` (under development) |
| STATUS           | Not yet available                         |

## Performance considerations

When backtesting with Databento DBN data, there are two options:
- Store the data in DBN (`.dbn.zst`) format files and decode to Nautilus objects on every run
- Convert the DBN files to Nautilus Parquet format and write to the data catalog once (stored as Parquet on disk)

Whilst the DBN -> Nautilus decoder is implemented in Rust and has been optimized,
the best performance for backtesting will be achieved by writing the Nautilus
objects to the data catalog, which performs the decoding step once.

[DataFusion](https://arrow.apache.org/datafusion/) provides a query engine which is leveraged as a backend to load 
the Nautilus Parquet data from disk, which achieves extremely high through-put (at least an order of magnitude faster
than converting DBN -> Nautilus on the fly for every backtest run).

```{note}
Performance benchmarks are under development.
```

## Data types

The following section discusses Databento schema -> Nautilus data type equivalence
and considerations.

### Instrument definitions

Databento provides a single schema to cover all instrument classes, these are
decoded to the appropriate Nautilus `Instrument` types.

The following Databento instrument classes are supported by NautilusTrader:

| Databento instrument class | Nautilus instrument type     |
|----------------------------|------------------------------|
| BOND                       | Not yet available            |
| CALL                       | `OptionsContract`            |
| FUTURE                     | `FuturesContract`            |
| STOCK                      | `Equity`                     |
| MIXEDSPREAD                | `OptionsSpread`              |
| PUT                        | `OptionsContract`            |
| FUTURESPREAD               | `FuturesSpread`              |
| OPTIONSPREAD               | `OptionsSpread`              |
| FXSPOT                     | `CurrencyPair`               |

### MBO (market by order)

This schema is the highest granularity offered by Databento, and represents
full order book depth. Some messages also provide trade information, and so when
decoding MBO messages Nautilus will produce an `OrderBookDelta` and optionally a
`TradeTick`.

The Nautilus live data client will buffer MBO messages until an `F_LAST` flag
is seen. A discrete `OrderBookDeltas` container object will then be passed to the
registered handler.

Order book snapshots are also buffered into a discrete `OrderBookDeltas` container
object, which occurs during the replay startup sequence.

### MBP-1 (market by price, top-level)

This schema represents the top-of-book only. Like with MBO messages, some
messages carry trade information, and so when decoding MBP-1 messages Nautilus 
will produce a `QuoteTick` and optionally a `TradeTick`.

### OHLCV (bar aggregates)

The `ts_event` timestamps are normalized to bar close during Nautilus decoding.
