# Tardis

Tardis provides granular data for cryptocurrency markets including tick-by-tick order book snapshots & updates,
trades, open interest, funding rates, options chains and liquidations data for leading crypto exchanges.

NautilusTrader provides an integration with the Tardis API and data formats, enabling seamless access.
The capabilities of this adapter include:

- `TardisCSVDataLoader`: Reads Tardis-format CSV files and converts them into Nautilus data.
- `TardisMachineClient`: Supports live streaming and historical replay of data from the Tardis Machine WebSocket server - converting messages into Nautilus data.
- `TardisHttpClient`: Requests instrument definition metadata from the Tardis HTTP API, parsing it into Nautilus instrument definitions.
- `TardisDataClient`: Provides a live data client for subscribing to data streams from a Tardis Machine WebSocket server.
- `TardisInstrumentProvider`: Provides instrument definitions from Tardis through the HTTP instrument metadata API.
- **Data pipeline functions**: Enables replay of historical data from Tardis Machine and writes it to the Nautilus Parquet format, including direct catalog integration for streamlined data management (see below).

:::info
A Tardis API key is required for the adapter to operate correctly. See also [environment variables](#environment-variables).
:::

## Overview

This adapter is implemented in Rust, with optional Python bindings for ease of use in Python-based workflows.
It does not require any external Tardis client library dependencies.

:::info
There is **no** need for additional installation steps for `tardis`.
The core components of the adapter are compiled as static libraries and automatically linked during the build process.
:::

## Tardis documentation

Tardis provides extensive user [documentation](https://docs.tardis.dev/).
We recommend also referring to the Tardis documentation in conjunction with this NautilusTrader integration guide.

## Supported formats

Tardis provides *normalized* market data—a unified format consistent across all supported exchanges.
This normalization is highly valuable because it allows a single parser to handle data from any [Tardis-supported exchange](#venues), reducing development time and complexity.
As a result, NautilusTrader will not support exchange-native market data formats, as it would be inefficient to implement separate parsers for each exchange at this stage.

The following normalized Tardis formats are supported by NautilusTrader:

| Tardis format                                                                                                               | Nautilus data type                                                   |
|:----------------------------------------------------------------------------------------------------------------------------|:---------------------------------------------------------------------|
| [book_change](https://docs.tardis.dev/api/tardis-machine#book_change)                                                       | `OrderBookDelta`                                                     |
| [book_snapshot_*](https://docs.tardis.dev/api/tardis-machine#book_snapshot_-number_of_levels-_-snapshot_interval-time_unit) | `OrderBookDepth10`                                                   |
| [quote](https://docs.tardis.dev/api/tardis-machine#book_snapshot_-number_of_levels-_-snapshot_interval-time_unit)           | `QuoteTick`                                                          |
| [quote_10s](https://docs.tardis.dev/api/tardis-machine#book_snapshot_-number_of_levels-_-snapshot_interval-time_unit)       | `QuoteTick`                                                          |
| [trade](https://docs.tardis.dev/api/tardis-machine#trade)                                                                   | `Trade`                                                              |
| [trade_bar_*](https://docs.tardis.dev/api/tardis-machine#trade_bar_-aggregation_interval-suffix)                            | `Bar`                                                                |
| [instrument](https://docs.tardis.dev/api/instruments-metadata-api)                                                          | `CurrencyPair`, `CryptoFuture`, `CryptoPerpetual`, `OptionContract` |
| [derivative_ticker](https://docs.tardis.dev/api/tardis-machine#derivative_ticker)                                           | *Not yet supported*                                                  |
| [disconnect](https://docs.tardis.dev/api/tardis-machine#disconnect)                                                         | *Not applicable*                                                     |

**Notes:**

- [quote](https://docs.tardis.dev/api/tardis-machine#book_snapshot_-number_of_levels-_-snapshot_interval-time_unit) is an alias for [book_snapshot_1_0ms](https://docs.tardis.dev/api/tardis-machine#book_snapshot_-number_of_levels-_-snapshot_interval-time_unit).
- [quote_10s](https://docs.tardis.dev/api/tardis-machine#book_snapshot_-number_of_levels-_-snapshot_interval-time_unit) is an alias for [book_snapshot_1_10s](https://docs.tardis.dev/api/tardis-machine#book_snapshot_-number_of_levels-_-snapshot_interval-time_unit).
- Both quote, quote\_10s, and one-level snapshots are parsed as `QuoteTick`.

:::info
See also the Tardis [normalized market data APIs](https://docs.tardis.dev/api/tardis-machine#normalized-market-data-apis).
:::

## Bars

The adapter will automatically convert [Tardis trade bar interval and suffix](https://docs.tardis.dev/api/tardis-machine#trade_bar_-aggregation_interval-suffix) to Nautilus `BarType`s.
This includes the following:

| Tardis suffix                                                                                                | Nautilus bar aggregation    |
|:-------------------------------------------------------------------------------------------------------------|:----------------------------|
| [ms](https://docs.tardis.dev/api/tardis-machine#trade_bar_-aggregation_interval-suffix) - milliseconds       | `MILLISECOND`               |
| [s](https://docs.tardis.dev/api/tardis-machine#trade_bar_-aggregation_interval-suffix) - seconds             | `SECOND`                    |
| [m](https://docs.tardis.dev/api/tardis-machine#trade_bar_-aggregation_interval-suffix) - minutes             | `MINUTE`                    |
| [ticks](https://docs.tardis.dev/api/tardis-machine#trade_bar_-aggregation_interval-suffix) - number of ticks | `TICK`                      |
| [vol](https://docs.tardis.dev/api/tardis-machine#trade_bar_-aggregation_interval-suffix) - volume size       | `VOLUME`                    |

## Symbology and normalization

The Tardis integration ensures seamless compatibility with NautilusTrader’s crypto exchange adapters
by consistently normalizing symbols. Typically, NautilusTrader uses the native exchange naming conventions
provided by Tardis. However, for certain exchanges, raw symbols are adjusted to adhere to the Nautilus symbology normalization, as outlined below:

### Common rules

- All symbols are converted to uppercase.
- Market type suffixes are appended with a hyphen for some exchanges (see [exchange-specific normalizations](#exchange-specific-normalizations)).
- Original exchange symbols are preserved in the Nautilus instrument definitions `raw_symbol` field.

### Exchange-specific normalizations

- **Binance**: Nautilus appends the suffix `-PERP` to all perpetual symbols.
- **Bybit**: Nautilus uses specific product category suffixes, including `-SPOT`, `-LINEAR`, `-INVERSE`, `-OPTION`.
- **dYdX**: Nautilus appends the suffix `-PERP` to all perpetual symbols.
- **Gate.io**: Nautilus appends the suffix `-PERP` to all perpetual symbols.

For detailed symbology documentation per exchange:

- [Binance symbology](./binance.md#symbology)
- [Bybit symbology](./bybit.md#symbology)
- [dYdX symbology](./dydx.md#symbology)

## Venues

Some exchanges on Tardis are partitioned into multiple venues.
The table below outlines the mappings between Nautilus venues and corresponding Tardis exchanges, as well as the exchanges that Tardis supports:

| Nautilus venue          | Tardis exchange(s)                                    |
|:------------------------|:------------------------------------------------------|
| `ASCENDEX`              | `ascendex`                                            |
| `BINANCE`               | `binance`, `binance-dex`, `binance-futures`, `binance-jersey`, `binance-options`, `binance-us` |
| `BINANCE_DELIVERY`      | `binance-delivery` (*COIN-margined contracts*)        |
| `BINANCE_US`            | `binance-us`                                          |
| `BITFINEX`              | `bitfinex`, `bitfinex-derivatives`                    |
| `BITFLYER`              | `bitflyer`                                            |
| `BITMEX`                | `bitmex`                                              |
| `BITNOMIAL`             | `bitnomial`                                           |
| `BITSTAMP`              | `bitstamp`                                            |
| `BLOCKCHAIN_COM`        | `blockchain-com`                                      |
| `BYBIT`                 | `bybit`, `bybit-options`, `bybit-spot`                |
| `COINBASE`              | `coinbase`                                            |
| `COINFLEX`              | `coinflex` (*for historical research*)                |
| `CRYPTO_COM`            | `crypto-com`                                          |
| `CRYPTOFACILITIES`      | `cryptofacilities`                                    |
| `DELTA`                 | `delta`                                               |
| `DERIBIT`               | `deribit`                                             |
| `DYDX`                  | `dydx`                                                |
| `FTX`                   | `ftx` (*historical research*)                         |
| `FTX_US`                | `ftx-us` (*historical research*)                      |
| `GATE_IO`               | `gate-io`, `gate-io-futures`                          |
| `GEMINI`                | `gemini`                                              |
| `HITBTC`                | `hitbtc`                                              |
| `HUOBI`                 | `huobi`, `huobi-dm`, `huobi-dm-linear-swap`, `huobi-dm-options` |
| `HUOBI_DELIVERY`        | `huobi-dm-swap`                                       |
| `KRAKEN`                | `kraken`                                              |
| `KUCOIN`                | `kucoin`                                              |
| `MANGO`                 | `mango`                                               |
| `OKCOIN`                | `okcoin`                                              |
| `OKEX`                  | `okex`, `okex-futures`, `okex-options`, `okex-swap`   |
| `PHEMEX`                | `phemex`                                              |
| `POLONIEX`              | `poloniex`                                            |
| `SERUM`                 | `serum` (*historical research*)                       |
| `STARATLAS`             | `staratlas`                                           |
| `UPBIT`                 | `upbit`                                               |
| `WOO_X`                 | `woo-x`                                               |

## Environment variables

The following environment variables are used by Tardis and NautilusTrader.

- `TM_API_KEY`: API key for the Tardis Machine.
- `TARDIS_API_KEY`: API key for NautilusTrader Tardis clients.
- `TARDIS_WS_URL` (optional): WebSocket URL for the `TardisMachineClient` in NautilusTrader.
- `TARDIS_BASE_URL` (optional): Base URL for the `TardisHttpClient` in NautilusTrader.
- `NAUTILUS_CATALOG_PATH` (optional): Root directory for writing replay data in the Nautilus catalog.

## Running Tardis Machine historical replays

The [Tardis Machine Server](https://docs.tardis.dev/api/tardis-machine) is a locally runnable server
with built-in data caching, providing both tick-level historical and consolidated real-time cryptocurrency market data through HTTP and WebSocket APIs.

You can perform complete Tardis Machine WebSocket replays of historical data and output the results
in Nautilus Parquet format, using either Python or Rust. Since the function is implemented in Rust,
performance is consistent whether run from Python or Rust, letting you choose based on your preferred workflow.

The end-to-end `run_tardis_machine_replay` data pipeline function utilizes a specified [configuration](#configuration) to execute the following steps:

- Connect to the Tardis Machine server.
- Request and parse all necessary instrument definitions from the [Tardis instruments metadata](https://docs.tardis.dev/api/instruments-metadata-api) HTTP API.
- Stream all requested instruments and data types for the specified time ranges from the Tardis Machine server.
- For each instrument, data type and date (UTC), generate a `.parquet` file in the Nautilus format.
- Disconnect from the Tardis Marchine server, and terminate the program.

:::note
You can request data for the first day of each month without an API key. For all other dates, a Tardis Machine API key is required.
:::

This process is optimized for direct output to a Nautilus Parquet data catalog.
Ensure that the `NAUTILUS_CATALOG_PATH` environment variable is set to the root `/catalog/` directory.
Parquet files will then be organized under `/catalog/data/` in the expected subdirectories corresponding to data type and instrument.

If no `output_path` is specified in the configuration file and the `NAUTILUS_CATALOG_PATH` environment variable is unset, the system will default to the current working directory.

### Procedure

First, ensure the `tardis-machine` docker container is running. Use the following command:

```bash
docker run -p 8000:8000 -p 8001:8001 -e "TM_API_KEY=YOUR_API_KEY" -d tardisdev/tardis-machine
```

This command starts the `tardis-machine` server without a persistent local cache, which may affect performance.
For improved performance, consider running the server with a persistent volume. Refer to the [Tardis Docker documentation](https://docs.tardis.dev/api/tardis-machine#docker) for details.

### Configuration

Next, ensure you have a configuration JSON file available.

**Configuration JSON format**

| Field               | Type              | Description                                                                         | Default                                                                                               |
|:--------------------|:------------------|:------------------------------------------------------------------------------------|:------------------------------------------------------------------------------------------------------|
| `tardis_ws_url`     | string (optional) | The Tardis Machine WebSocket URL.                                                   | If `null` then will use the `TARDIS_WS_URL` env var.                                                  |
| `normalize_symbols` | bool (optional)   | If Nautilus [symbol normalization](#symbology-and-normalization) should be applied. | If `null` then will default to `true`.                                                                |
| `output_path`       | string (optional) | The output directory path to write Nautilus Parquet data to.                        | If `null` then will use the `NAUTILUS_CATALOG_PATH` env var, otherwise the current working directory. |
| `options`           | JSON[]            | An array of [ReplayNormalizedRequestOptions](https://docs.tardis.dev/api/tardis-machine#replay-normalized-options) objects.                                                                 |

An example configuration file, `example_config.json`, is available [here](https://github.com/nautechsystems/nautilus_trader/blob/develop/crates/adapters/tardis/bin/example_config.json):

```json
{
  "tardis_ws_url": "ws://localhost:8001",
  "output_path": null,
  "options": [
    {
      "exchange": "bitmex",
      "symbols": [
        "xbtusd",
        "ethusd"
      ],
      "data_types": [
        "trade"
      ],
      "from": "2019-10-01",
      "to": "2019-10-02"
    }
  ]
}
```

### Python replays

To run a replay in Python, create a script similar to the following:

```python
import asyncio

from nautilus_trader.core import nautilus_pyo3


async def run():
    config_filepath = Path("YOUR_CONFIG_FILEPATH")
    await nautilus_pyo3.run_tardis_machine_replay(str(config_filepath.resolve()))


if __name__ == "__main__":
    asyncio.run(run())
```

### Rust replays

To run a replay in Rust, create a binary similar to the following:

```rust
use std::{env, path::PathBuf};

use nautilus_adapters::tardis::replay::run_tardis_machine_replay_from_config;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    let config_filepath = PathBuf::from("YOUR_CONFIG_FILEPATH");
    run_tardis_machine_replay_from_config(&config_filepath).await;
}
```

Make sure to enable Rust logging by exporting the following environment variable:

```bash
export RUST_LOG=debug
```

A working example binary can be found [here](https://github.com/nautechsystems/nautilus_trader/blob/develop/crates/adapters/tardis/bin/example_replay.rs).

This can also be run using cargo:

```bash
cargo run --bin tardis-replay <path_to_your_config>
```

## Loading Tardis CSV data

Tardis-format CSV data can be loaded using either Python or Rust. The loader reads the CSV text data
from disk and parses it into Nautilus data. Since the loader is implemented in Rust, performance remains
consistent regardless of whether you run it from Python or Rust, allowing you to choose based on your preferred workflow.

You can also optionally specify a `limit` parameter for the `load_*` functions/methods to control the maximum number of rows loaded.

:::note
Loading mixed-instrument CSV files is challenging due to precision requirements and is not recommended. Use single-instrument CSV files instead (see below).
:::

### Loading CSV data in Python

You can load Tardis-format CSV data in Python using the `TardisCSVDataLoader`.
When loading data, you can optionally specify the instrument ID but must specify both the price precision, and size precision.
Providing the instrument ID improves loading performance, while specifying the precisions is required, as they cannot be inferred from the text data alone.

To load the data, create a script similar to the following:

```python
from nautilus_trader.adapters.tardis.loaders import TardisCSVDataLoader
from nautilus_trader.model import InstrumentId


instrument_id = InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
loader = TardisCSVDataLoader(
    price_precision=1,
    size_precision=0,
    instrument_id=instrument_id,
)

filepath = Path("YOUR_CSV_DATA_PATH")
limit = None

deltas = loader.load_deltas(filepath, limit)
```

### Loading CSV data in Rust

You can load Tardis-format CSV data in Rust using the loading functions found [here](https://github.com/nautechsystems/nautilus_trader/blob/develop/crates/adapters/tardis/src/csv/mod.rs).
When loading data, you can optionally specify the instrument ID but must specify both the price precision and size precision.
Providing the instrument ID improves loading performance, while specifying the precisions is required, as they cannot be inferred from the text data alone.

For a complete example, see the [example binary here](https://github.com/nautechsystems/nautilus_trader/blob/develop/crates/adapters/tardis/bin/example_csv.rs).

To load the data, you can use code similar to the following:

```rust
use std::path::Path;

use nautilus_adapters::tardis;
use nautilus_model::identifiers::InstrumentId;

#[tokio::main]
async fn main() {
    // You must specify precisions and the CSV filepath
    let price_precision = 1;
    let size_precision = 0;
    let filepath = Path::new("YOUR_CSV_DATA_PATH");

    // Optionally specify an instrument ID and/or limit
    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    let limit = None;

    // Consider propagating any parsing error depending on your workflow
    let _deltas = tardis::csv::load_deltas(
        filepath,
        price_precision,
        size_precision,
        Some(instrument_id),
        limit,
    )
    .unwrap();
}
```

## Requesting instrument definitions

You can request instrument definitions in both Python and Rust using the `TardisHttpClient`.
This client interacts with the [Tardis instruments metadata API](https://docs.tardis.dev/api/instruments-metadata-api) to request and parse instrument metadata into Nautilus instruments.

The `TardisHttpClient` constructor accepts optional parameters for `api_key`, `base_url`, and `timeout_secs` (default is 60 seconds).

The client provides methods to retrieve either a specific `instrument`, or all `instruments` available on a particular exchange.
Ensure that you use Tardis’s lower-kebab casing convention when referring to a [Tardis-supported exchange](https://api.tardis.dev/v1/exchanges).

:::note
A Tardis API key is required to access the instruments metadata API.
:::

### Requesting instruments in Python

To request instrument definitions in Python, create a script similar to the following:

```python
import asyncio

from nautilus_trader.core import nautilus_pyo3


async def run():
    http_client = nautilus_pyo3.TardisHttpClient()

    instrument = await http_client.instrument("bitmex", "xbtusd")
    print(f"Received: {instrument}")

    instruments = await http_client.instruments("bitmex")
    print(f"Received: {len(instruments)} instruments")


if __name__ == "__main__":
    asyncio.run(run())
```

### Requesting instruments in Rust

To request instrument definitions in Rust, use code similar to the following.
For a complete example, see the [example binary here](https://github.com/nautechsystems/nautilus_trader/blob/develop/crates/adapters/tardis/bin/example_http.rs).

```rust
use nautilus_adapters::tardis::{enums::Exchange, http::client::TardisHttpClient};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    let client = TardisHttpClient::new(None, None, None).unwrap();

    // Nautilus instrument definitions
    let resp = client.instruments(Exchange::Bitmex).await;
    println!("Received: {resp:?}");

    let resp = client.instrument(Exchange::Bitmex, "ETHUSDT").await;
    println!("Received: {resp:?}");
}
```

## Instrument provider

The `TardisInstrumentProvider` requests and parses instrument definitions from Tardis through the HTTP instrument metadata API.
Since there are multiple [Tardis-supported exchanges](#venues), when loading all instruments,
you must filter for the desired venues using an `InstrumentProviderConfig`:

```python
from nautilus_trader.config import InstrumentProviderConfig

# See supported venues https://nautilustrader.io/docs/nightly/integrations/tardis#venues
venues = {"BINANCE", "BYBIT"}
filters = {"venues": frozenset(venues)}
instrument_provider_config = InstrumentProviderConfig(load_all=True, filters=filters)
```

You can also load specific instrument definitions in the usual way:

```python
from nautilus_trader.config import InstrumentProviderConfig

instrument_ids = [
    InstrumentId.from_str("BTCUSDT-PERP.BINANCE"),  # Will use the 'binance-futures' exchange
    InstrumentId.from_str("BTCUSDT.BINANCE"),  # Will use the 'binance' exchange
]
instrument_provider_config = InstrumentProviderConfig(load_ids=instrument_ids)
```

:::note
Instruments must be available in the cache for all subscriptions.
For simplicity, it’s recommended to load all instruments for the venues you intend to subscribe to.
:::

## Live data client

The `TardisDataClient` enables integration of a Tardis Machine with a running NautilusTrader system.
It supports subscriptions to the following data types:

- `OrderBookDelta` (L2 granularity from Tardis, includes all changes or full-depth snapshots)
- `OrderBookDepth10` (L2 granularity from Tardis, provides snapshots up to 10 levels)
- `QuoteTick`
- `TradeTick`
- `Bar` (trade bars with [Tardis-supported bar aggregations](#bars))

### Data WebSockets

The main `TardisMachineClient` data WebSocket manages all stream subscriptions received during the initial connection phase,
up to the duration specified by `ws_connection_delay_secs`. For any additional subscriptions made
after this period, a new `TardisMachineClient` is created. This approach optimizes performance by
allowing the main WebSocket to handle potentially hundreds of subscriptions in a single stream if
they are provided at startup.

When an initial subscription delay is set with `ws_connection_delay_secs`, unsubscribing from any
of these streams will not actually remove the subscription from the Tardis Machine stream, as selective
unsubscription is not supported by Tardis. However, the component will still unsubscribe from message
bus publishing as expected.

All subscriptions made after any initial delay will behave normally, fully unsubscribing from the
Tardis Machine stream when requested.

:::tip
If you anticipate frequent subscription and unsubscription of data, it is recommended to set
`ws_connection_delay_secs` to zero. This will create a new client for each initial subscription,
allowing them to be later closed individually upon unsubscription.
:::

## Limitations and considerations

The following limitations and considerations are currently known:

- Historical data requests are not supported, as each would require a minimum one-day replay from the Tardis Machine, potentially with a filter. This approach is neither practical nor efficient.
