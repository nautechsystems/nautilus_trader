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

Conversion also rejects a file when its rows fall outside the timestamp interval in its filename; the schema-only
dry run does not detect this. Correct the source coverage in a separate copy, then retry into a new empty destination.

The printed report distinguishes migrated files and rows, transcoded rows, path-derived identifier rows, skipped
files, and unmigrated files; empty coverage files count as migrated files with zero rows. Inspect the report before
cutting over.

## Arrow representation

The current format uses standard Arrow types. Each representation below applies to the record families named
beside it.

| Value                   | Current Arrow representation                      | Record families                                                                       |
| ----------------------- | ------------------------------------------------- | ------------------------------------------------------------------------------------- |
| Prices and sizes        | `Decimal128(38, 16)`                              | Quotes, trades, bars, deltas, depth levels, mark and index prices, closes             |
| Decimal text            | UTF-8 strings                                     | Funding `rate`, instruments, order, position, snapshot, and report records            |
| Floating-point measures | `Float64`                                         | Option Greeks, position `signed_qty` and average-price/return fields                  |
| Instant timestamps      | `Timestamp(Nanosecond, Some("UTC"))`              | Every `ts_event`/`ts_init` column and other instant fields                            |
| Durations and counters  | Unsigned integers                                 | Position `duration`, funding `interval`, sequences, counts, order IDs, flags          |
| Market-data enums       | `Dictionary<Int8, Utf8>` enum names               | Trade aggressor side, delta action/side, close type, status action, Greeks convention |
| Record enums and IDs    | UTF-8 strings                                     | Order, position, snapshot, and report enums, sides, statuses, and IDs                 |
| Structured payloads     | UTF-8 strings with `arrow.json`                   | Account balances/margins/`info`, order `info`/commissions/tags, reports               |
| Custom `Money`          | Struct of decimal amount and currency dictionary  | Custom-data `Money` fields from the Arrow macro                                       |
| Depth sides             | Lists of price, size, count, and order ID structs | Order book depths                                                                     |

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

### Schema identification and compatibility

Schema fingerprints record ordered field names, Arrow types, and nullability. The migration registry uses exact
fingerprints for registered legacy transcoders. Current-schema matching checks columns by name and type, accepts plain
UTF-8 for dictionary strings, and does not require the same field order or nullability. Schema metadata is excluded
from the fingerprint: `type_name`, identity, and numeric precision remain separate requirements. There is no persisted
integer schema version. A release version alone does not identify a file's schema.

V2 catalog Arrow schemas differ from v1. Compatible updates preserve the meaning of existing fields, exact numeric
values, identities, and timestamps. A change to an incompatible schema requires an
explicit migration and documented source support; readers must not silently reinterpret old files. For files in
current directory layouts, runtime queries reject recognized legacy schemas in files selected by the query and direct
callers to migration, even when a SQL predicate would return no rows. Timestamp bounds can exclude files before schema
validation. Queries also skip older directory layouts; either case can return no rows without a schema error.
Migrate legacy catalogs before querying them.

### Nulls and rejected values

Undefined prices and quantities map to Arrow nulls in nullable families such as deltas, closes, and mark and index
prices. Quotes, trades, and bars declare their price and size columns non-nullable, reject undefined values when
encoding, and reject nulls when decoding, naming the field and row. Depth level fields are non-nullable: encoding
omits absent levels, and decoding rejects null levels and null values. Invalid values and values outside the
representable range produce errors.

### Identity and file layout

Every file's schema metadata names its type under `type_name`, for example `QuoteTick`, `OrderFilled`,
`CryptoPerpetual`, or a registered custom type, and carries its identity under `instrument_id` or `bar_type`.
Directory names repeat that identity. Market-data files, including funding rates, instrument status, and option
Greeks, keep the instrument ID only in metadata; record files also store it as a column. The writer strips the
in-memory nullable `identifier` column before persisting, so stored files carry no `identifier` column. Parquet files
do not store a cluster key.

The catalog groups files by data type and identifier. Custom data uses `data/custom/<type>/<identifier>/`; instruments
use a folder for each concrete instrument type. Older folder names are recognized when reading the migration source,
including `quote_tick`, `order_book_depth10`, per-class instrument directories, and `custom_<type>`.

## What converts

