# Nautilus Parquet Fixtures

The `64-bit/` and `128-bit/` files deliberately retain the frozen binary catalog format. Tests
select the folder for the active build and use these files to pin explicit migration reads,
fixed-width normalization, filters, and cross-precision failure behavior.

New fixtures must use the open Arrow format unless a test explicitly targets legacy
normalization. The files under `test_data/binance/` and the top-level
`quote_tick_*_rust.parquet` files are source-data fixtures, not Nautilus catalog format fixtures.

`arrow/depths.parquet` uses the final, build-independent Arrow schema with UTC nanosecond timestamps. It contains
one `AAPL.XNAS` row migrated from the 64-bit binary depth fixture: one level per side, price `1.2345`,
size `2.5`, count `3`, zero order IDs, flags `32`, sequence `7`, and event/init timestamps `100`/`200` ns.
Runtime catalog tests read this file directly. The precision-specific depth files remain migration
inputs and are not runtime catalog fixtures.
