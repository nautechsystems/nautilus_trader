# Databento

NautilusTrader provides an adapter for integrating with the Databento API and [Databento Binary Encoding (DBN)](https://databento.com/docs/standards-and-conventions/databento-binary-encoding) format data.
As Databento is purely a market data provider, there is no execution client provided - although a sandbox environment with simulated execution could still be set up.
It's also possible to match Databento data with Interactive Brokers execution, or to calculate traditional asset class signals for crypto trading.

The capabilities of this adapter include:

- Loading historical data from DBN files and decoding into Nautilus objects for backtesting or writing to the data catalog.
- Requesting historical data which is decoded to Nautilus objects to support live trading and backtesting.
- Subscribing to real-time data feeds which are decoded to Nautilus objects to support live trading and sandbox environments.

:::tip
[Databento](https://databento.com/signup) currently offers 125 USD in free data credits (historical data only) for new account sign-ups.

With careful requests, this is more than enough for testing and evaluation purposes.
We recommend you make use of the [/metadata.get_cost](https://databento.com/docs/api-reference-historical/metadata/metadata-get-cost) endpoint.
:::

## Overview

The adapter implementation takes the [databento-rs](https://crates.io/crates/databento) crate as a dependency,
which is the official Rust client library provided by Databento.

:::info
There is **no** need for an optional extra installation of `databento`, as the core components of the
adapter are compiled as static libraries and linked automatically during the build process.
:::

The following adapter classes are available:

- `DatabentoDataLoader`: Loads Databento Binary Encoding (DBN) data from files.
- `DatabentoInstrumentProvider`: Integrates with the Databento API (HTTP) to provide latest or historical instrument definitions.
- `DatabentoHistoricalClient`: Integrates with the Databento API (HTTP) for historical market data requests.
- `DatabentoLiveClient`: Integrates with the Databento API (raw TCP) for subscribing to real-time data feeds.
- `DatabentoDataClient`: Provides a `LiveMarketDataClient` implementation for running a trading node in real time.

:::info
As with the other integration adapters, most users will simply define a configuration for a live trading node (covered below),
and won't need to necessarily work with these lower level components directly.
:::

## Examples

You can find functional live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/databento/).

## Databento documentation

Databento provides extensive documentation for new users which can be found in the [Databento new users guide](https://databento.com/docs/quickstart/new-user-guides).
We recommend also referring to the Databento documentation in conjunction with this NautilusTrader integration guide.

## Databento Binary Encoding (DBN)

Databento Binary Encoding (DBN) is an extremely fast message encoding and storage format for normalized market data.
The [DBN specification](https://databento.com/docs/standards-and-conventions/databento-binary-encoding) includes a simple, self-describing metadata header and a fixed set of struct definitions,
which enforce a standardized way to normalize market data.

The integration provides a decoder which can convert DBN format data to Nautilus objects.

The same Rust implemented Nautilus decoder is used for:
- Loading and decoding DBN files from disk
- Decoding historical and live data in real time

## Supported schemas

The following Databento schemas are supported by NautilusTrader:

| Databento schema | Nautilus data type                |
|:-----------------|:----------------------------------|
| MBO              | `OrderBookDelta`                  |
| MBP_1            | `(QuoteTick, Option<TradeTick>)`  |
| MBP_10           | `OrderBookDepth10`                |
| BBO_1S           | `QuoteTick`                       |
| BBO_1M           | `QuoteTick`                       |
| TBBO             | `(QuoteTick, TradeTick)`          |
| TRADES           | `TradeTick`                       |
| OHLCV_1S         | `Bar`                             |
| OHLCV_1M         | `Bar`                             |
| OHLCV_1H         | `Bar`                             |
| OHLCV_1D         | `Bar`                             |
| DEFINITION       | `Instrument` (various types)      |
| IMBALANCE        | `DatabentoImbalance`              |
| STATISTICS       | `DatabentoStatistics`             |
| STATUS           | `InstrumentStatus`                |

:::info
See also the Databento [Schemas and data formats](https://databento.com/docs/schemas-and-data-formats) guide.
:::

## Instrument IDs and symbology

Databento market data includes an `instrument_id` field which is an integer assigned
by either the original source venue, or internally by Databento during normalization.

It's important to realize that this is different to the Nautilus `InstrumentId`
which is a string made up of a symbol + venue with a period separator i.e. `"{symbol}.{venue}"`.

The Nautilus decoder will use the Databento `raw_symbol` for the Nautilus `symbol` and an [ISO 10383 MIC](https://www.iso20022.org/market-identifier-codes) (Market Identifier Code)
from the Databento instrument definition message for the Nautilus `venue`.

Databento datasets are identified with a *dataset code* which is not the same
as a venue identifier. You can read more about Databento dataset naming conventions [here](https://databento.com/docs/api-reference-historical/basics/datasets).

Of particular note is for CME Globex MDP 3.0 data (`GLBX.MDP3` dataset code), the following
exchanges are all grouped under the `GLBX` venue. These mappings can be determined from the
instruments `exchange` field:
- `CBCM`: XCME-XCBT inter-exchange spread
- `NYUM`: XNYM-DUMX inter-exchange spread
- `XCBT`: Chicago Board of Trade (CBOT)
- `XCEC`: Commodities Exchange Center (COMEX)
- `XCME`: Chicago Mercantile Exchange (CME)
- `XFXS`: CME FX Link spread
- `XNYM`: New York Mercantile Exchange (NYMEX)

:::info
Other venue MICs can be found in the `venue` field of responses from the [metadata.list_publishers](https://databento.com/docs/api-reference-historical/metadata/metadata-list-publishers) endpoint.
:::

## Timestamps

Databento data includes various timestamp fields including (but not limited to):

- `ts_event`: The matching-engine-received timestamp expressed as the number of nanoseconds since the UNIX epoch.
- `ts_in_delta`: The matching-engine-sending timestamp expressed as the number of nanoseconds before `ts_recv`.
- `ts_recv`: The capture-server-received timestamp expressed as the number of nanoseconds since the UNIX epoch.
- `ts_out`: The Databento sending timestamp.

Nautilus data includes at *least* two timestamps (required by the `Data` contract):

- `ts_event`: UNIX timestamp (nanoseconds) when the data event occurred
- `ts_init`: UNIX timestamp (nanoseconds) when the data object was initialized

When decoding and normalizing Databento to Nautilus we generally assign the Databento `ts_recv` value to the Nautilus
`ts_event` field, as this timestamp is much more reliable and consistent, and is guaranteed to be monotonically increasing per instrument.
The exception to this are the `DatabentoImbalance` and `DatabentoStatistics` data types, which have fields for all timestamps as these types are defined specifically for the adapter.

:::info
See the following Databento docs for further information:
- [Databento standards and conventions - timestamps](https://databento.com/docs/standards-and-conventions/common-fields-enums-types#timestamps)
- [Databento timestamping guide](https://databento.com/docs/architecture/timestamping-guide)
:::

## Data types

The following section discusses Databento schema -> Nautilus data type equivalence
and considerations.

:::info
See Databento [schemas and data formats](https://databento.com/docs/schemas-and-data-formats).
:::

### Instrument definitions

Databento provides a single schema to cover all instrument classes, these are
decoded to the appropriate Nautilus `Instrument` types.

The following Databento instrument classes are supported by NautilusTrader:

| Databento instrument class | Code |  Nautilus instrument type    |
|----------------------------|------|------------------------------|
| Stock                      | `K`  | `Equity`                     |
| Future                     | `F`  | `FuturesContract`            |
| Call                       | `C`  | `OptionContract`             |
| Put                        | `P`  | `OptionContract`             |
| Future spread              | `S`  | `FuturesSpread`              |
| Option spread              | `T`  | `OptionSpread`               |
| Mixed spread               | `M`  | `OptionSpread`               |
| FX spot                    | `X`  | `CurrencyPair`               |
| Bond                       | `B`  | Not yet available            |

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

This schema represents the top-of-book only (quotes *and* trades). Like with MBO messages, some
messages carry trade information, and so when decoding MBP-1 messages Nautilus
will produce a `QuoteTick` and *also* a `TradeTick` if the message is a trade.

### OHLCV (bar aggregates)

The Databento bar aggregation messages are timestamped at the **open** of the bar interval.
The Nautilus decoder will normalize the `ts_event` timestamps to the **close** of the bar
(original `ts_event` + bar interval).

### Imbalance & Statistics

The Databento `imbalance` and `statistics` schemas cannot be represented as a built-in Nautilus data types,
and so they have specific types defined in Rust `DatabentoImbalance` and `DatabentoStatistics`.
Python bindings are provided via pyo3 (Rust) so the types behave a little differently to a built-in Nautilus
data types, where all attributes are pyo3 provided objects and not directly compatible
with certain methods which may expect a Cython provided type. There are pyo3 -> legacy Cython
object conversion methods available, which can be found in the API reference.

Here is a general pattern for converting a pyo3 `Price` to a Cython `Price`:
```python
price = Price.from_raw(pyo3_price.raw, pyo3_price.precision)
```

Additionally requesting for and subscribing to these data types requires the use of the
lower level generic methods for custom data types. The following example subscribes to the `imbalance`
schema for the `AAPL.XNAS` instrument (Apple Inc trading on the Nasdaq exchange):

```python
from nautilus_trader.adapters.databento import DATABENTO_CLIENT_ID
from nautilus_trader.adapters.databento import DatabentoImbalance
from nautilus_trader.model import DataType

instrument_id = InstrumentId.from_str("AAPL.XNAS")
self.subscribe_data(
    data_type=DataType(DatabentoImbalance, metadata={"instrument_id": instrument_id}),
    client_id=DATABENTO_CLIENT_ID,
)
```

Or requesting the previous days `statistics` schema for the `ES.FUT` parent symbol (all active E-mini S&P 500 futures contracts on the CME Globex exchange):

```python
from nautilus_trader.adapters.databento import DATABENTO_CLIENT_ID
from nautilus_trader.adapters.databento import DatabentoStatisics
from nautilus_trader.model import DataType

instrument_id = InstrumentId.from_str("ES.FUT.GLBX")
metadata = {
    "instrument_id": instrument_id,
    "start": "2024-03-06",
}
self.request_data(
    data_type=DataType(DatabentoImbalance, metadata=metadata),
    client_id=DATABENTO_CLIENT_ID,
)
```

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

:::note
Performance benchmarks are currently under development.
:::

## Loading DBN data

You can load DBN files and convert the records to Nautilus objects using the
`DatabentoDataLoader` class. There are two main purposes for doing so:

- Pass the converted data to `BacktestEngine.add_data` directly for backtesting.
- Pass the converted data to `ParquetDataCatalog.write_data` for later streaming use with a `BacktestNode`.

### DBN data to a BacktestEngine

This code snippet demonstrates how to load DBN data and pass to a `BacktestEngine`.
Since the `BacktestEngine` needs an instrument added, we'll use a test instrument
provided by the `TestInstrumentProvider` (you could also pass an instrument object
which was parsed from a DBN file too).
The data is a month of TSLA (Tesla Inc) trades on the Nasdaq exchange:

```python
# Add instrument
TSLA_NASDAQ = TestInstrumentProvider.equity(symbol="TSLA")
engine.add_instrument(TSLA_NASDAQ)

# Decode data to legacy Cython objects
loader = DatabentoDataLoader()
trades = loader.from_dbn_file(
    path=TEST_DATA_DIR / "databento" / "temp" / "tsla-xnas-20240107-20240206.trades.dbn.zst",
    instrument_id=TSLA_NASDAQ.id,
)

# Add data
engine.add_data(trades)
```

### DBN data to a ParquetDataCatalog

This code snippet demonstrates how to load DBN data and write to a `ParquetDataCatalog`.
We pass a value of false for the `as_legacy_cython` flag, which will ensure the
DBN records are decoded as pyo3 (Rust) objects. It's worth noting that legacy Cython
objects can also be passed to `write_data`, but these need to be converted back to
pyo3 objects under the hood (so passing pyo3 objects is an optimization).

```python
# Initialize the catalog interface
# (will use the `NAUTILUS_PATH` env var as the path)
catalog = ParquetDataCatalog.from_env()

instrument_id = InstrumentId.from_str("TSLA.XNAS")

# Decode data to pyo3 objects
loader = DatabentoDataLoader()
trades = loader.from_dbn_file(
    path=TEST_DATA_DIR / "databento" / "temp" / "tsla-xnas-20240107-20240206.trades.dbn.zst",
    instrument_id=instrument_id,
    as_legacy_cython=False,  # This is an optimization for writing to the catalog
)

# Write data
catalog.write_data(trades)
```

:::info
See also the [Data concepts guide](../concepts/data.md).
:::

## Real-time client architecture

The `DatabentoDataClient` is a Python class which contains other Databento adapter classes.
There are two `DatabentoLiveClient`s per Databento dataset:
- One for MBO (order book deltas) real-time feeds
- One for all other real-time feeds

:::warning
There is currently a limitation that all MBO (order book deltas) subscriptions for a dataset have to be made at
node startup, to then be able to replay data from the beginning of the session. If subsequent subscriptions
arrive after start, then an error will be logged (and the subscription ignored).

There is no such limitation for any of the other Databento schemas.
:::

A single `DatabentoHistoricalClient` instance is reused between the `DatabentoInstrumentProvider` and `DatabentoDataClient`,
which makes historical instrument definitions and data requests.

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
            "http_gateway": None,  # Override for the default HTTP historical gateway
            "live_gateway": None,  # Override for the default raw TCP real-time gateway
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

- `api_key`: The Databento API secret key. If ``None`` then will source the `DATABENTO_API_KEY` environment variable.
- `http_gateway`: The historical HTTP client gateway override (useful for testing and typically not needed by most users).
- `live_gateway`: The raw TCP real-time client gateway override (useful for testing and typically not needed by most users).
- `parent_symbols`: The Databento parent symbols to subscribe to instrument definitions for on start. This is a map of Databento dataset keys -> to a sequence of the parent symbols, e.g. {'GLBX.MDP3', ['ES.FUT', 'ES.OPT']} (for all E-mini S&P 500 futures and options products).
- `instrument_ids`: The instrument IDs to request instrument definitions for on start.
- `timeout_initial_load`: The timeout (seconds) to wait for instruments to load (concurrently per dataset).
- `mbo_subscriptions_delay`: The timeout (seconds) to wait for MBO/L3 subscriptions (concurrently per dataset). After the timeout the MBO order book feed will start and replay messages from the initial snapshot and then all deltas.

:::tip
We recommend using environment variables to manage your credentials.
:::
