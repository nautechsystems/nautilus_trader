# Tardis

Tardis provides granular cryptocurrency market data, including tick-by-tick order book snapshots and
updates, trades, open interest, funding rates, option summaries, and liquidations.

NautilusTrader integrates with the Tardis API, Tardis Machine WebSocket server, and Tardis CSV
formats. The capabilities of this adapter include:

- CSV loading and streaming functions read Tardis-format files into Nautilus data in bulk or
  bounded chunks.
- `run_tardis_machine_replay` replays historical data and writes Nautilus Parquet catalog files.
- `TardisDataClientConfig` and `TardisDataClientFactory` connect a Nautilus node to a configured
  historical replay or real-time Tardis Machine stream.
- Python and Rust expose `TardisMachineClient` and `TardisHttpClient` for lower-level access to
  normalized streams and instrument metadata.

:::info
A `TARDIS_API_KEY` is required for Nautilus instrument metadata calls. Tardis Machine uses
`TM_API_KEY` for historical dates outside the free first day of each month. See also
[environment variables](#environment-variables).
:::

## Overview

The adapter is implemented in Rust with optional Python bindings. Its components are compiled into
NautilusTrader, so it does not require a separate Tardis client library installation. Consult the
[Tardis documentation](https://docs.tardis.dev/) for the upstream APIs, formats, and server.

## Supported formats

Tardis provides *normalized* market data, a unified format consistent across supported exchanges.
This normalization lets one parser handle data from any [Tardis-supported exchange](#venues).
NautilusTrader does not support exchange-native Tardis market data formats in this adapter.

The following normalized Tardis Machine formats are supported by NautilusTrader. See the official
[Tardis data type reference](https://docs.tardis.dev/tardis-machine/data-types) for field schemas.

| Tardis format       | Nautilus data type                                            |
| :------------------ | :------------------------------------------------------------ |
| `book_change`       | `OrderBookDeltas`                                             |
| `book_snapshot_*`   | `OrderBookDepth10` or `OrderBookDeltas`                       |
| `quote`             | `QuoteTick`                                                   |
| `quote_10s`         | `QuoteTick`                                                   |
| `trade`             | `TradeTick`                                                   |
| `trade_bar_*`       | `Bar`                                                         |
| `derivative_ticker` | `FundingRateUpdate`, `MarkPriceUpdate`, or `IndexPriceUpdate` |
| `option_summary`    | `OptionGreeks`; optional `QuoteTick` from BBO fields          |
| `disconnect`        | Ignored                                                       |

**Notes:**

- Tardis documents `quote` as an alias for `book_snapshot_1_0ms`.
- Tardis documents `quote_10s` as an alias for `book_snapshot_1_10s`.
- `quote`, `quote_10s`, and one-level snapshots are parsed as `QuoteTick`.
- The data client emits funding rate, mark price, and index price updates from `derivative_ticker`
  messages only when their values change. The catalog replay pipeline does not write these updates.
- Tardis `option_summary` messages include best bid/offer fields. Nautilus always maps this feed to
  `OptionGreeks`; set `extract_bbo_as_quotes` to `true` to also emit `QuoteTick` from those BBO
  fields.
- The adapter does not parse the Tardis `book_ticker`, `liquidation`, or `error` normalized formats.

:::info
See also the Tardis [Tardis Machine quickstart](https://docs.tardis.dev/tardis-machine/quickstart).
:::

## Bars

The adapter converts Tardis trade bar intervals and suffixes to Nautilus `BarType`s.
This includes the following:

| Tardis suffix | Meaning         | Nautilus bar aggregation   |
| :------------ | :-------------- | :------------------------- |
| `ms`          | Milliseconds    | `MILLISECOND`              |
| `s`           | Seconds         | `SECOND`                   |
| `m`           | Minutes         | `MINUTE`, `HOUR`, or `DAY` |
| `ticks`       | Number of ticks | `TICK`                     |
| `vol`         | Volume size     | `VOLUME`                   |

Minute intervals that divide evenly into hours or days use the canonical Nautilus `HOUR` or `DAY`
aggregation.

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

- **Binance**: Nautilus appends the suffix `-PERP` to perpetual symbols from `binance`,
  `binance-futures`, `binance-us`, `binance-dex`, and `binance-jersey`.
- **Bybit**: Nautilus uses product category suffixes, including `-SPOT`, `-LINEAR`,
  `-INVERSE`, and `-OPTION`.
- **dYdX v3**: Nautilus appends the suffix `-PERP` to perpetual symbols from `dydx`.
- **Gate.io**: Nautilus appends the suffix `-PERP` to perpetual symbols from `gate-io-futures`.
- **MEXC**: Nautilus appends the suffix `-PERP` to perpetual symbols from `mexc-futures`.

For detailed symbology documentation per exchange:

- [Binance symbology](./binance.md#symbology)
- [Bybit symbology](./bybit.md#symbology)
- [dYdX symbology](./dydx.md#symbology)

## Venues

Some exchanges on Tardis are partitioned into multiple venues.
The table below outlines the mappings between Nautilus venues and corresponding Tardis exchanges:

| Nautilus venue     | Tardis exchange(s)                                                                                           |
| :----------------- | :----------------------------------------------------------------------------------------------------------- |
| `ASCENDEX`         | `ascendex`                                                                                                   |
| `BINANCE`          | `binance`, `binance-dex`, `binance-european-options`, `binance-futures`, `binance-jersey`, `binance-options` |
| `BINANCE_DELIVERY` | `binance-delivery` (*COIN-margined contracts*)                                                               |
| `BINANCE_US`       | `binance-us`                                                                                                 |
| `BITFINEX`         | `bitfinex`, `bitfinex-derivatives`                                                                           |
| `BITFLYER`         | `bitflyer`                                                                                                   |
| `BITGET`           | `bitget`, `bitget-futures`                                                                                   |
| `BITMEX`           | `bitmex`                                                                                                     |
| `BITNOMIAL`        | `bitnomial`                                                                                                  |
| `BITSTAMP`         | `bitstamp`                                                                                                   |
| `BLOCKCHAIN_COM`   | `blockchain-com`                                                                                             |
| `BYBIT`            | `bybit`, `bybit-options`, `bybit-spot`                                                                       |
| `COINBASE`         | `coinbase`                                                                                                   |
| `COINBASE_INTX`    | `coinbase-international`                                                                                     |
| `COINFLEX`         | `coinflex` (*historical data only*)                                                                          |
| `CRYPTO_COM`       | `crypto-com`                                                                                                 |
| `CRYPTOFACILITIES` | `cryptofacilities`                                                                                           |
| `DELTA`            | `delta`                                                                                                      |
| `DERIBIT`          | `deribit`                                                                                                    |
| `DYDX`             | `dydx`                                                                                                       |
| `DYDX_V4`          | `dydx-v4`                                                                                                    |
| `FTX`              | `ftx`, `ftx-us` (*historical data only*)                                                                     |
| `GATE_IO`          | `gate-io`, `gate-io-futures`                                                                                 |
| `GEMINI`           | `gemini`                                                                                                     |
| `HITBTC`           | `hitbtc`                                                                                                     |
| `HUOBI`            | `huobi`, `huobi-dm`, `huobi-dm-linear-swap`, `huobi-dm-options`                                              |
| `HUOBI_DELIVERY`   | `huobi-dm-swap`                                                                                              |
| `HYPERLIQUID`      | `hyperliquid`                                                                                                |
| `KRAKEN`           | `kraken`                                                                                                     |
| `KUCOIN`           | `kucoin`, `kucoin-futures`                                                                                   |
| `LIGHTER`          | `lighter`                                                                                                    |
| `MANGO`            | `mango`                                                                                                      |
| `MEXC`             | `mexc`, `mexc-futures`                                                                                       |
| `OKCOIN`           | `okcoin`                                                                                                     |
| `OKEX`             | `okex`, `okex-futures`, `okex-options`, `okex-spreads`, `okex-swap`                                          |
| `PHEMEX`           | `phemex`                                                                                                     |
| `POLONIEX`         | `poloniex`                                                                                                   |
| `SERUM`            | `serum` (*historical data only*)                                                                             |
| `STAR_ATLAS`       | `star-atlas`                                                                                                 |
| `UPBIT`            | `upbit`                                                                                                      |
| `WOO_X`            | `woo-x`                                                                                                      |

Some exchange IDs represent delisted venues retained for historical data. Consult the official
[historical data details](https://docs.tardis.dev/historical-data-details) for availability and
delisting status.

## Environment variables

The following environment variables are used by Tardis and NautilusTrader.

- `TM_API_KEY`: API key passed to the Tardis Machine process for historical data access.
- `TARDIS_API_KEY`: API key for Nautilus instrument metadata requests.
- `TARDIS_MACHINE_WS_URL` (optional): Tardis Machine WebSocket base URL.
- `NAUTILUS_PATH` (optional): Parent directory containing the `catalog/` subdirectory for
  replay output.

The Tardis instruments metadata API requires bearer-token authorization and is available to active
pro and business Tardis subscriptions.

## Running Tardis Machine historical replays

The [Tardis Machine Server](https://docs.tardis.dev/tardis-machine/quickstart) is a locally
runnable server with built-in data caching. It provides tick-level historical and consolidated
real-time cryptocurrency market data through HTTP and WebSocket APIs.

You can run complete Tardis Machine WebSocket replays from Python or Rust and write the results in
Nautilus Parquet format. Both interfaces call the same Rust replay implementation.

The end-to-end `run_tardis_machine_replay` data pipeline function uses a specified
[configuration](#configuration) to execute the following steps:

- Connect to the Tardis Machine server.
- Request and parse all instrument definitions for the configured exchanges from the Tardis
  instruments metadata API.
- Stream all requested instruments and data types for the specified time ranges from Tardis Machine.
- For each data type and date (UTC), write catalog-compatible `.parquet` files by instrument or
  bar type.
- Finish the stream and flush the remaining data to disk.

### Output files

Files are written one per UTC day and instrument, or per bar type, using ISO 8601 timestamp ranges:

- **Format**: `{start_timestamp}_{end_timestamp}.parquet`
- **Example**: `2023-10-01T00-00-00-000000000Z_2023-10-01T23-59-59-999999999Z.parquet`
- **Relative path**: `{data_type}/{instrument_id}/{filename}`, or `bars/{bar_type}/{filename}` for
  bars.

This format is compatible with Nautilus data catalog queries, consolidation, and management.

:::note
You can request data for the first day of each month without a Tardis Machine API key. Other
dates require `TM_API_KEY`.
:::

This process is optimized for direct output to a Nautilus Parquet data catalog.
Set `NAUTILUS_PATH` to the parent directory that contains the `catalog/` subdirectory. Parquet
files are written under `<NAUTILUS_PATH>/catalog/data/` in subdirectories by data type and
instrument or bar type.

If no `output_path` is specified and `NAUTILUS_PATH` is unset, output defaults to the current
working directory.

### Procedure

:::warning
Do not publish Tardis Machine ports on the host address `0.0.0.0`. Docker
[publishes ports on all host interfaces by default](https://docs.docker.com/engine/network/port-publishing/)
when a mapping omits the host address. On Linux, Docker
[diverts published container traffic before `ufw` applies its rules](https://docs.docker.com/engine/network/packet-filtering-firewalls/#docker-and-ufw),
which can bypass the expected firewall restrictions. Bind both ports to `127.0.0.1` unless you
require and separately secure remote access.
:::

For dates outside the free first day of each month, set `TM_API_KEY` in the host environment. Then
start the `tardis-machine` Docker container:

```bash
docker run \
  -p 127.0.0.1:8000:8000 \
  -p 127.0.0.1:8001:8001 \
  -e TM_API_KEY \
  -d tardisdev/tardis-machine
```

This command starts the `tardis-machine` server without a persistent local cache, which may affect
performance. For better replay performance, run it with a persistent volume.

### Configuration

Next, ensure you have a configuration JSON file available.

**Configuration JSON fields**

- `tardis_ws_url` (`str | null`): Tardis Machine WebSocket URL. Defaults to
  `TARDIS_MACHINE_WS_URL`.
- `normalize_symbols` (`bool | null`): applies Nautilus symbol normalization. Defaults to `true`.
- `output_path` (`str | null`): output directory for Parquet data. When unset, uses
  `<NAUTILUS_PATH>/catalog/data` if `NAUTILUS_PATH` is set, then the current working directory.
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

| Value     | Nautilus type      | Output directory     | Description                             |
| :-------- | :----------------- | :------------------- | :-------------------------------------- |
| `deltas`  | `OrderBookDeltas`  | `order_book_deltas/` | Clear and add deltas for each snapshot. |
| `depth10` | `OrderBookDepth10` | `order_book_depths/` | Snapshots with up to 10 price levels.   |

**When to use each format:**

- **`deltas` (default)**: use when you need to reconstruct book state or combine snapshots with
  `book_change` data. Each snapshot becomes a clear delta followed by an add delta for each level.
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

from nautilus_trader.adapters.tardis import run_tardis_machine_replay


async def run():
    config_filepath = Path("YOUR_CONFIG_FILEPATH")
    await run_tardis_machine_replay(str(config_filepath.resolve()))


if __name__ == "__main__":
    asyncio.run(run())
```

### Rust replays

To run a replay in Rust, create a binary similar to the following:

```rust
use std::path::PathBuf;

use nautilus_tardis::replay::run_tardis_machine_replay_from_config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let config_filepath = PathBuf::from("YOUR_CONFIG_FILEPATH");
    run_tardis_machine_replay_from_config(&config_filepath).await?;
    Ok(())
}
```

Logging defaults to INFO level. To enable debug logging, export the following environment variable:

```bash
export NAUTILUS_LOG=debug
```

A working example binary is available at `crates/adapters/tardis/bin/example_replay.rs`.

This can also be run using cargo:

```bash
cargo run -p nautilus-tardis --bin tardis-replay <path_to_your_config>
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
`convert_tardis_options_chain_csv(...)` to convert `options_chain` rows into
Nautilus catalog data. This path does not call Tardis Machine or the instrument metadata API, so
it is useful when you already have Tardis CSV files or want a no-API-key catalog bootstrap from
downloaded data.

The converter writes `OptionGreeks` for every selected row. With the default
`extract_bbo_as_quotes=True`, complete best bid/offer rows also write `QuoteTick`. Keep this
enabled for option-chain backtests: greeks-only catalogs do not provide quotes, so the chain
manager cannot publish populated `OptionChainSlice` snapshots for strikes without BBO data.

Instrument derivation supports only Deribit options. For other option venues, set
`write_instruments=False` before conversion and load the instruments through another source
before backtesting. Leaving it enabled for a non-Deribit file can fail after data files have
been written to the catalog. Pass daily `options_chain` CSV paths in chronological order. The
`underlyings` filter matches symbol prefixes such as `["BTC-"]`. Set `snapshot_interval_ms` to
keep the last row per instrument per interval within each input file, or use `None` to write
every selected row. Rows must be ordered by `local_timestamp` within each file when thinning.

Provide explicit `price_precision` and `size_precision` for deterministic quote
metadata. Inferred precision can increase as later rows are read, so data written earlier in a
file can keep lower precision metadata.

```python
from pathlib import Path

from nautilus_trader.adapters.tardis import convert_tardis_options_chain_csv


convert_tardis_options_chain_csv(
    filepaths=[Path("deribit_options_chain_2020-06-08.csv")],
    catalog_path=Path("catalog"),
    underlyings=["BTC-"],
    snapshot_interval_ms=60_000,
    price_precision=4,
    size_precision=1,
)
```

## Loading Tardis CSV data

Tardis-format CSV data can be loaded using either Python or Rust. The loader reads the CSV text data
from disk and parses it into Nautilus data. Both interfaces call the same Rust loader.

You can also specify a `limit` parameter for the `load_*` functions to control the maximum number
of rows loaded.

:::note
Loading mixed-instrument CSV files is challenging due to precision requirements and is not
recommended. Use single-instrument CSV files instead.

The `load_tardis_options_chain`, `stream_tardis_options_chain`, and
`convert_tardis_options_chain_csv` functions are the exception: Tardis `options_chain` files are
mixed-instrument chain files, and these paths track precision per instrument. Explicit precisions
are still recommended for deterministic output.
:::

### Loading CSV data in Python

You can load Tardis-format CSV data in Python using the module-level `load_tardis_*` functions.
When loading data, you can optionally specify the instrument ID, price precision, and size
precision. Providing the instrument ID improves loading performance. Price and size precision are
inferred from the CSV when omitted, but explicit values are recommended for deterministic output,
especially with large files.

To load the data, create a script similar to the following:

```python
from pathlib import Path

from nautilus_trader.adapters.tardis import load_tardis_deltas
from nautilus_trader.model import InstrumentId


instrument_id = InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
deltas = load_tardis_deltas(
    filepath=Path("YOUR_CSV_DATA_PATH"),
    price_precision=1,
    size_precision=0,
    instrument_id=instrument_id,
)
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

use nautilus_model::identifiers::InstrumentId;
use nautilus_tardis::csv::load_deltas;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Optionally specify precisions and the CSV filepath
    let price_precision = Some(1);
    let size_precision = Some(0);
    let filepath = Path::new("YOUR_CSV_DATA_PATH");

    // Optionally specify an instrument ID and/or limit
    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    let limit = None;

    let _deltas = load_deltas(
        filepath,
        price_precision,
        size_precision,
        Some(instrument_id),
        limit,
    )?;
    Ok(())
}
```

## Streaming Tardis CSV data

For memory-efficient processing of large CSV files, the Tardis integration can load and process
data in configurable chunks rather than loading entire files into memory at once. This is useful for
processing multi-gigabyte CSV files without exhausting system memory.

Python provides streaming functions for the following CSV data:

- Order book deltas (`stream_tardis_deltas` and `stream_tardis_batched_deltas`).
- Order book depth snapshots (`stream_tardis_depth10_from_snapshot5` and
  `stream_tardis_depth10_from_snapshot25`).
- Quote ticks (`stream_tardis_quotes`).
- Trade ticks (`stream_tardis_trades`).
- Funding rates (`stream_tardis_funding_rates`).
- Options chain rows (`stream_tardis_options_chain`).

Rust exposes the equivalent `stream_*` functions.

### Streaming CSV data in Python

The module-level `stream_tardis_*` functions return iterators of bounded chunks. Each function
accepts a `chunk_size` parameter that controls how many records are read per chunk:

```python
from pathlib import Path

from nautilus_trader.adapters.tardis import stream_tardis_trades
from nautilus_trader.model import InstrumentId

instrument_id = InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
filepath = Path("large_trades_file.csv")

trades = stream_tardis_trades(
    filepath=filepath,
    chunk_size=100_000,
    price_precision=1,
    size_precision=0,
    instrument_id=instrument_id,
)

# Stream trade ticks in chunks
for chunk in trades:
    print(f"Processing chunk with {len(chunk)} trades")
    # Process each chunk - only this chunk is in memory
    for trade in chunk:
        # Your processing logic here
        pass
```

### Streaming order book data

For order book data, streaming is available for both deltas and depth snapshots:

```python
from pathlib import Path

from nautilus_trader.adapters.tardis import stream_tardis_deltas
from nautilus_trader.adapters.tardis import stream_tardis_depth10_from_snapshot5


filepath = Path("book_snapshot_5.csv")

# Stream order book deltas
for chunk in stream_tardis_deltas(filepath):
    print(f"Processing {len(chunk)} deltas")
    # Process delta chunk

# Stream depth10 snapshots from snapshot_5 files
for chunk in stream_tardis_depth10_from_snapshot5(filepath):
    print(f"Processing {len(chunk)} depth snapshots")
    # Process depth chunk
```

### Streaming quote data

Quote data can be streamed similarly:

```python
from pathlib import Path

from nautilus_trader.adapters.tardis import stream_tardis_quotes


filepath = Path("quotes.csv")

# Stream quote ticks
for chunk in stream_tardis_quotes(filepath):
    print(f"Processing {len(chunk)} quotes")
    # Process quote chunk
```

### Memory use

Streaming bounds the number of parsed records retained at one time:

- **Controlled memory use**: Only one chunk is loaded in memory at a time.
- **Large file processing**: The iterator can process files larger than available RAM.
- **Configurable chunk sizes**: Tune `chunk_size` based on your system's memory and performance
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

use nautilus_model::identifiers::InstrumentId;
use nautilus_tardis::csv::stream_trades;

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    )?;

    for chunk in stream {
        let chunk = chunk?;
        println!("Processing chunk with {} trades", chunk.len());
        // Process chunk
    }

    Ok(())
}
```

## Instrument metadata

The replay pipeline and data client request metadata for every exchange in their configured Tardis
options before connecting to Tardis Machine. They use the
[Tardis instruments metadata API](https://docs.tardis.dev/api/instruments-metadata-api) to parse
instrument metadata into Nautilus definitions. The data client also publishes those definitions to
the Nautilus data engine.

:::note
A `TARDIS_API_KEY` for an active Tardis pro or business subscription is required. The automatic
bootstrap requests all instrument metadata for each configured Tardis exchange.
:::

Python and Rust users can also request instrument definitions directly with `TardisHttpClient`.
The client accepts optional `api_key`, `base_url`, `timeout_secs`, `normalize_symbols`, and
`proxy_url` arguments. It can retrieve one symbol or all instruments for an exchange. Use Tardis
lower-kebab exchange IDs such as `binance-futures`.

### Requesting instruments in Python

```python
import asyncio

from nautilus_trader.adapters.tardis import TardisHttpClient


async def run():
    http_client = TardisHttpClient()

    instrument = await http_client.instruments("bitmex", symbol="xbtusd")
    print(f"Received: {instrument}")

    instruments = await http_client.instruments("bitmex")
    print(f"Received: {len(instruments)} instruments")


if __name__ == "__main__":
    asyncio.run(run())
```

### Requesting instruments in Rust

For a complete example, see `crates/adapters/tardis/bin/example_http.rs`.

```rust
use nautilus_tardis::{
    common::enums::TardisExchange,
    http::TardisHttpClient,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let client = TardisHttpClient::new(None, None, None, true, None)?;

    // Tardis instrument definitions
    let info = client
        .instruments_info(TardisExchange::Bitmex, Some("XBTUSD"), None)
        .await?;
    println!("Received: {info:?}");

    // Nautilus instrument definitions
    let instruments = client
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
        .await?;
    println!("Received: {instruments:?}");
    Ok(())
}
```

## Nautilus data client

`TardisDataClientConfig` and `TardisDataClientFactory` integrate a configured Tardis Machine stream
with a Nautilus node. The configuration selects one mode:

- A non-empty `options` list connects to the historical `ws-replay-normalized` endpoint.
- When `options` is empty, a non-empty `stream_options` list connects to the real-time
  `ws-stream-normalized` endpoint and reconnects automatically after an interruption.

One list must be non-empty. If both are set, `options` selects historical replay mode. These request
options determine the upstream exchanges, symbols, and data types. Nautilus subscription commands
do not add or remove data from the Tardis Machine WebSocket.

The data client adds `derivative_ticker` to every configured request so it can publish funding
rates, mark prices, and index prices when their values change. It also supports the other outputs in
[supported formats](#supported-formats), including `OptionGreeks` and optional BBO `QuoteTick` data
from `option_summary` messages.

Create Python stream options from Tardis JSON, then pass them to the public data client config:

```python
from nautilus_trader.adapters.tardis import StreamNormalizedRequestOptions
from nautilus_trader.adapters.tardis import TardisDataClientConfig
from nautilus_trader.adapters.tardis import TardisDataClientFactory


stream_options = StreamNormalizedRequestOptions.from_json(
    b'{"exchange":"binance-futures","symbols":["BTCUSDT"],"dataTypes":["trade","quote"]}',
)
config = TardisDataClientConfig(stream_options=[stream_options])
factory = TardisDataClientFactory()
```

Pass `factory` and `config` to `LiveNode.builder(...).add_data_client(...)`. See
`examples/live/tardis/data_tester.py` for the node registration pattern and
`crates/adapters/tardis/examples/node_data_tester.rs` for a complete Rust replay client.

The Rust data client config can set `book_snapshot_output` to `depth10`. The Python data client
config uses the default `deltas` output; the standalone replay JSON configuration supports both
values.

## Trade ID derivation

Trade ticks use the venue-provided trade ID from the Tardis message or CSV row
as the `TradeId`. When the venue omits the trade ID (empty string or null on
some exchanges), both the WebSocket parser and CSV parser fall back to a
deterministic FNV-1a hash of the symbol, timestamp, price, amount, and side.
The same venue event yields the same trade ID across replays, keeping
downstream dedup intact.

## Limitations and considerations

`TardisDataClient` does not implement Nautilus data requests, including instrument, order book,
quote, trade, funding rate, and bar requests. Configure historical replay through `options`, or use
`run_tardis_machine_replay` for catalog workflows.

## Contributing

:::info
For additional features or to contribute to the Tardis adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
