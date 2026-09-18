# Nautilus Parquet Fixtures

`legacy/64-bit/` and `legacy/128-bit/` retain the frozen binary catalog format. Tests select the
folder for the active build and use these files to pin explicit migration reads, fixed-width
normalization, filters, and cross-precision failure behavior.

`arrow/` holds the current, build-independent Arrow schema: `Decimal128(38, 16)` physical fields and
UTC nanosecond timestamps. Ordinary runtime query, filter, unread-record, and dataframe tests read
these files directly.

New fixtures must use the open Arrow format unless a test explicitly targets legacy
normalization. The files under `test_data/binance/` and the top-level
`quote_tick_*_rust.parquet` files are source-data fixtures, not Nautilus catalog format fixtures.

Regenerate the current-schema market-data files from the frozen 64-bit inputs with:

```bash
cargo run --locked --manifest-path crates/persistence/Cargo.toml --example generate-arrow-fixtures
```

Generation leaves `arrow/depths.parquet` unchanged; that file is maintained separately.

## Current Arrow fixtures

`arrow/depths.parquet` contains one `AAPL.XNAS` row migrated from the 64-bit binary depth fixture:
one level per side, price `1.2345`, size `2.5`, count `3`, zero order IDs, flags `32`, sequence `7`,
and event/init timestamps `100`/`200` ns. The precision-specific depth files remain migration
inputs and are not runtime catalog fixtures.

The remaining `arrow/` market-data files are transcoded from `legacy/64-bit/` by
`crates/persistence/examples/generate_arrow_fixtures.rs`. Values are exactly representable in both
model builds. Precision metadata from the source files is preserved. Singleton source row groups are
coalesced so the checked-in files stay under the added-file size gate.

- `quotes.parquet`: 9,500 `EUR/USD.SIM` quotes, `price_precision` `5`, `size_precision` `0`, 10 row
  groups (nine of 1,000 then 500). First `ts_init` `1577898000000000065`, last
  `1577919652000000125`. After reading the first 1,000 rows the next `ts_init` is
  `1577900944000000879`.
- `trades.parquet`: 100 `EUR/USD.SIM` trades, `price_precision` `4`, `size_precision` `4`, one row
  group. `ts_event` and `ts_init` are `0`.
- `bars.parquet`: 10 `ADABTC.BINANCE` bars, `price_precision` `8`, `size_precision` `8`, bar type
  `ADABTC.BINANCE-1-MINUTE-LAST-EXTERNAL`. First `ts_init` `1637971200000000000`, last
  `1637971740000000000`.
- `deltas.parquet`: 1,077 `1.166564490-60424-0.0.BETFAIR` order book deltas, `price_precision` `0`,
  `size_precision` `0`, one row group. First `ts_init` `1576840503572000000`, last
  `1576878616388999936`.
- `quotes-3-groups-filter-query.parquet`: 15,000 `BTCUSDT-PERP.BINANCE` quotes, `price_precision`
  `1`, `size_precision` `3`, three row groups of 5,000. First `ts_init` `1701388800012000000`, last
  `1701388904235000000`. Filter tests use `ts_init >= 1701388832486000000`.
