# Binance fixture data

This directory stores crate-local fixtures for the Rust Binance adapter.

Start with the canonical examples from the Binance API docs.

See [`SOURCES.md`](SOURCES.md) for the current source map by parser surface.

## Current fixture directories

- `spot/http_json/` holds Spot REST JSON fixtures.
- `spot/user_data_json/` holds Spot user-data JSON fixtures.
- `futures/http_json/` holds Futures HTTP JSON fixtures.
- `futures/market_data_json/` holds Futures market-data stream fixtures.
- `futures/user_data_json/` holds Futures user-data stream fixtures.

## When to use live capture

- The parser consumes SBE wire payloads that Binance docs do not publish as raw bytes.
- The docs example omits fields we need to test.
- We need to confirm that a live payload still matches the docs example.

Two capture binaries produce raw SBE fixture data.

### HTTP SBE capture

```bash
cargo run --bin binance-spot-http-capture-fixtures --package nautilus-binance
```

Writes fixtures under `spot/http_sbe/{env}/{category}/`:

- `spot/http_sbe/mainnet/public/` for public mainnet captures.
- `spot/http_sbe/testnet/private_read/` for signed read-only testnet captures.
- `spot/http_sbe/testnet/order_flow/` for testnet order-flow captures.
- `spot/http_sbe/demo/private_read/` for signed read-only demo captures.
- `spot/http_sbe/demo/order_flow/` for demo order-flow captures.

Public capture requires no credentials. Signed read-only capture requires Spot
credentials for the selected environment. Order-flow capture is blocked on mainnet.
It only runs on `testnet` or `demo` with `--include-order-flow`,
`--order-quantity`, and `--order-price`.

### WebSocket user data SBE capture

```bash
cargo run --bin binance-spot-ws-user-data-capture --package nautilus-binance -- \
  --environment testnet --include-order-flow \
  --order-quantity 0.001 --order-price 10000
```

Writes fixtures under `spot/user_data_sbe/{env}/`. Connects at the raw
WebSocket level, authenticates via `session.logon`, and subscribes to the user
data stream. With `--include-order-flow`, places and cancels a limit order to
trigger execution report (template 603) and account position (template 607)
events. All captured frames are raw SBE binary.

### Capture output format

Each capture run writes:

- Raw SBE payload bytes in a `.sbe` file.
- Per-fixture metadata in a `.metadata.json` file.
- An aggregate `manifest.json` for the full capture run.

## Source priority

Use docs examples as the primary source for JSON fixtures. Use captured SBE
payloads as the wire-format source for `decode_*` coverage when Binance only
documents schema fields and message semantics.

Some branch-specific JSON fixtures are derived from a published docs example
when Binance only shows one state for a message. Keep those cases narrow.
Document each derived case in `SOURCES.md`.
