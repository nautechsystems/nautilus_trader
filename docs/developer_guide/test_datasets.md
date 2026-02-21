# Test Datasets

Target standards for curating, storing, and consuming external datasets used as test fixtures.
New datasets should follow these standards. Existing datasets are documented under
[legacy datasets](#legacy-datasets) and will be migrated incrementally.

## Dataset categories

**Small data** (< 1 MB) is checked directly into `tests/test_data/<source>/`
alongside a `metadata.json` file. These files are always available without network access.

**Large data** (> 1 MB) is hosted as Parquet in the R2 test-data bucket.
A SHA-256 checksum is recorded in `tests/test_data/large/checksums.json`.
The `ensure_test_data_exists()` helper downloads the file on first use and verifies integrity.

## Required metadata

Every curated dataset must include a `metadata.json` with at minimum:

| Field          | Description                                                       |
|----------------|-------------------------------------------------------------------|
| `file`         | Filename of the dataset.                                          |
| `sha256`       | SHA-256 hash of the file.                                         |
| `size_bytes`   | File size in bytes.                                               |
| `original_url` | Download URL of the original source data.                         |
| `licence`      | License terms and any redistribution constraints.                 |
| `added_at`     | ISO 8601 timestamp when the dataset was curated.                  |

These fields match the output of `scripts/curate-dataset.sh`. Additional recommended
fields for richer provenance:

| Field           | Description                                                      |
|-----------------|------------------------------------------------------------------|
| `instrument`    | Instrument symbol(s) covered.                                    |
| `date`          | Trading date(s) covered.                                         |
| `format`        | Storage format (e.g., "NautilusTrader OrderBookDelta Parquet").  |
| `original_file` | Original vendor filename before transformation.                  |
| `parser`        | Parser used for transformation (e.g., "itchy 0.3.4").            |

## Storage format

New datasets should be stored as **NautilusTrader Parquet** (not raw vendor formats).
This ensures:

- Consistent data types across all test datasets.
- No vendor format parsing at test time.
- Clear derivative-work status for licensing.

Use ZSTD compression (level 3) with 1M row groups.

## Naming convention

```
<source>_<instrument>_<date>_<datatype>.parquet
```

Examples:

- `itch_AAPL_2019-01-30_deltas.parquet`
- `tardis_BTCUSDT_2020-09-01_depth10.parquet`

## Curation workflow

### Simple files (single download)

Use `scripts/curate-dataset.sh`:

```bash
scripts/curate-dataset.sh <slug> <filename> <download-url> <licence>
```

This creates a versioned directory (`v1/<slug>/`) with the file,
`LICENSE.txt`, and `metadata.json` containing the required fields above.

### Complex pipelines (parse + transform)

For datasets requiring format conversion (e.g., binary ITCH to Parquet):

1. Write a curation function in `crates/testkit/src/<source>/` gated behind
   `#[cfg(test)]` or an `#[ignore]` test.
2. The function should: download, parse, filter, convert to NautilusTrader types, write Parquet.
3. Output the Parquet file and `metadata.json` to a local directory.
4. Upload to R2 manually, then add the checksum to `checksums.json`.

## Adding a new dataset

1. Curate the data following the workflow above.
2. Write `metadata.json` with all required fields.
3. For small data: commit to `tests/test_data/<source>/`.
4. For large data: upload Parquet to R2, add checksum to `tests/test_data/large/checksums.json`.
5. Add path helper functions to `crates/testkit/src/common.rs`.
6. Write tests that consume the dataset.

## Test runner serialization

Tests that download large data files share target paths across test binaries.
Because `nextest` runs each binary in a separate process, concurrent downloads
to the same path can race. The nextest config at `.config/nextest.toml` defines
a `large-data-tests` group with `max-threads = 1` to serialize these binaries.

When adding a new test binary that downloads large shared files, add it to the
group filter:

```toml
[[profile.default.overrides]]
filter = 'binary(grid_mm_itch) | binary(orderbook_integration) | binary(your_new_binary)'
test-group = 'large-data-tests'
```

## Regenerating datasets

When a schema change invalidates a large Parquet file, regenerate it from the
original source data using the curation tests below. After regenerating:

1. `sha256sum /tmp/<output_file>.parquet`
2. Update `tests/test_data/large/checksums.json` with the new hash.
3. Update the corresponding `metadata.json` (sha256, size_bytes).
4. Upload the Parquet file to R2.
5. Commit `checksums.json` and `metadata.json` (this also busts the CI cache).

### ITCH AAPL L3 deltas

Source: `01302019.NASDAQ_ITCH50.gz` (~4.4 GB) from NASDAQ EMI.

```bash
# Download source (keep a local copy, this is a large file)
wget -O ~/Downloads/01302019.NASDAQ_ITCH50.gz \
  "https://emi.nasdaq.com/ITCH/Nasdaq%20ITCH/01302019.NASDAQ_ITCH50.gz"

# Curation test expects source at /tmp
ln -sf ~/Downloads/01302019.NASDAQ_ITCH50.gz /tmp/01302019.NASDAQ_ITCH50.gz

# Regenerate parquet (output: /tmp/itch_AAPL.XNAS_2019-01-30_deltas.parquet)
cargo test -p nautilus-testkit --lib test_curate_aapl_itch -- --ignored --nocapture
```

### Tardis Deribit BTC-PERPETUAL L2 deltas

Source: `tardis_deribit_incremental_book_L2_2020-04-01_BTC-PERPETUAL.csv.gz` from
[Tardis](https://tardis.dev/). First-of-month data is available as free samples
(no API key required).

```bash
# Download source (free sample, no API key needed)
wget -O tests/test_data/large/tardis_deribit_incremental_book_L2_2020-04-01_BTC-PERPETUAL.csv.gz \
  "https://datasets.tardis.dev/v1/deribit/incremental_book_L2/2020/04/01/BTC-PERPETUAL.csv.gz"

# Regenerate parquet (output: /tmp/tardis_BTC-PERPETUAL.DERIBIT_2020-04-01_deltas.parquet)
cargo test -p nautilus-tardis test_curate_deribit_deltas -- --ignored --nocapture
```

## Legacy datasets

These datasets predate this policy and use raw vendor formats (CSV/CSV.gz)
without `metadata.json`. They remain valid for existing tests. New datasets
should follow the Parquet standard above.

| Dataset                  | Source | Format           | Location                  | Status  |
|--------------------------|--------|------------------|---------------------------|---------|
| Tardis Deribit L2 deltas | Tardis | Parquet (large)  | `tests/test_data/large/`  | Curated |
| ITCH AAPL L3 deltas      | NASDAQ | Parquet (large)  | `tests/test_data/large/`  | Curated |
| Tardis Deribit L2        | Tardis | CSV (checked in) | `tests/test_data/tardis/` | Legacy  |
| Tardis Binance snapshots | Tardis | CSV.gz (large)   | `tests/test_data/large/`  | Legacy  |
| Tardis Bitmex trades     | Tardis | CSV.gz (large)   | `tests/test_data/large/`  | Legacy  |
