# Migrate a Parquet catalog

Use `nautilus catalog migrate-parquet` to rewrite an existing Nautilus Parquet catalog into the current Arrow format.
The command reads the source and writes a separate destination, which must be new or empty and must not overlap the
source. Both locations can be local paths or supported object-store URIs.

## Run the migration

Validate the source schemas first:

```bash
nautilus catalog migrate-parquet /data/catalog-old /data/catalog-new --dry-run
```

The dry run plans the migration from source schemas and layout only. It reports recognized files, schema conflicts,
and files outside the supported catalog tree, and it creates no destination files or directories. It does not decode
row values or prove the destination is writable. Resolve reported schema errors before running the conversion:

```bash
nautilus catalog migrate-parquet /data/catalog-old /data/catalog-new
```

When running from a source checkout, use:

```bash
cargo run -p nautilus-cli -- catalog migrate-parquet /data/catalog-old /data/catalog-new
```

For remote storage, pass native object-store settings with repeated `--source-option key=value` and
`--target-option key=value` arguments.

Overlap between the two locations is rejected before anything is created, and no data is written into a non-empty
destination, so a repeated invocation against a completed destination fails rather than overwriting its files. For
local paths, the destination directory itself may be created before the emptiness check runs.

The migration preserves the source files: it only reads them, and it rechecks each file's size and version markers
before converting it.

The printed report distinguishes migrated files and rows, transcoded rows, path-derived identifier rows, skipped
files, and unmigrated files; empty coverage files count as migrated files with zero rows. Inspect the report before
cutting over.

## Arrow representation

The current format uses standard Arrow types. Each representation below applies to the record families named
beside it.

| Value                   | Current Arrow representation                      | Record families                                                              |
| ----------------------- | ------------------------------------------------- | ---------------------------------------------------------------------------- |
| Prices and sizes        | `Decimal128(38, 16)`                              | Quotes, trades, bars, deltas, depth levels, mark and index prices, closes    |
| Decimal text            | UTF-8 strings                                     | Funding `rate`, instruments, order, position, snapshot, and report records   |
| Floating-point measures | `Float64`                                         | Option Greeks, position `signed_qty` and average-price/return fields         |
| Instant timestamps      | `Timestamp(Nanosecond, Some("UTC"))`              | Every `ts_event`/`ts_init` column and other instant fields                   |
| Durations and counters  | Unsigned integers                                 | Position `duration`, funding `interval`, sequences, counts, order IDs, flags |
| Market-data enums       | `Dictionary<Int8, Utf8>` enum names               | Trade aggressor side, delta action/side, close type                          |
| Record enums and IDs    | UTF-8 strings                                     | Order, position, snapshot, and report enums, sides, statuses, and IDs        |
| Structured payloads     | UTF-8 strings with `arrow.json`                   | Account balances/margins/`info`, order `info`/commissions/tags, reports      |
| Custom `Money`          | Struct of decimal amount and currency dictionary  | Custom-data `Money` fields from the Arrow macro                              |
| Depth sides             | Lists of price, size, count, and order ID structs | Order book depths                                                            |

Nanosecond values remain exact, including distinct timestamps within the same
microsecond, and durations remain integer values.

`OrderFilled` shows how the record families differ from market data: its `last_px`, `last_qty`, `order_side`,
`order_type`, and `liquidity_side` columns are all UTF-8 strings, and only its `info` column carries `arrow.json`.

Display output is a separate convenience representation and can use floating-point prices and sizes. Use raw output,
where it is available, when exact decimal values are required.

### Storage scale and precision

Physical decimals always use scale 16, while each file's schema metadata carries the domain `price_precision` and
`size_precision` used to interpret them. Standard-precision builds rescale their raw integers on encode, and readers
reject values their numeric configuration cannot represent exactly rather than rounding them. Precision metadata above
16 and `Money` currencies with precision above 16 are rejected, so DeFi precisions such as wei fall outside the
catalog representation.

