# PR1 Notes (Instruments)

## Summary
- Implemented Lighter public HTTP models, parser, and client for `orderBooks`, mapping to `CryptoPerpetual` instruments with decimals → increments fallback.
- Exposed PyO3 HTTP client and wired the Python `LighterInstrumentProvider` to ingest instruments, apply filters (including market index), and cache market indices for ingested instruments only.
- Added fixtures (`tests/test_data/lighter/http/orderbooks.json`) and unit tests for provider/filter behavior.

## Outstanding / Blockers
- Python unit tests cannot be executed in CI locally because the runtime alternated between conda (Py3.11) and the project venv (Py3.12). The compiled extensions exist for 3.12, but pytest was run with 3.11, causing `ModuleNotFoundError: nautilus_trader.core.data`.
- To resolve: run tests with the same interpreter used to build the extension (activate `.venv` and ensure `maturin develop --locked --manifest-path crates/pyo3/Cargo.toml -F extension-module` is run with that interpreter), or rebuild for the conda Python and run pytest there. Once aligned, the remaining failing test (`test_load_all_caches_market_index`) should pass with the latest provider caching fix.

## Next Steps
- Standard path: `source .venv/bin/activate && maturin develop --locked --manifest-path crates/pyo3/Cargo.toml -F extension-module && python -m pytest tests/unit_tests/adapters/lighter -q`.
- If using conda: rebuild the extension for that Python (same `maturin develop ... --interpreter python3.11`), then run pytest with that interpreter.
