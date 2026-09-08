# Migrate a Parquet catalog

Use `nautilus catalog migrate-parquet` to rewrite an existing Nautilus Parquet catalog into the current Arrow format.
The command reads the source and writes a separate, empty destination. Both locations can be local paths or supported
object-store URIs.

## Run the migration

Validate the source schemas first:

```bash
nautilus catalog migrate-parquet /data/catalog-old /data/catalog-new --dry-run
```

The dry run reports recognized files, schema conflicts, and files outside the supported catalog tree. It creates no
destination files. Unrecognized fixed-width byte columns cannot be converted automatically; they are rejected rather
than copied into the new catalog. Resolve reported schema errors before running the conversion:

```bash
nautilus catalog migrate-parquet /data/catalog-old /data/catalog-new
```

When running from a source checkout, use:

```bash
cargo run -p nautilus-cli -- catalog migrate-parquet /data/catalog-old /data/catalog-new
```

The destination must be empty and must not overlap the source. A repeated invocation against the completed destination
fails rather than overwriting its files. For remote storage, pass native object-store settings with repeated
`--source-option key=value` and `--target-option key=value` arguments.

The migration preserves the source files. Its report distinguishes migrated rows, empty coverage files, and unsupported
files. Feather streams are outside this migration's scope.

## Arrow representation

| Value              | Current Arrow representation                                       |
| ------------------ | ------------------------------------------------------------------ |
| Prices and sizes   | `Decimal128(38, 16)`, with Arrow nulls for undefined values.       |
| Instant timestamps | `Timestamp(Nanosecond, Some("UTC"))`, representing UTC instants.   |
| Model enums        | `Dictionary<Int8, Utf8>` containing enum names.                    |
| JSON fields        | UTF-8 strings carrying the `arrow.json` extension annotation.      |
| Depth sides        | Variable-length lists of price, size, count, and order ID structs. |

Nanosecond values remain exact, including distinct timestamps within the same microsecond. Durations remain integer
values. Parquet files do not store a cluster key.

The catalog groups files by data type and identifier. Custom data uses `data/custom/<type>/<identifier>/`; instruments
use a folder for each concrete instrument type. Older supported folder names are recognized when reading the migration
source.

Develop's fixed-depth format omits order IDs. Migration retains the zero IDs returned by its reader; the new format
preserves order IDs for subsequent writes.

## Read the result

Point `ParquetDataCatalog`, backtest data configuration, or the live node's catalog configuration at the destination.
Use the current Nautilus version to write additional data. PyArrow, DuckDB, pandas, and Polars can read the open Arrow
values without a Nautilus-specific binary decoder.

For new streamed data, see [Parquet streaming](stream_parquet_catalog.md).
