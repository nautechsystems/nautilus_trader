# Released Catalog Fixtures

These files come from NautilusTrader 1.231.0 writers. They verify a bounded migration contract, including exact
nanosecond timestamps and filename coverage that starts before the first quote. Current readers reject legacy
built-in schemas under current directory names, but skip older directory layouts. Migrate these fixtures before
querying them. The migration preserves every source file.

| Fixture directory | Writer                | Families                                     |
| ----------------- | --------------------- | -------------------------------------------- |
| `128-bit`         | Released Python wheel | Quotes, `CurrencyPair`, Binance mark updates |
| `64-bit`          | Released Rust source  | Quotes and `CurrencyPair`                    |
| Both directories  | Python adapter writer | Identical Binance mark-update file           |

The Python source is the PyPI artifact
`nautilus_trader-1.231.0-cp312-cp312-manylinux_2_35_x86_64.whl`, with SHA-256
`8c438e95c275a13df0c0ddb7012c462708b5e99ff3612e36a1b7bd49ab39c216`.
It uses 16-byte fixed binary prices and sizes. Its instrument strings include dictionary-encoded IDs and currencies.
The released instrument writer omits `tick_scheme_name`; its own reader returns `None` for that field.
The expected instrument values come from that reader. Migration cannot reconstruct fields absent from the source.

The standard-precision source is the released `v1.231.0` tag, peeled commit
`27a8e54e7ac3c57d6cbf8891f0283dfbaee97317`. Its Rust writer uses 8-byte fixed binary prices and sizes.
The Binance custom fixture comes from the Python adapter writer: its prices and rate are UTF-8 decimal text, so
it has no precision-dependent fixed binary fields. Copying it into the standard catalog tests the same released
adapter schema against the standard decoder; it does not claim a standard-precision Python wheel exists.

## Reproduce

Use a new empty destination for each generator. Run `generate.py` with the released CPython 3.12 wheel installed:

```bash
python generate.py /tmp/released-catalog-128
```

In a separate checkout of the released tag, place `generate.rs` at
`crates/persistence/examples/generate_release_catalog.rs` and declare that example in the crate's `Cargo.toml`.
Run it without `high-precision`:

```bash
CARGO_BUILD_JOBS=12 cargo run --locked --no-default-features -p nautilus-persistence \
  --example generate_release_catalog -- /tmp/released-catalog-64
```

Copy the Python-generated `data/custom_binance_futures_mark_price_update/` tree into the standard catalog and add
its `custom` entry from the Python `expected.json` to the Rust `expected.json`. The generators use the released APIs,
so run them in the released environments. Parquet bytes can vary with dependency versions; the committed files and
`expected.json` values are the regression inputs.

These fixtures do not certify other releases, instruments, adapters, historical depth layouts, or typed decoding
of legacy adapter payloads with a current adapter class. Synthetic and develop-source tests cover additional shapes
without extending this released-source support matrix.