| Source shape                                   | Conversion                                                             |
| ---------------------------------------------- | ---------------------------------------------------------------------- |
| Fixed-width market data, 8- or 16-byte fields  | Normalize to decimals, UTC timestamps, enum dictionaries, plain UTF-8  |
| Flat 10-level and fixed-list depths            | Rebuild as variable-depth lists; null levels dropped; missing IDs zero |
| Records with legacy `type` metadata            | Align to the registered record schema, including `info`                |
| Instruments with legacy `class` metadata       | Decode and re-encode into the per-class string schema                  |
| Known legacy status, funding, and close shapes | Convert through the three registered transcoders                       |
| Final-format custom data with `type_name`      | Pass through; rename `custom_<type>` to `custom/<type>`                |
| Legacy custom data without `type_name`         | Infer the type from the `custom_<type>` directory and rename           |
| Empty coverage files                           | Copy as empty files under the renamed layout                           |

The migration then decodes every built-in destination batch and encodes it again with the current encoder, so
migrated files carry the same schema and metadata as files that later writes produce, and the two consolidate. Rows
that the current decoders reject fail the migration.

Custom-data `ts_event` and `ts_init` columns stored as `uint64` nanoseconds convert to
`timestamp("ns", tz="UTC")`. Legacy dictionary-encoded string columns become plain UTF-8;
other supported custom-data columns pass through unchanged.
Custom files whose timestamps are already nanosecond Arrow timestamps pass through unchanged.
Files whose timestamps use any other physical type (notably `int64` from older pandas-written
catalogs) fail preflight: recast them to `uint64` nanoseconds before migrating.

Type-name inference from `custom_<type>` directories is a best-effort snake_case to PascalCase
conversion: acronyms do not survive it, so verify the inferred destination directories before
cutover.

Fixed-depth source formats omit order IDs, and the migration retains the zero IDs their reader returns. The
destination format preserves order IDs for subsequent writes.

### What does not convert

- **Feather trees.** `backtest` and `live` Feather trees are outside this command's scope. Convert staged runs
  through the [streaming workflow](stream_parquet_catalog.md) instead.
- **Unmigrated paths.** `portfolio_snapshot` directories, non-Parquet leaves, and unrecognized catalog directories
  are reported as unmigrated and never converted.
- **Preflight failures.** Schema conflicts, missing `type_name` metadata outside legacy `custom_<type>`
  directories, instruments without `class` or `type_name` metadata, fixed-binary custom columns, custom timestamps
  that are neither `uint64` nor nanosecond Arrow timestamps, and unknown fingerprints fail preflight with every
  problem listed, before any destination write.

### Verified sources and cutover checks

Released-source coverage is limited to the following fixtures from version **1.231.0**:

| Source writer                   | Numeric configuration | Verified families                             |
| ------------------------------- | --------------------- | --------------------------------------------- |
| Released Python Linux wheel     | 128-bit               | Quotes, `CurrencyPair`, Binance mark updates  |
| Rust source at the released tag | 64-bit                | Quotes and `CurrencyPair`                     |
| Released Python adapter writer  | Precision-independent | Binance mark updates in both fixture catalogs |

The Binance fixture is `BinanceFuturesMarkPriceUpdate`: its prices and funding rate are strings, and the same
released adapter file is tested with both numeric configurations. Tests compare decoded quote and instrument fields,
all adapter Arrow fields, partition identities, nanosecond timestamps, known-empty coverage, and source bytes.
Adapter verification covers migration and raw Arrow queries. It does not promise typed decoding with a changed
adapter schema. The released Python instrument writer omits tick schemes, so migration cannot restore them.
Fixture provenance and reproduction instructions accompany the source data in
`test_data/nautilus/catalog_1_231_0/`.

This matrix does not establish support for every 1.x release, instrument class, or adapter. Other releases and
families require their own source fixtures and cutover comparisons. Unknown fingerprints and unsupported custom
timestamp types remain preflight errors.

Additional end-to-end coverage comes from develop commit `1602043deb`, built with the `high-precision` feature
enabled. It covers quotes, trades, bars, fixed-depth order books, `CurrencyPair` instruments, account-state records,
and a generic custom data type. The tests compare decoded destination values against the original values and check
that the source files remain byte-identical. This fixture does not establish a minimum supported release or cover
every historical catalog format.

Synthetic instrument regression tests also cover plain and dictionary-encoded strings in `CryptoPerpetual` files
with legacy `class` metadata, both `uint64` and `timestamp("ns", tz="UTC")` timestamps, and with or without the removed
`maker_fee` and `taker_fee` columns.
Migration drops those fee columns because the current instrument model no longer stores them. Runtime queries reject
these legacy instrument files and direct users to the migration command; they do not convert files while reading.

Check the destination carefully when the source contains:

- Data written by an older Nautilus release.
- Instrument types other than `CurrencyPair` and `CryptoPerpetual`.
- Adapter custom data outside the Binance fixture above, such as Betfair, Deribit, and Hyperliquid.
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
