# Databento

```{warning}
We are currently working on this integration guide - consider it incomplete for now.
```

NautilusTrader provides an adapter for integrating with the Databento API and [Databento Binary Encoding (DBN)](https://docs.databento.com/knowledge-base/new-users/dbn-encoding) format data.
As Databento is purely a market data provider, there is no execution client provided - although a sandbox environment with simulated execution could still be set up.
It's also possible to match Databento data with Interactive Brokers execution, or to provide traditional asset class signals for crypto trading.

The capabilities of this adapter include:
- Loading historical data from DBN files and decoding into Nautilus objects for backtesting or writing to the data catalog
- Requesting historical data which is decoded to Nautilus objects to support live trading and backtesting
- Subscribing to real-time data feeds which are decoded to Nautilus objects to support live trading and sandbox environments

```{tip}
[Databento](https://databento.com/signup) currently offers 125 USD in free data credits (historical data only) for new account sign-ups.

With careful requests, this is more than enough for testing and evaluation purposes.
It's recommended you make use of the [/metadata.get_cost](https://docs.databento.com/api-reference-historical/metadata/metadata-get-cost) endpoint.
```

## Overview

The adapter implementation takes the [databento-rs](https://crates.io/crates/databento) crate as a dependency,
which is the official Rust client library provided by Databento 🦀. There are actually no Databento Python dependencies.

```{note}
There is no optional extra installation for `databento`, at this stage the core components of the adapter are compiled
as static libraries and linked during the build by default.
```

The following adapter classes are available:
- `DatabentoDataLoader` - Loads Databento Binary Encoding (DBN) data from files
- `DatabentoInstrumentProvider` - Integrates with the Databento API (HTTP) to provide latest or historical instrument definitions
- `DatabentoHistoricalClient` - Integrates with the Databento API (HTTP) for historical market data requests
- `DatabentoLiveClient` - Integrates with the Databento API (raw TCP) for subscribing to real-time data feeds
- `DatabentoDataClient` - Provides a `LiveMarketDataClient` implementation for running a trading node in real time

```{note}
As with the other integration adapters, most users will simply define a configuration for a live trading node (covered below),
and won't need to necessarily work with these lower level components individually.
```

## Documentation

Databento provides extensive documentation for users which can be found in the knowledge base https://docs.databento.com/knowledge-base/new-users.
It's recommended you also refer to the Databento documentation in conjunction with this Nautilus integration guide.

## Databento Binary Encoding (DBN)

The integration provides a decoder which can convert DBN format data to Nautilus objects.
You can read more about the DBN format [here](https://docs.databento.com/knowledge-base/new-users/dbn-encoding).

The same Rust implemented decoder is used for:
- Loading and decoding DBN files from disk
- Decoding historical and live data in real time

## Supported schemas

The following Databento schemas are supported by NautilusTrader:

| Databento schema | Nautilus data type           |
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
- Convert the DBN files to Nautilus objects and then write to the data catalog once (stored as Nautilus Parquet format on disk)

Whilst the DBN -> Nautilus decoder is implemented in Rust and has been optimized,
the best performance for backtesting will be achieved by writing the Nautilus
objects to the data catalog, which performs the decoding step once.

[DataFusion](https://arrow.apache.org/datafusion/) provides a query engine backend to efficiently load and stream
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
| STOCK                      | `Equity`                     |
| FUTURE                     | `FuturesContract`            |
| CALL                       | `OptionsContract`            |
| PUT                        | `OptionsContract`            |
| FUTURESPREAD               | `FuturesSpread`              |
| OPTIONSPREAD               | `OptionsSpread`              |
| MIXEDSPREAD                | `OptionsSpread`              |
| FXSPOT                     | `CurrencyPair`               |
| BOND                       | Not yet available            |

### MBO (market by order)

This schema is the highest granularity data offered by Databento, and represents
full order book depth. Some messages also provide trade information, and so when
decoding MBO messages Nautilus will produce an `OrderBookDelta` and optionally a
`TradeTick`.

The Nautilus live data client will buffer MBO messages until an `F_LAST` flag
is seen. A discrete `OrderBookDeltas` container object will then be passed to the
registered handler.

Order book snapshots are also buffered into a discrete `OrderBookDeltas` container
object, which occurs during the replay startup sequence.

### MBP-1 (market by price, top-of-book)

This schema represents the top-of-book only. Like with MBO messages, some
messages carry trade information, and so when decoding MBP-1 messages Nautilus 
will produce a `QuoteTick` and optionally a `TradeTick`.

### OHLCV (bar aggregates)

The Databento bar aggregation schemas are timestamped at the **open** of the bar interval.
The Nautilus decoder will normalize the `ts_event` timestamps to the **close** of the bar
(original `ts_event` + bar interval).

## Instrument IDs and symbology

Databento market data includes an `instrument_id` field which is an integer assigned
by either the original source venue, or internally by Databento during normalization.

It's important to realize that this is different to the Nautilus `InstrumentId`
which is a string made up of a symbol + venue with a period separator i.e. `"{symbol}.{venue}"`.

The Nautilus decoder will use the Databento `raw_symbol` for the Nautilus `symbol` and an [ISO 10383 MIC (Market Identification Code)](https://www.iso20022.org/market-identifier-codes)
from the Databento instrument definition message for the Nautilus `venue`.

Databento datasets are identified with a *dataset code* which is not the same
as a venue identifier. You can read more about Databento dataset naming conventions [here](https://docs.databento.com/api-reference-historical/basics/datasets).

Of particular note is for CME Globex MDP 3.0 data (`GLBX.MDP3` dataset code), the `venue` that
Nautilus will use is the CME exchange code provided by instrument definition messages (which the Interactive Brokers adapter can map):
- `CBCM` - XCME-XCBT inter-exchange spread
- `NYUM` - XNYM-DUMX inter-exchange spread
- `XCBT` - Chicago Board of Trade (CBOT)
- `XCEC` - Commodities Exchange Center (COMEX)
- `XCME` - Chicago Mercantile Exchange (CME)
- `XFXS` - CME FX Link spread
- `XNYM` - New York Mercantile Exchange (NYMEX)

Other venue MICs can be found in the `venue` field of responses from the [metadata.list_publishers](https://docs.databento.com/api-reference-historical/metadata/metadata-list-publishers?historical=http&live=python) endpoint.

## Configuration

The most common use case is to configure a live `TradingNode` to include a
Databento data client. To achieve this, add a `DATABENTO` section to your client
configuration(s):

```python
from nautilus_trader.adapters.databento import DATABENTO
from nautilus_trader.live.node import TradingNode

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        DATABENTO: {
            "api_key": None,  # 'DATABENTO_API_KEY' env var
            "http_gateway": None,  # Override for the default HTTP gateway
            "instrument_provider": InstrumentProviderConfig(load_all=True),
            "instrument_ids": None,  # Nautilus instrument IDs to load on start
            "parent_symbols": None,  # Databento parent symbols to load on start
        },
    },
    ..., # Omitted
)
```

Then, create a `TradingNode` and add the client factory:

```python
from nautilus_trader.adapters.databento.factories import DatabentoLiveDataClientFactory
from nautilus_trader.live.node import TradingNode

# Instantiate the live trading node with a configuration
node = TradingNode(config=config)

# Register the client factory with the node
node.add_data_client_factory(DATABENTO, DatabentoLiveDataClientFactory)

# Finally build the node
node.build()
```

### Configuration parameters

- `api_key` - The Databento API secret key. If ``None`` then will source the `DATABENTO_API_KEY` environment variable
- `http_gateway` - The historical HTTP client gateway override (useful for testing and typically not needed by most users)
- `live_gateway` - The live client gateway override (useful for testing and typically not needed by most users)
- `parent_symbols` - The Databento parent symbols to subscribe to instrument definitions for on start. This is a map of Databento dataset keys -> to a sequence of the parent symbols, e.g. {'GLBX.MDP3', ['ES.FUT', 'ES.OPT']} (for all E-mini S&P 500 futures and options products)
- `instrument_ids` - The instrument IDs to request instrument definitions for on start
- `timeout_initial_load` - The timeout (seconds) to wait for instruments to load (concurrently per dataset).
- `mbo_subscriptions_delay` - The timeout (seconds) to wait for MBO/L3 subscriptions (concurrently per dataset). After the timeout the MBO order book feed will start and replay messages from the start of the week which encompasses the initial snapshot and then all deltas

## Real-time client architecture

The `DatabentoDataClient` is a Python class which contains other Databento adapter classes.
There are two `DatabentoLiveClient`s per Databento dataset:
- One for MBO (order book deltas) real-time feeds
- One for all other real-time feeds

```{note}
There is currently a limitation that all MBO (order book deltas) subscriptions for a dataset have to be made at
node startup, to then be able to replay data from the beginning of the session. If subsequent subscriptions
arrive after start, then they will be ignored and an error logged.

There is no such limitation for any of the other Databento schemas.
```

A single `DatabentoHistoricalClient` instance is reused between the `DatabentoInstrumentProvider` and `DatabentoDataClient`,
which makes historical instrument definitions and data requests.