### Nulls and rejected values

Undefined prices and quantities map to Arrow nulls in nullable families such as deltas, depths, closes, and mark and
index prices. Quotes, trades, and bars reject undefined values when encoding and reject nulls when decoding, naming
the field and row. Invalid values and values outside the representable range produce errors.

### Identity and file layout

Identity lives in schema metadata (`instrument_id`, `bar_type`, `type`, `class`, `type_name`) and in directory names,
and it travels with each file so a read restores it. The writer strips the in-memory nullable `identifier` column
before persisting, so stored files carry no `identifier` column. Parquet files do not store a cluster key.

The catalog groups files by data type and identifier. Custom data uses `data/custom/<type>/<identifier>/`; instruments
use a folder for each concrete instrument type. Older folder names are recognized when reading the migration source,
including `quote_tick`, `order_book_depth10`, per-class instrument directories, and `custom_<type>`.

## What converts

| Source shape                                   | Conversion                                                             |
| ---------------------------------------------- | ---------------------------------------------------------------------- |
| Fixed-width market data, 8- or 16-byte fields  | Normalize to decimals, UTC timestamps, enum dictionaries, plain UTF-8  |
| Flat 10-level and fixed-list depths            | Rebuild as variable-depth lists; null levels dropped; missing IDs zero |
| Records with `type` metadata                   | Align to the registered record schema, including `info`                |
| Instruments with `class` metadata              | Decode and re-encode into the per-class string schema                  |
| Known legacy status, funding, and close shapes | Convert through the three registered transcoders                       |
| Final-format custom data with `type_name`      | Pass through; rename `custom_<type>` to `custom/<type>`                |
| Empty coverage files                           | Copy as empty files under the renamed layout                           |

Custom-data `ts_event` and `ts_init` columns stored as `uint64` nanoseconds convert to
`timestamp("ns", tz="UTC")`. Other supported custom-data columns pass through unchanged.

Fixed-depth source formats omit order IDs, and the migration retains the zero IDs their reader returns. The
destination format preserves order IDs for subsequent writes.

### What does not convert

- **Feather trees.** `backtest` and `live` Feather trees are outside this command's scope. Convert staged runs
  through the [streaming workflow](stream_parquet_catalog.md) instead.
- **Unmigrated paths.** `portfolio_snapshot` directories, non-Parquet leaves, and unrecognized catalog directories
  are reported as unmigrated and never converted.
- **Preflight failures.** Schema conflicts, missing `type_name` or `class` metadata, fixed-binary custom columns, and
  unknown fingerprints fail preflight with every problem listed, before any destination write.

### Sources to validate before cutover

The conversions above are exercised against catalogs written by the current development format. Check the destination
carefully when the source contains:

- Data written by an older Nautilus release.
- Adapter custom data types with Arrow support, such as Betfair, Deribit, and Hyperliquid.
- Deltas, mark and index prices, closes, Greeks, current-shape funding, or record families other than
  `account_state`.

Validate the destination by querying it and comparing decoded values, identities, timestamps, and coverage intervals
against the source. File and row counts alone are not sufficient.

## Recover from a failed migration

Each source file is read fully into memory before conversion, one file at a time. Files are written sequentially with
create-only writes, so a failure can leave a partial destination behind.

:::warning[Retry into a new empty destination]
There is no resume into a partial destination. Fix the cause and retry into a new empty destination; retrying into
the partial one fails the empty-destination check.
:::

Rollback uses the preserved source: point the application back at the source catalog with a compatible application
version. There is no reverse migration, so data written only to the new destination after cutover does not carry back
to the source.

## Read the result

Point `ParquetDataCatalog`, backtest data configuration, or the live node's catalog configuration at the destination.
Use the current Nautilus version to write additional data. PyArrow, pandas, and Polars read the open Arrow values
without a Nautilus-specific binary decoder.

For new streamed data, see [Parquet streaming](stream_parquet_catalog.md).
