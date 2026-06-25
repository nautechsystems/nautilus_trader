# Tardis

Tardis provides granular data for cryptocurrency markets including tick-by-tick order book snapshots and
updates, trades, open interest, funding rates, option summaries, and liquidations data for leading
crypto exchanges.

NautilusTrader integrates with the Tardis API, Tardis Machine WebSocket server, and Tardis CSV
formats. The capabilities of this adapter include:

- `TardisCSVDataLoader`: reads Tardis-format CSV files into Nautilus data, with bulk and
  memory-efficient streaming paths.
- `TardisMachineClient`: streams live or historical replay data from Tardis Machine and converts
  messages into Nautilus data.
- `TardisHttpClient`: requests instrument metadata from the Tardis HTTP API and parses it into
  Nautilus instrument definitions.
- `TardisDataClient`: provides a live data client for Tardis Machine streams.
- `TardisInstrumentProvider`: loads instrument definitions from the Tardis metadata API.
- **Data pipeline functions**: replay historical data from Tardis Machine and write Nautilus
  Parquet catalog files.

:::info
A `TARDIS_API_KEY` is required for Nautilus instrument metadata calls. Tardis Machine uses
`TM_API_KEY` for historical dates outside the free first day of each month. See also
[environment variables](#environment-variables).
:::

## Overview

This adapter is implemented in Rust, with optional Python bindings.
It does not require any external Tardis client library dependencies.

:::info
There is **no** need for additional installation steps for `tardis`.
The core components of the adapter are compiled as static libraries and linked during the build.
:::

## Tardis documentation

Tardis provides extensive user [documentation](https://docs.tardis.dev/).
We recommend also referring to the Tardis documentation in conjunction with this NautilusTrader integration guide.

## Supported formats

Tardis provides *normalized* market data, a unified format consistent across supported exchanges.
This normalization lets one parser handle data from any [Tardis-supported exchange](#venues).
NautilusTrader does not support exchange-native Tardis market data formats in this adapter.

The following normalized Tardis Machine formats are supported by NautilusTrader. See the official
[Tardis data type reference](https://docs.tardis.dev/tardis-machine/data-types) for field schemas.

| Tardis format       | Nautilus data type                                                |
|:--------------------|:------------------------------------------------------------------|
| `book_change`       | `OrderBookDelta`                                                  |
| `book_snapshot_*`   | `OrderBookDepth10` or `OrderBookDeltas`                           |
| `quote`             | `QuoteTick`                                                       |
| `quote_10s`         | `QuoteTick`                                                       |
| `trade`             | `Trade`                                                           |
| `trade_bar_*`       | `Bar`                                                             |
| `instrument`        | `CurrencyPair`, `CryptoFuture`, `CryptoPerpetual`, `CryptoOption` |
| `derivative_ticker` | `FundingRateUpdate`                                               |
| `option_summary`    | `OptionGreeks`; optional `QuoteTick` from BBO fields              |
| `disconnect`        | *Not applicable*                                                  |

**Notes:**

- Tardis documents `quote` as an alias for `book_snapshot_1_0ms`.
- Tardis documents `quote_10s` as an alias for `book_snapshot_1_10s`.
- `quote`, `quote_10s`, and one-level snapshots are parsed as `QuoteTick`.
- The Rust data client also emits mark and index price updates from `derivative_ticker` messages
  when those values change.
- Tardis `option_summary` messages include best bid/offer fields. Nautilus always maps this feed to
  `OptionGreeks`; set `extract_bbo_as_quotes` to `true` to also emit `QuoteTick` from those BBO
  fields.

:::info
See also the Tardis [Tardis Machine quickstart](https://docs.tardis.dev/tardis-machine/quickstart).
:::

## Bars

The adapter converts Tardis trade bar intervals and suffixes to Nautilus `BarType`s.
This includes the following:

| Tardis suffix | Meaning         | Nautilus bar aggregation |
|:--------------|:----------------|:-------------------------|
| `ms`          | Milliseconds    | `MILLISECOND`            |
| `s`           | Seconds         | `SECOND`                 |
| `m`           | Minutes         | `MINUTE`                 |
| `ticks`       | Number of ticks | `TICK`                   |
| `vol`         | Volume size     | `VOLUME`                 |

## Symbology and normalization

The Tardis integration ensures compatibility with NautilusTrader's crypto exchange adapters
by consistently normalizing symbols. Typically, NautilusTrader uses the native exchange naming
conventions provided by Tardis. For certain exchanges, raw symbols are adjusted to adhere to
Nautilus symbology normalization, as outlined below:

### Common rules

- All symbols are converted to uppercase.
- Market type suffixes are appended with a hyphen for some exchanges.
- Original exchange symbols are preserved in the Nautilus instrument definitions `raw_symbol` field.

### Exchange-specific normalizations

- **Binance**: Nautilus appends the suffix `-PERP` to all perpetual symbols.
- **Bybit**: Nautilus uses product category suffixes, including `-SPOT`, `-LINEAR`,
  `-INVERSE`, and `-OPTION`.
- **dYdX**: Nautilus appends the suffix `-PERP` to all perpetual symbols.
- **Gate.io**: Nautilus appends the suffix `-PERP` to all perpetual symbols.

For detailed symbology documentation per exchange:

- [Binance symbology](./binance.md#symbology)
- [Bybit symbology](./bybit.md#symbology)
- [dYdX symbology](./dydx.md#symbology)

## Venues

Some exchanges on Tardis are partitioned into multiple venues.
The table below outlines the mappings between Nautilus venues and corresponding Tardis exchanges:

| Nautilus venue          | Tardis exchange(s)                                    |
|:------------------------|:------------------------------------------------------|
| `ASCENDEX`              | `ascendex`                                            |
| `BINANCE`               | `binance`, `binance-dex`, `binance-futures`, `binance-options` |
| `BINANCE_DELIVERY`      | `binance-delivery` (*COIN‑margined contracts*)        |
| `BINANCE_US`            | `binance-us`                                          |
| `BITFINEX`              | `bitfinex`, `bitfinex-derivatives`                    |
| `BITFLYER`              | `bitflyer`                                            |
| `BITGET`                | `bitget`, `bitget-futures`                            |
| `BITMEX`                | `bitmex`                                              |
| `BITNOMIAL`             | `bitnomial`                                           |
| `BITSTAMP`              | `bitstamp`                                            |
| `BLOCKCHAIN_COM`        | `blockchain-com`                                      |
| `BYBIT`                 | `bybit`, `bybit-options`, `bybit-spot`                |
| `COINBASE`              | `coinbase`                                            |
| `COINBASE_INTX`         | `coinbase-international`                              |
| `COINFLEX`              | `coinflex` (*for historical research*)                |
| `CRYPTO_COM`            | `crypto-com`                                          |
| `CRYPTOFACILITIES`      | `cryptofacilities`                                    |
| `DELTA`                 | `delta`                                               |
| `DERIBIT`               | `deribit`                                             |
| `DYDX`                  | `dydx`                                                |
| `DYDX_V4`               | `dydx-v4`                                             |
| `FTX`                   | `ftx`, `ftx-us` (*historical research*)               |
| `GATE_IO`               | `gate-io`, `gate-io-futures`                          |
| `GEMINI`                | `gemini`                                              |
| `HITBTC`                | `hitbtc`                                              |
| `HUOBI`                 | `huobi`, `huobi-dm`, `huobi-dm-linear-swap`, `huobi-dm-options` |
| `HUOBI_DELIVERY`        | `huobi-dm-swap`                                       |
| `HYPERLIQUID`           | `hyperliquid`                                         |
| `KRAKEN`                | `kraken`                                              |
| `KUCOIN`                | `kucoin`, `kucoin-futures`                            |
| `MANGO`                 | `mango`                                               |
| `OKCOIN`                | `okcoin`                                              |
| `OKEX`                  | `okex`, `okex-futures`, `okex-options`, `okex-spreads`, `okex-swap` |
| `PHEMEX`                | `phemex`                                              |
| `POLONIEX`              | `poloniex`                                            |
| `SERUM`                 | `serum` (*historical research*)                       |
| `STAR_ATLAS`            | `star-atlas`                                          |
| `UPBIT`                 | `upbit`                                               |
| `WOO_X`                 | `woo-x`                                               |

Tardis also exposes legacy Binance exchanges such as `binance-european-options` and
`binance-jersey`.

## Environment variables

The following environment variables are used by Tardis and NautilusTrader.

- `TM_API_KEY`: API key for the Tardis Machine.
- `TARDIS_API_KEY`: API key for NautilusTrader Tardis clients.
- `TARDIS_MACHINE_WS_URL` (optional): WebSocket URL for the `TardisMachineClient`.
- `TARDIS_BASE_URL` (optional): Base URL for the `TardisHttpClient` in NautilusTrader.
- `NAUTILUS_PATH` (optional): Parent directory containing the `catalog/` subdirectory for
  replay output.

The Tardis instruments metadata API requires bearer-token authorization and is available to active
pro and business Tardis subscriptions.

## Running Tardis Machine historical replays

The [Tardis Machine Server](https://docs.tardis.dev/tardis-machine/quickstart) is a locally
runnable server with built-in data caching. It provides tick-level historical and consolidated
real-time cryptocurrency market data through HTTP and WebSocket APIs.

You can perform complete Tardis Machine WebSocket replays of historical data and output the results
in Nautilus Parquet format, using either Python or Rust. Since the function is implemented in Rust,
performance is consistent whether run from Python or Rust.

The end-to-end `run_tardis_machine_replay` data pipeline function uses a specified
[configuration](#configuration) to execute the following steps:

- Connect to the Tardis Machine server.
- Request and parse all necessary instrument definitions from the Tardis instruments metadata API.
- Stream all requested instruments and data types for the specified time ranges from Tardis Machine.
- For each instrument, data type and date (UTC), generate a catalog-compatible `.parquet` file.
- Disconnect from the Tardis Machine server, and terminate the program.

**File naming convention**

Files are written one per day, per instrument, using ISO 8601 timestamp ranges:

- **Format**: `{start_timestamp}_{end_timestamp}.parquet`
- **Example**: `2023-10-01T00-00-00-000000000Z_2023-10-01T23-59-59-999999999Z.parquet`
- **Structure**: `data/{data_type}/{instrument_id}/{filename}`

This format is compatible with Nautilus data catalog queries, consolidation, and management.

:::note
You can request data for the first day of each month without a Tardis Machine API key. Other
dates require `TM_API_KEY`.
:::

This process is optimized for direct output to a Nautilus Parquet data catalog.
Set `NAUTILUS_PATH` to the parent directory that contains the `catalog/` subdirectory. Parquet
files are written under `<NAUTILUS_PATH>/catalog/data/` in subdirectories by data type and
instrument.

If no `output_path` is specified and `NAUTILUS_PATH` is unset, output defaults to the current
working directory.

### Procedure

First, ensure the `tardis-machine` docker container is running. Use the following command:

```bash
docker run -p 8000:8000 -p 8001:8001 -e "TM_API_KEY=YOUR_API_KEY" -d tardisdev/tardis-machine
```

This command starts the `tardis-machine` server without a persistent local cache, which may affect
performance. For better replay performance, run it with a persistent volume.

### Configuration

Next, ensure you have a configuration JSON file available.

**Configuration JSON fields**

- `tardis_ws_url` (`str | null`): Tardis Machine WebSocket URL. Defaults to
  `TARDIS_MACHINE_WS_URL`.
- `normalize_symbols` (`bool | null`): applies Nautilus symbol normalization. Defaults to `true`.
- `output_path` (`str | null`): output directory for Parquet data. Defaults to `NAUTILUS_PATH`,
  then the current working directory.
- `book_snapshot_output` (`"deltas" | "depth10" | null`): output format for snapshots. Defaults
  to `"deltas"`.
- `extract_bbo_as_quotes` (`bool | null`): also writes `QuoteTick` data from best bid/offer fields
  in Tardis Machine `option_summary` messages. Defaults to `false`.
- `compression` (`"zstd" | "snappy" | "uncompressed" | null`): Parquet compression codec.
  Defaults to `"zstd"` level 3.
- `proxy_url` (`str | null`): proxy URL for Tardis HTTP requests. Defaults to no proxy.
- `options` (`JSON[]`): required replay request option objects.

An example configuration file is available at `crates/adapters/tardis/bin/example_config.json`:

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

### Book snapshot output

The `book_snapshot_output` configuration option controls how Tardis `book_snapshot_*` messages are
converted and stored.

| Value     | Nautilus type      | Output directory     | Description                           |
|:----------|:-------------------|:---------------------|:--------------------------------------|
| `deltas`  | `OrderBookDeltas`  | `order_book_deltas/` | Price level updates.                  |
| `depth10` | `OrderBookDepth10` | `order_book_depths/` | Snapshots with up to 10 price levels. |

**When to use each format:**

- **`deltas` (default)**: use when you need to reconstruct book state or combine snapshots with
  `book_change` data. Each price level becomes a separate delta record.
- **`depth10`**: use when a strategy needs periodic depth snapshots. Each snapshot is a single
  record, and snapshots with more than 10 levels keep only the first 10.

**Avoiding file overwrites:**

When downloading both `book_snapshot_*` and `book_change` data for the same instrument and date
range, `depth10` writes snapshots to `order_book_depths/` and avoids overwriting
`order_book_deltas/`.

Example configuration with explicit format:

```json
{
  "tardis_ws_url": "ws://localhost:8001",
  "book_snapshot_output": "depth10",
  "options": [
    {
      "exchange": "binance-futures",
      "symbols": ["btcusdt"],
      "data_types": ["book_snapshot_5_100ms", "book_change"],
      "from": "2024-01-01",
      "to": "2024-01-02"
    }
  ]
}
```

### Option summary BBO extraction

Set `extract_bbo_as_quotes` to `true` when requesting Tardis Machine `option_summary` data and the
backtest also needs option BBO quotes. Nautilus still writes `OptionGreeks` from every
`option_summary` message. When all best bid/offer fields are present and sizes are valid, it also
writes a `QuoteTick` for the same instrument and timestamps.

This option only applies to Tardis Machine `option_summary` replay and stream messages. It does not
change Tardis CSV loading.

```json
{
  "tardis_ws_url": "ws://localhost:8001",
  "extract_bbo_as_quotes": true,
  "options": [
    {
      "exchange": "deribit",
      "symbols": ["BTC-28JUN24-70000-C"],
      "data_types": ["option_summary"],
      "from": "2024-01-01",
      "to": "2024-01-02"
    }
  ]
}
```

### Python replays

To run a replay in Python, create a script similar to the following:

```python
import asyncio
from pathlib import Path

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
use std::path::PathBuf;

use nautilus_adapters::tardis::replay::run_tardis_machine_replay_from_config;

#[tokio::main]
async fn main() {
    nautilus_common::logging::ensure_logging_initialized();

    let config_filepath = PathBuf::from("YOUR_CONFIG_FILEPATH");
    run_tardis_machine_replay_from_config(&config_filepath).await;
}
```

Logging defaults to INFO level. To enable debug logging, export the following environment variable:

```bash
export NAUTILUS_LOG=debug
```

A working example binary is available at `crates/adapters/tardis/bin/example_replay.rs`.

This can also be run using cargo:

```bash
cargo run --bin tardis-replay <path_to_your_config>
```

### Option-chain backtest catalog

An option-chain backtest starts after the Tardis replay has written data to the Nautilus
catalog. The backtest loader does not request missing Tardis data during a run, so the
catalog must contain:

- Option instruments from the Tardis instrument metadata API.
- `QuoteTick` data from one-level option book snapshots, quote data, or `option_summary` BBO
  extraction.
- `OptionGreeks` data from Tardis `option_summary` messages.

Use both `QuoteTick` and `OptionGreeks` in the `BacktestDataConfig` list for the same
option instrument IDs. The option-chain manager aggregates the replayed BBO and Greeks
into `OptionChainSlice` snapshots. Use `snapshot_interval_ms=None` for raw publishing,
or set an interval in milliseconds to publish thinned snapshots.

Strategies can select contracts by moneyness with ATM-relative or ATM-percent strike
ranges, by delta with `StrikeRange.delta(target, tolerance)`, or by fixed strike with
`StrikeRange.fixed([...])`. Option order matching in backtests is quote-driven:
marketable orders fill as takers against the opposing BBO, while passive limits can
fill as makers when later BBO updates trade through the limit.

Configure option fees explicitly on the simulated venue with structural fee models such
as `CappedOptionFeeModel` or `TieredNotionalOptionFeeModel`. There is no automatic
Tardis exchange to fee model mapping.

### Option-chain CSV catalog conversion

For historical option chains from downloadable Tardis CSV files, use
`TardisCSVDataLoader.convert_options_chain_csv(...)` to convert `options_chain` rows into
Nautilus catalog data. This path does not call Tardis Machine or the instrument metadata API, so
it is useful when you already have Tardis CSV files or want a no-API-key catalog bootstrap from
downloaded data.

The converter writes `OptionGreeks` for every selected row. With the default
`extract_bbo_as_quotes=True`, complete best bid/offer rows also write `QuoteTick`. Keep this
enabled for option-chain backtests: greeks-only catalogs do not provide quotes, so the chain
manager cannot publish populated `OptionChainSlice` snapshots for strikes without BBO data.

Instrument derivation currently supports Deribit options. For other option venues, set
`write_instruments=False` before conversion and load the instruments through another source
before backtesting. Leaving it enabled for a non-Deribit file can fail after data files have
been written to the catalog. Pass daily `options_chain` CSV paths in chronological order. The
`underlyings` filter matches symbol prefixes such as `["BTC-"]`. Set `snapshot_interval_ms` to
keep the last row per instrument per interval within each input file, or use `None` to write
every selected row. Rows must be ordered by `local_timestamp` within each file when thinning.

Provide explicit `price_precision` and `size_precision` on the loader for deterministic quote
metadata. Inferred precision can increase as later rows are read, so data written earlier in a
file can keep lower precision metadata.

```python
from pathlib import Path

from nautilus_trader.adapters.tardis import TardisCSVDataLoader


loader = TardisCSVDataLoader(
    price_precision=4,
    size_precision=1,
)
loader.convert_options_chain_csv(
    filepaths=[Path("deribit_options_chain_2020-06-08.csv")],
    catalog_path=Path("catalog"),
    underlyings=["BTC-"],
    snapshot_interval_ms=60_000,
)
```

## Loading Tardis CSV data

Tardis-format CSV data can be loaded using either Python or Rust. The loader reads the CSV text data
from disk and parses it into Nautilus data. Since the loader is implemented in Rust, performance
remains consistent regardless of whether you run it from Python or Rust.

You can also specify a `limit` parameter for the `load_*` functions and methods to control the
maximum number of rows loaded.

:::note
Loading mixed-instrument CSV files is challenging due to precision requirements and is not
recommended. Use single-instrument CSV files instead.

The `load_options_chain`, `stream_options_chain`, and `convert_options_chain_csv` methods are
the exception: Tardis `options_chain` files are mixed-instrument chain files, and these paths
track precision per instrument. Explicit precisions are still recommended for deterministic
output.
:::

### Loading CSV data in Python

You can load Tardis-format CSV data in Python using the `TardisCSVDataLoader`.
When loading data, you can optionally specify the instrument ID, price precision, and size
precision. Providing the instrument ID improves loading performance. Price and size precision are
inferred from the CSV when omitted, but explicit values are recommended for deterministic output,
especially with large files.

To load the data, create a script similar to the following:

```python
from pathlib import Path

from nautilus_trader.adapters.tardis import TardisCSVDataLoader
from nautilus_trader.model import InstrumentId


instrument_id = InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
loader = TardisCSVDataLoader(
    price_precision=1,
    size_precision=0,
    instrument_id=instrument_id,
)

filepath = Path("YOUR_CSV_DATA_PATH")
limit = None

deltas = loader.load_deltas(filepath, limit=limit)
```

### Loading CSV data in Rust

You can load Tardis-format CSV data in Rust using the loading functions in
`crates/adapters/tardis/src/csv/mod.rs`. When loading data, you can optionally specify the
instrument ID, price precision, and size precision. Providing the instrument ID improves loading
performance. Price and size precision are inferred from the CSV when omitted, but explicit values
are recommended for deterministic output.

For a complete example, see `crates/adapters/tardis/bin/example_csv.rs`.

To load the data, you can use code similar to the following:

```rust
use std::path::Path;

use nautilus_adapters::tardis;
use nautilus_model::identifiers::InstrumentId;

#[tokio::main]
async fn main() {
    // Optionally specify precisions and the CSV filepath
    let price_precision = Some(1);
    let size_precision = Some(0);
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

## Streaming Tardis CSV data

For memory-efficient processing of large CSV files, the Tardis integration can load and process
data in configurable chunks rather than loading entire files into memory at once. This is useful for
processing multi-gigabyte CSV files without exhausting system memory.

The Python streaming functionality is available for the high-volume CSV types:

- Order book deltas (`stream_deltas`).
- Quote ticks (`stream_quotes`).
- Trade ticks (`stream_trades`).
- Order book depth snapshots (`stream_depth10`).
- Options chain rows (`stream_options_chain`).

Rust also exposes streaming functions for these CSV types, plus batched deltas and funding rates.

### Streaming CSV data in Python

The `TardisCSVDataLoader` provides streaming methods that yield chunks of data as iterators. Each
method accepts a `chunk_size` parameter that controls how many records are read per chunk:

```python
from nautilus_trader.adapters.tardis import TardisCSVDataLoader
from nautilus_trader.model import InstrumentId

instrument_id = InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
loader = TardisCSVDataLoader(
    price_precision=1,
    size_precision=0,
    instrument_id=instrument_id,
)

filepath = Path("large_trades_file.csv")
chunk_size = 100_000  # Process 100,000 records per chunk (default)

# Stream trade ticks in chunks
for chunk in loader.stream_trades(filepath, chunk_size):
    print(f"Processing chunk with {len(chunk)} trades")
    # Process each chunk - only this chunk is in memory
    for trade in chunk:
        # Your processing logic here
        pass
```

### Streaming order book data

For order book data, streaming is available for both deltas and depth snapshots:

```python
# Stream order book deltas
for chunk in loader.stream_deltas(filepath):
    print(f"Processing {len(chunk)} deltas")
    # Process delta chunk

# Stream depth10 snapshots (specify levels: 5 or 25)
for chunk in loader.stream_depth10(filepath, levels=5):
    print(f"Processing {len(chunk)} depth snapshots")
    # Process depth chunk
```

### Streaming quote data

Quote data can be streamed similarly:

```python
# Stream quote ticks
for chunk in loader.stream_quotes(filepath):
    print(f"Processing {len(chunk)} quotes")
    # Process quote chunk
```

### Memory efficiency benefits

The streaming approach provides significant memory efficiency advantages:

- **Controlled Memory Usage**: Only one chunk is loaded in memory at a time.
- **Scalable Processing**: Can process files larger than available RAM.
- **Configurable Chunk Sizes**: Tune `chunk_size` based on your system's memory and performance
  requirements (default 100,000).

:::warning
When using streaming with precision inference, the inferred precision may differ from bulk loading
the entire file. Precision inference works within chunk boundaries, and different chunks may contain
values with different precision requirements. For deterministic precision behavior, provide
explicit `price_precision` and `size_precision` parameters.
:::

### Streaming CSV data in Rust

The underlying streaming functionality is implemented in Rust and can be used directly:

```rust
use std::path::Path;

use nautilus_adapters::tardis::csv::stream_trades;
use nautilus_model::identifiers::InstrumentId;

#[tokio::main]
async fn main() {
    let filepath = Path::new("large_trades_file.csv");
    let chunk_size = 100_000;
    let price_precision = Some(1);
    let size_precision = Some(0);
    let instrument_id = Some(InstrumentId::from("BTC-PERPETUAL.DERIBIT"));

    // Stream trades in chunks
    let stream = stream_trades(
        filepath,
        chunk_size,
        price_precision,
        size_precision,
        instrument_id,
    ).unwrap();

    for chunk_result in stream {
        match chunk_result {
            Ok(chunk) => {
                println!("Processing chunk with {} trades", chunk.len());
                // Process chunk
            }
            Err(e) => {
                eprintln!("Error processing chunk: {}", e);
                break;
            }
        }
    }
}
```

## Requesting instrument definitions

You can request instrument definitions in both Python and Rust using the `TardisHttpClient`.
This client interacts with the
[Tardis instruments metadata API](https://docs.tardis.dev/api/instruments-metadata-api) to request
and parse instrument metadata into Nautilus instruments.

The `TardisHttpClient` constructor accepts optional parameters for `api_key`, `base_url`,
`timeout_secs`, `normalize_symbols`, and `proxy_url`.

The client provides methods to retrieve either a specific `instrument`, or all `instruments`
available on a particular exchange. Use Tardis lower-kebab exchange IDs such as `binance-futures`.

:::note
A `TARDIS_API_KEY` with access to the instruments metadata API is required.
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
For a complete example, see `crates/adapters/tardis/bin/example_http.rs`.

```rust
use nautilus_tardis::{
    enums::TardisExchange,
    http::client::TardisHttpClient,
};

#[tokio::main]
async fn main() {
    nautilus_common::logging::ensure_logging_initialized();

    let client = TardisHttpClient::new(None, None, None, true, None).unwrap();

    // Tardis instrument definitions
    let resp = client
        .instruments_info(TardisExchange::Bitmex, Some("XBTUSD"), None)
        .await;
    println!("Received: {resp:?}");

    // Nautilus instrument definitions
    let resp = client
        .instruments(
            TardisExchange::Bitmex,
            Some("XBTUSD"),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;
    println!("Received: {resp:?}");
}
```

## Instrument provider

The `TardisInstrumentProvider` requests and parses instrument definitions from Tardis through the
HTTP instrument metadata API.
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
    InstrumentId.from_str("BTCUSDT-PERP.BINANCE"),  # Uses the 'binance-futures' exchange
    InstrumentId.from_str("BTCUSDT.BINANCE"),  # Uses the 'binance' exchange
]
instrument_provider_config = InstrumentProviderConfig(load_ids=instrument_ids)
```

### Option exchange filtering

The instrument provider filters out option-specific exchanges, such as `binance-options`,
`binance-european-options`, `bybit-options`, `okex-options`, and `huobi-dm-options`, when the
`instrument_type` filter is not provided or does not include `"option"`.

To explicitly load option instruments, include `"option"` in the `instrument_type` filter:

```python
from nautilus_trader.config import InstrumentProviderConfig

venues = {"BINANCE", "BYBIT"}
filters = {
    "venues": frozenset(venues),
    "instrument_type": {"option"},  # Explicitly request options
}
instrument_provider_config = InstrumentProviderConfig(load_all=True, filters=filters)
```

This filtering prevents unnecessary API calls to option exchanges when they are not needed.

:::note
Instruments must be available in the cache for all subscriptions.
For simplicity, it's recommended to load all instruments for the venues you intend to subscribe to.
:::

## Live data client

The `TardisDataClient` integrates Tardis Machine with a running NautilusTrader system.
The Python live data client translates standard subscriptions into Tardis Machine streams for:

- `OrderBookDelta` (L2 granularity from Tardis, including changes or full-depth snapshots)
- `QuoteTick`
- `TradeTick`
- `Bar` (trade bars with [Tardis-supported bar aggregations](#bars))
- `FundingRateUpdate` (from derivative_ticker messages)

Configured Tardis Machine replay/stream options can also emit `OrderBookDepth10` when
`book_snapshot_output` is `depth10`. `OptionGreeks` from `option_summary` is supported by the
Tardis Machine replay path and catalog writer. Set `extract_bbo_as_quotes` to also emit
`QuoteTick` from the best bid/offer fields in those `option_summary` messages.

### Data WebSockets

The main `TardisMachineClient` data WebSocket manages all stream subscriptions received during the
initial connection phase, up to the duration specified by `ws_connection_delay_secs`. For any
additional subscriptions made after this period, a new `TardisMachineClient` is created. This lets
the main WebSocket handle many startup subscriptions in a single stream.

When an initial subscription delay is set with `ws_connection_delay_secs`, unsubscribing from any of
these streams does not remove the subscription from the Tardis Machine stream because Tardis does
not support selective unsubscription. The component still unsubscribes from message bus publishing.

All subscriptions made after any initial delay behave normally, fully unsubscribing from the
Tardis Machine stream when requested.

:::tip
If you anticipate frequent subscription and unsubscription of data, set `ws_connection_delay_secs`
to zero. This creates a new client for each initial subscription, allowing each to close
individually on unsubscription.
:::

## Trade ID derivation

Trade ticks use the venue-provided trade ID from the Tardis message or CSV row
as the `TradeId`. When the venue omits the trade ID (empty string or null on
some exchanges), both the WebSocket parser and CSV parser fall back to a
deterministic FNV-1a hash of the symbol, timestamp, price, amount, and side.
The same venue event yields the same trade ID across replays, keeping
downstream dedup intact.

## Limitations and considerations

The following limitations and considerations are currently known:

- Historical quote and trade requests are not supported by `TardisDataClient`. Historical external
  `Bar` requests use Tardis Machine replay and require date-based replay windows. For catalog
  workflows, prefer `run_tardis_machine_replay`.

## Contributing

:::info
For additional features or to contribute to the Tardis adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
