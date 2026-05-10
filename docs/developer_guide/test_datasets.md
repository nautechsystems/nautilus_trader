# Test Datasets

Target standards for curating, storing, and consuming external datasets used as test fixtures.
New datasets should follow these standards. Existing datasets that predate this
policy are documented under [legacy datasets](#legacy-datasets).

## Dataset categories

**Small data** (< 1 MB) is checked directly into `tests/test_data/<source>/`
alongside a `metadata.json` file. These files are always available without network access.

**Large data** (> 1 MB) is hosted as Parquet in the R2 test-data bucket.
A SHA-256 checksum is recorded in `tests/test_data/large/checksums.json`.
The `ensure_test_data_exists()` helper downloads the file on first use and verifies integrity.

**User-fetched data** is used when a vendor license, entitlement model, or access control does not
allow NautilusTrader to redistribute the data through the public repo or the public R2 bucket.
In this model, the repo stores only a manifest, fetch instructions, and the transform code. Each
user downloads the source data with their own vendor account and converts it locally.

Use the user-fetched model when any of the following apply:

- The vendor requires each user to hold their own account, API key, or historical-data license.
- The license allows internal use but does not clearly allow redistribution of derived fixtures.
- The dataset is suitable for examples or opt-in integration tests, but not for default CI.

## Required metadata

Every curated dataset that stores or redistributes a concrete artifact must include a
`metadata.json` with at minimum:

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
| `format`        | Storage format (e.g., "Nautilus OrderBookDelta Parquet").        |
| `original_file` | Original vendor filename before transformation.                  |
| `parser`        | Parser used for transformation (e.g., "itchy 0.3.4").            |

User-fetched datasets use the same metadata fields where they apply. They should also include:

| Field                 | Description                                                           |
|-----------------------|-----------------------------------------------------------------------|
| `distribution`        | Must be `"user-fetch"`.                                               |
| `fetch_method`        | How the user acquires the source data (API, web portal, CLI, etc.).   |
| `fetch_reference`     | URL or document reference for the user‑facing download flow.          |
| `auth`                | Required credentials or entitlements, if any.                         |
| `transform_version`   | Version of the local transform pipeline that builds the final files.   |
| `redistribution`      | Short note describing redistribution limits for the dataset.          |
| `public_mirror`       | Must be `false` for restricted vendor datasets.                       |

For user-fetched datasets without a single committed or mirrored artifact, `file`, `sha256`, and
`size_bytes` may be omitted from `metadata.json`. In that case, `target_files` in `manifest.json`
is authoritative for the local output files. For user-fetched datasets, `original_url` may point
to the vendor download entry point rather than the exact file URL when the exact file is generated
per user account or per request.

Other metadata fields remain recommended where they apply. In particular, `licence` and `added_at`
should still be recorded for user-fetched datasets.

## Storage format

New datasets should be stored as **Nautilus Parquet** (not raw vendor formats).
This ensures:

- Consistent data types across all test datasets.
- No vendor format parsing at test time.
- Clear derivative-work status for licensing.

Use ZSTD compression (level 3) with 1M row groups.

User-fetched datasets should also end up as Nautilus Parquet after the local transform step.
Raw vendor files should stay outside the repo and outside the public R2 bucket.

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

### User-fetched pipelines (restricted redistribution)

For datasets that NautilusTrader cannot redistribute:

1. Commit a manifest and `metadata.json`, but do not commit the real vendor data or derived
   Parquet output.
2. Provide a local fetch command or helper that uses the user's own vendor credentials,
   entitlements, or purchased historical files.
3. Convert the vendor data locally into Nautilus Parquet.
4. Store the resulting files in a local cache path that is ignored by git.
5. Make tests and examples opt in. They should skip cleanly when the dataset is missing.

The default distribution order for new datasets is:

1. Checked in small data.
2. Public R2 large data.
3. User-fetched data.

Choose user-fetched only when the first two options are not acceptable under the vendor's terms.

Do not:

- Upload restricted vendor datasets to the public R2 bucket.
- Commit real vendor-derived Parquet files to the repo when redistribution rights are unclear.
- Make default CI depend on vendor credentials or paid historical-data access.

You may maintain a private mirror for internal CI or employees when the license permits internal
sharing. Treat this as a separate operational path, not as part of the public test-data standard.

## Adding a new dataset

1. Curate the data following the workflow above.
2. Write `metadata.json` with all required fields.
3. For small data: commit to `tests/test_data/<source>/`.
4. For large data: upload Parquet to R2, add checksum to `tests/test_data/large/checksums.json`.
5. For user-fetched data: commit the manifest and fetch instructions only. Keep the source and
   derived data out of the repo and out of the public R2 bucket.
6. Add path helper functions to `crates/testkit/src/common.rs` when shared testkit access is needed.
7. Write tests that consume the dataset.

For user-fetched data, prefer this layout:

```text
tests/test_data/<source>/<slug>/
  metadata.json
  manifest.json
  README.md
```

Use `tests/test_data/local/<source>/<slug>/` as the standard local cache path for generated
artifacts. Keep raw vendor downloads in a sibling `vendor/` directory under the same cache path
when local retention is needed.

The manifest should be machine-readable and stable. It should capture the minimum information needed
to reproduce the fetch and transform steps on another machine.

`metadata.json` is authoritative for provenance, licensing, and redistribution rules.
`manifest.json` is authoritative for fetch inputs, commands, cache locations, and output files.

Recommended manifest fields:

| Field               | Description                                                        |
|---------------------|--------------------------------------------------------------------|
| `slug`              | Stable dataset identifier.                                         |
| `vendor`            | Vendor or venue name.                                              |
| `source_type`       | `api`, `portal-download`, `purchased-archive`, etc.                |
| `source_filters`    | Symbols, event IDs, market IDs, date ranges, or file names.        |
| `target_files`      | Output Nautilus Parquet files expected after conversion.            |
| `cache_dir`         | Local output location relative to `tests/test_data/local/`.         |
| `fetch_command`     | Suggested command or script entry point.                           |
| `transform_command` | Suggested local conversion command.                                |
| `env`               | Required environment variables.                                    |
| `notes`             | Short operational notes for users.                                 |

Tests that rely on user-fetched data should:

- Be marked or grouped separately from default CI tests.
- Skip with a clear message when the local dataset is absent.
- Avoid network access unless the user explicitly opts in.
- Reuse a stable local cache path so the fetch happens once per machine.

For pytest-based tests, prefer a guard like:

```python
if not filepath.exists():
    pytest.skip(f"User-fetched test data not found: {filepath}")
```

For Rust tests that require manual dataset preparation, prefer `#[ignore]` when the test is not
expected to run in default CI.

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

## Tutorial test data

Several tutorials load user-provided market data. The `NAUTILUS_DATA_DIR` environment variable
overrides the base data path used by these tutorials. The test suite sets this variable to
`tests/test_data/local/` so that tutorials run against small sample files stored locally.

### Directory layout

```text
tests/test_data/local/
  Binance/
    BTCUSDT_T_DEPTH_2022-11-01_depth_snap.csv
    BTCUSDT_T_DEPTH_2022-11-01_depth_update.csv
  Bybit/
    2024-12-01_XRPUSDT_ob500.data.zip
  HISTDATA/
    DAT_ASCII_EURUSD_T_202001.csv.gz
```

The `tests/test_data/local/` directory is gitignored. Tests skip when the data is absent.

### Obtaining the data

**Binance depth snapshots** are available from the
[Binance public data portal](https://data.binance.vision/). Download the BTCUSDT T_DEPTH files
for 2022-11-01 and place the snap and update CSVs under `tests/test_data/local/Binance/`. For
testing, a subset of rows (e.g. first 10,000) is sufficient.

**Bybit ob500 orderbook data** is available from the Bybit CDN:

```bash
curl -L "https://quote-saver.bycsi.com/orderbook/linear/XRPUSDT/2024-12-01_XRPUSDT_ob500.data.zip" \
  -o tests/test_data/local/Bybit/2024-12-01_XRPUSDT_ob500.data.zip
```

The full file is ~360 MB. For testing, extract the first few hundred lines and repackage as a
smaller zip.

**HISTDATA tick data** is available from [histdata.com](https://www.histdata.com/). Download
EUR/USD ASCII tick data for any month and place the CSV (or `.csv.gz`) under
`tests/test_data/local/HISTDATA/`.

### Running the tests

```bash
pytest tests/docs_tests/test_tutorials.py::test_tutorial_with_local_data -v
```

Tests skip with a message when the corresponding data subdirectory is empty or missing.

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
