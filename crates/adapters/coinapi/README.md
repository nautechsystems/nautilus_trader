# nautilus-coinapi

[CoinAPI Market Data API](https://www.coinapi.io/products/market-data-api?ref=nautilus) integration for
[NautilusTrader](https://nautilustrader.io).

CoinAPI provides real-time and historical cryptocurrency market data across multiple exchanges
through REST and WebSocket APIs.

The adapter integrates CoinAPI's Market Data API for live feeds and historical data requests.
See the [integration guide](../../../docs/integrations/coinapi.md) for supported capabilities and
API constraints.

## Feature flags

- `examples`: Enables the crate's example binaries.
- `extension-module`: Builds as a Python extension module.
- `fuzz`: Enables libFuzzer integration for fuzz targets.
- `high-precision` (default): Enables
  [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
  to use 128-bit value types.
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `simulation`: Routes HTTP and WebSocket through deterministic transport when built with
  `cfg(madsim)`.

## Integrator attribution

The adapter automatically sends the fixed referral header `X-Referrer: nautilus` on every REST
request and WebSocket upgrade request, including retries and reconnects. API Bricks records the
referral code and determines usage attribution on its side. The header carries no credential or
account data.

See the [CoinAPI integration guide](../../../docs/integrations/coinapi.md#integrator-attribution)
for signup referral links and attribution details.

## Fuzzing

Coverage-guided fuzz targets for WebSocket, book, and REST parsing live in [`fuzz/`](fuzz/README.md).
They require the workspace-pinned `cargo-fuzz` binary and a Rust nightly toolchain.

## License

Licensed under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
