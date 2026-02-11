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

## Legacy datasets

These datasets predate this policy and use raw vendor formats (CSV/CSV.gz)
without `metadata.json`. They remain valid for existing tests. New datasets
should follow the Parquet standard above.

| Dataset                  | Source | Format           | Location                  | Status  |
|--------------------------|--------|------------------|---------------------------|---------|
| Tardis Deribit L2 deltas | Tardis | Parquet (large)  | `tests/test_data/large/`  | Curated |
| Tardis Deribit L2        | Tardis | CSV (checked in) | `tests/test_data/tardis/` | Legacy  |
| Tardis Binance snapshots | Tardis | CSV.gz (large)   | `tests/test_data/large/`  | Legacy  |
| Tardis Bitmex trades     | Tardis | CSV.gz (large)   | `tests/test_data/large/`  | Legacy  |
