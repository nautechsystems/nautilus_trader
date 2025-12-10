# PR1 Notes (Instruments)

## Summary

- Implemented Lighter public HTTP models, parser, and client for `orderBooks`, mapping to `CryptoPerpetual` instruments with decimals → increments fallback.
- Exposed PyO3 HTTP client and wired the Python `LighterInstrumentProvider` to ingest instruments, apply filters (including market index), and cache market indices for ingested instruments only.
- Added fixtures (`tests/test_data/lighter/http/orderbooks.json`) and unit tests for provider/filter behavior.

## Status / Blockers

- Blocking issue resolved: Python unit tests now pass when using the aligned venv (Py3.13) with `pytest-asyncio` installed and plugin autoload disabled to avoid rerunfailures socket binding.

## Next Steps

- Standard path: `source .venv/bin/activate && PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 python -m pytest -p pytest_asyncio.plugin tests/unit_tests/adapters/lighter -q`.
- If using conda/alt Python: rebuild the extension for that interpreter with `maturin develop --locked --manifest-path crates/pyo3/Cargo.toml -F extension-module --interpreter <python>` and run the same pytest command.
