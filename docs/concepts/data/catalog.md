# Data catalog

The data catalog stores NautilusTrader data in [Parquet](https://parquet.apache.org) files for
backtesting, live trading, and research.

## Overview and architecture

`ParquetDataCatalog` is the Python interface to the Rust catalog and DataFusion query engine.
The Rust model and persistence crates define the Arrow schemas for built-in data. Registered custom
data supplies its schema and encode/decode handlers at runtime.

Instant timestamps use `Timestamp(Nanosecond, Some("UTC"))`; durations remain integers. Arrow readers
and Nautilus queries preserve the nanoseconds and UTC annotation. SQL readers that map these columns
to microsecond-precision `TIMESTAMPTZ` can truncate sub-microsecond values.

Parquet provides compressed columnar storage and cross-language access. The catalog stores these
files under one root without requiring a separate database service. A local path or object-store
URI selects the storage backend.

## Initializing

Pass a local path or URI as the first constructor argument:

```python
from pathlib import Path

from nautilus_trader.persistence import ParquetDataCatalog


CATALOG_PATH = Path.cwd() / "catalog"
catalog = ParquetDataCatalog(str(CATALOG_PATH))
```

## Filesystem protocols and storage options

The catalog accepts the storage protocols supported by its Rust object-store backend.

### Supported filesystem protocols

| Storage              | URI schemes        | Common option keys                                                        |
| -------------------- | ------------------ | ------------------------------------------------------------------------- |
| Local filesystem     | Plain path, `file` | None.                                                                     |
| Amazon S3            | `s3`               | `region`, `access_key_id`, `secret_access_key`, `endpoint_url`.           |
| Google Cloud Storage | `gs`, `gcs`        | `service_account_path`, `service_account_key`, `application_credentials`. |
| Azure Blob Storage   | `az`, `abfs`       | `account_name`, `account_key`, `sas_token`.                               |
| HTTP or WebDAV       | `http`, `https`    | None.                                                                     |

Pass credentials and other backend settings through `storage_options`:

```python
catalog = ParquetDataCatalog(
    "s3://my-bucket/nautilus-data/",
    storage_options={
        "access_key_id": "your-key",
        "secret_access_key": "your-secret",
        "region": "us-east-1",
    },
)

azure_catalog = ParquetDataCatalog(
    "abfs://container@account.dfs.core.windows.net/nautilus-data/",
    storage_options={"account_key": "your-account-key"},
)
```

## Writing data

Use the writer for the concrete data type. Instrument definitions and custom data have separate
writers.

```python
catalog.write_instruments([instrument])
catalog.write_quote_ticks(quote_ticks)

catalog.write_trade_ticks(
    trade_ticks,
    start=1704067200000000000,
    end=1704153600000000000,
)

catalog.write_bars(bars, skip_disjoint_check=True)
```

The built-in market-data writers are:

- `write_quote_ticks`
- `write_trade_ticks`
- `write_order_book_deltas`
- `write_order_book_depths`
- `write_bars`
- `write_mark_price_updates`
- `write_index_price_updates`
- `write_option_greeks`

Each writer accepts optional `start` and `end` overrides as UNIX nanoseconds. The data in one call
must have one identity, such as one instrument ID or bar type, and must be ordered by `ts_init`.

## File naming and data organization

The catalog names files from their timestamp range with the pattern
`{start_timestamp}_{end_timestamp}.parquet`. It converts each ISO 8601 timestamp to a filename-safe
form by replacing `:` and `.` with `-`.

Built-in data is organized in directories by data type and identifier. For instrument IDs and bar
types, the catalog removes `/` and replaces `^` with `_` when creating the URI-safe directory name:

```text
catalog/
├── data/
│   ├── quotes/
│   │   └── EURUSD.SIM/
│   │       └── 2024-01-01T00-00-00-000000000Z_2024-01-01T23-59-59-999999999Z.parquet
│   └── trades/
│       └── BTCUSD.BINANCE/
│           └── 2024-01-01T00-00-00-000000000Z_2024-01-01T23-59-59-999999999Z.parquet
```

Custom data uses `data/custom/<type_name>/` with optional identifier path segments.

:::warning[Overlapping writes]
By default, overlapping writes raise an `OSError` to maintain data integrity.
Set `skip_disjoint_check=True` only when the overlap is intentional.
:::

## Reading data

Use a typed query when the expected return type is known. `start` and `end` are UNIX nanoseconds:

```python
quotes = catalog.query_quote_ticks(
    identifiers=["EUR/USD.SIM"],
    start=1704067200000000000,
    end=1704153600000000000,
)

trades = catalog.query_trade_ticks(
    identifiers=["BTC/USD.BINANCE"],
    start=1704067200000000000,
    end=1704153600000000000,
)
```

## `BacktestDataConfig`: backtest data

`BacktestDataConfig` defines the catalog data that a `BacktestNode` loads for one run.

### Core parameters

- `data_type` is one of `QuoteTick`, `TradeTick`, `Bar`, `OrderBookDelta`, `OrderBookDepth`,
  `MarkPriceUpdate`, `IndexPriceUpdate`, `FundingRateUpdate`, `InstrumentStatus`, `OptionGreeks`, or
  `InstrumentClose`.
- `catalog_path` identifies the catalog root.
- One of `instrument_id`, `instrument_ids`, or `bar_types` is required.
- `start_time` and `end_time` are optional UNIX nanosecond bounds.
- `filter_expr` is an optional DataFusion SQL predicate.
- `catalog_fs_protocol` prefixes `catalog_path` for remote storage.
- `catalog_fs_rust_storage_options` supplies the Rust backend options. If it is unset,
  `BacktestNode` falls back to `catalog_fs_storage_options`.
- For bars, `bar_spec` combines with the instrument ID to select an `EXTERNAL` bar type. Explicit
  `bar_types` can select internal, external, or composite bars.
- `optimize_file_loading` registers whole directories when possible.

### Basic usage examples

```python
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.model import BarAggregation
from nautilus_trader.model import BarSpecification
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import PriceType

quote_data = BacktestDataConfig(
    data_type="QuoteTick",
    catalog_path="/path/to/catalog",
    instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
    start_time=1704067200000000000,
    end_time=1704153600000000000,
)

trade_data = BacktestDataConfig(
    data_type="TradeTick",
    catalog_path="/path/to/catalog",
    instrument_ids=[
        InstrumentId.from_str("BTC/USD.BINANCE"),
        InstrumentId.from_str("ETH/USD.BINANCE"),
    ],
)

bar_data = BacktestDataConfig(
    data_type="Bar",
    catalog_path="/path/to/catalog",
    instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
    bar_spec=BarSpecification(5, BarAggregation.MINUTE, PriceType.LAST),
)
```

This bar config selects `AAPL.NASDAQ-5-MINUTE-LAST-EXTERNAL`.

### Cloud storage and filtering

```python
book_data = BacktestDataConfig(
    data_type="OrderBookDelta",
    catalog_path="my-bucket/nautilus-data",
    catalog_fs_protocol="s3",
    catalog_fs_rust_storage_options={
        "access_key_id": "your-access-key",
        "secret_access_key": "your-secret-key",
        "region": "us-east-1",
    },
    instrument_id=InstrumentId.from_str("BTC/USD.COINBASE"),
    filter_expr="ts_init >= 1704067200000000000",
)
```

### Integration with BacktestRunConfig

Pass the data configurations to `BacktestRunConfig`:

```python
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OmsType

data_configs = [
    BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/path/to/catalog",
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
    ),
]

run_config = BacktestRunConfig(
    venues=[
        BacktestVenueConfig(
            name="SIM",
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            book_type=BookType.L1_MBP,
            starting_balances=["1_000_000 USD"],
        ),
    ],
    data=data_configs,
    start=1704067200000000000,
    end=1704153600000000000,
)
```

### Data loading process

When a backtest runs, the `BacktestNode` processes each `BacktestDataConfig`:

1. Create a `ParquetDataCatalog` from the configuration.
1. Load the required instrument definitions while building the engine.
1. Build and run a DataFusion query from the configuration fields.
1. Sort merged data by `ts_init` and add it to the backtest engine.

## Direct catalog access

Use `ParquetDataCatalog` to query or write a catalog directly. Use `BacktestDataConfig` when a
`BacktestNode` should load catalog data for a run. `LiveNodeConfig` has no counterpart for loading
catalog data; request historical data through a configured data client or query the catalog
directly. Its `streaming` field configures feather writing only.

## Querying and filtering

The generic query takes a catalog directory name such as `quotes`, `trades`, or `bars`. Use it when
you need the `files` or `optimize_file_loading` controls:

```python
catalog.query(
    data_type="quotes",
    identifiers=["EUR/USD.SIM"],
    start=1704067200000000000,
    end=1704153600000000000,
    where_clause="ts_event <= ts_init",
    files=None,
)
```

Typed methods such as `query_quote_ticks`, `query_trade_ticks`, and `query_bars` return the concrete
model type. `query_custom_data` resolves custom decoders through the runtime registry. `query`, the
typed market-data query methods, and `query_custom_data` use UNIX nanosecond time bounds and accept
a DataFusion SQL `where_clause`.

:::warning[Time-zone database mismatch]
With the current `Cargo.lock`, DataFusion SQL temporal functions resolve named time zones with the
transitive `chrono-tz` 0.10.4 database (IANA 2025b). Rust core time-zone operations use Jiff 0.2.35
with its bundled IANA 2026c database. Zone results can differ when zone rules change or historical
data is corrected after 2025b until DataFusion migrates.

If RustSec files unmaintained advisories for `chrono` or `chrono-tz`, maintain matching documented
ignores in `.cargo/audit.toml` and `deny.toml` until DataFusion migrates.
:::

## Catalog operations

Catalog operations rename, consolidate, or delete data files.

### Reset file names

Reset Parquet file names to match their content timestamps so filename-based filtering remains
accurate. `reset_all_file_names()` processes the entire catalog; `reset_data_file_names(...)`
targets a data path. Supply an instrument ID for data types partitioned by instrument. Without one,
the operation recursively reads the type directory and moves the renamed files into that directory.

```python
catalog.reset_all_file_names()
catalog.reset_data_file_names("quotes", "EUR/USD.SIM")
catalog.reset_data_file_names("trades", "BTC/USD.BINANCE")
```

### Recover from overlapping file names

Overlapping file names block writes to the affected directory. A rejected coverage extension never
renames files, so overlap points to an earlier rename, a manual move, or a concurrent writer. When
the file contents are still disjoint, recover the directory with a filename reset:

1. Stop all writers to the catalog.
1. Back up the affected directory.
1. Inspect the actual `ts_init` range of each file and confirm the content ranges are disjoint.
1. On a copy of the directory, map each file to its content-derived name and confirm no
   two files share a destination and no destination equals another file's current name. Then run
   `reset_data_file_names(...)` for the affected path and confirm each renamed file matches its
   content range. For custom data types, use `reset_all_file_names()` instead, which
   covers every leaf directory including custom layouts.
1. Replace the damaged directory with the repaired copy, then write a later disjoint interval to
   confirm the directory accepts writes again.

Use filename reset only for filename-only damage. When file contents overlap, resetting names
cannot reconcile the data; rebuild the affected range from source through a separately validated
process instead.

### Consolidate catalog

Combine small Parquet files to reduce file count and query overhead.
With no bounds, `consolidate_catalog()` processes each leaf data directory in the catalog.
`consolidate_data(...)` operates on one directory; supply an instrument ID for data types
partitioned by instrument.

```python
catalog.consolidate_catalog()

catalog.consolidate_catalog(
    start=1704067200000000000,
    end=1704153600000000000,
    ensure_contiguous_files=True,
)

catalog.consolidate_data(
    "quotes",
    instrument_id="EUR/USD.SIM",
    start=1704067200000000000,
    end=1706745600000000000,
)
```

### Consolidate catalog by period

Split data files into fixed periods. Durations and time bounds use nanoseconds. Both methods accept
optional bounds. Supply an identifier to the data-type method for data partitioned by instrument.

The catalog-wide method processes quotes, trades, order book deltas, order book depths, bars, index
prices, mark prices, instrument closes, and registered custom types. It logs a warning and skips
other types.

```python
DAY_NS = 86_400_000_000_000
HOUR_NS = 3_600_000_000_000

catalog.consolidate_catalog_by_period(period_nanos=DAY_NS)

catalog.consolidate_catalog_by_period(
    period_nanos=HOUR_NS,
    start=1704067200000000000,
    end=1704153600000000000,
)

catalog.consolidate_data_by_period(
    type_name="quotes",
    identifier="EUR/USD.SIM",
    period_nanos=HOUR_NS,
)

catalog.consolidate_data_by_period(
    type_name="trades",
    identifier="EUR/USD.SIM",
    period_nanos=HOUR_NS,
    start=1704067200000000000,
    end=1706745600000000000,
)
```

### Delete data range

Delete data within a time range, optionally limited to one data type and instrument. Omitting
`start` extends the range to the beginning; omitting `end` extends it to the end. For
`delete_data_range(...)`, omitting both bounds removes all matching data. Supply an instrument ID
for data partitioned by instrument.

`delete_data_range(...)` supports quotes, trades, bars, order book deltas, order book depth, and
registered custom types. Pass `order_book_depths` for order book depth and `custom/<TypeName>`
for custom data, such as `custom/MarketTickPython`.

`delete_catalog_range(...)` continues after unsupported directories, logs a warning, and leaves
their data unchanged. It also skips order book depth directories because their stored path name
differs from the direct method's type name. Use `delete_data_range(...)` when you need to confirm
that the requested type is supported.

```python
catalog.delete_catalog_range(
    start=1704067200000000000,
    end=1704153600000000000,
)

catalog.delete_catalog_range(end=1704067200000000000)

catalog.delete_data_range(
    type_name="quotes",
    instrument_id="BTC/USD.BINANCE",
)

catalog.delete_data_range(
    type_name="trades",
    instrument_id="EUR/USD.SIM",
    start=1704067200000000000,
    end=1706745600000000000,
)
```

:::danger[Permanent data removal]
Delete operations cannot be undone. The catalog splits partially overlapping files to preserve data
outside the range.
:::

## Feather streaming and conversion

The runtime can stage records in Feather and promote them into the Parquet catalog with
`StreamingConfig(writer_backend="Parquet", ...)`. Staged records become available to catalog queries
after promotion succeeds. A staging flush and a catalog commit are separate steps.

Parquet defaults to promotion on close, no interval-based promotion, and retention of committed
Feather sources. A positive `parquet_commit_interval_ms` uses live wall-clock scheduling or checks
against the supplied backtest clock during writes and flushes. See
[stream data into a Parquet catalog](../../how_to/stream_parquet_catalog.md) for defaults, configuration,
query visibility, and recovery.

`StreamingFeatherWriter` remains available for direct staging. Its completed sessions can be converted
manually with `ParquetDataCatalog.convert_stream_to_data()`.
