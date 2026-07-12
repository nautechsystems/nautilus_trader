# Binaries

Standalone executables across the workspace. These are developer tools, smoke
tests, and operational utilities — not part of the core trading engine.

Build a specific adapter's binaries with:

```bash
cargo build -p nautilus-hyperliquid --bins
```

Build all binaries:

```bash
cargo build --all-features
```

## Core tools

| Crate | Binary | Description |
|-------|--------|-------------|
| `nautilus-cli` | `nautilus` | CLI for database init/drop and blockchain operations. |
| `nautilus-pyo3` | `python-stub-gen` | Generate `.pyi` type stubs from PyO3 annotations. |
| `nautilus-persistence` | `to_json` | Convert persisted data to JSON. |
| `nautilus-persistence` | `to_parquet` | Convert persisted data to Parquet. |
| `nautilus-event-store` | `verify` | Verify event store integrity. |

## Adapter binaries

### Architect AX

| Binary | Description |
|--------|-------------|
| `architect-ax-flatten` | Cancel all open orders and close all positions. |
| `architect-ax-http-public` | Smoke test for public HTTP endpoints. |
| `architect-ax-ws-data` | Smoke test for market data WebSocket. |
| `architect-ax-ws-orders` | Smoke test for order event WebSocket. |

### Binance

| Binary | Description |
|--------|-------------|
| `binance-http-public` | Smoke test for Spot HTTP client with SBE encoding. |
| `binance-ws-spot-data` | Smoke test for Spot WebSocket SBE market data. |
| `binance-capture-spot-http-fixtures` | Capture SBE HTTP fixtures for tests. |
| `binance-capture-spot-ws-user-data` | Capture raw SBE WS user data fixtures. |

### BitMEX

| Binary | Description |
|--------|-------------|
| `bitmex-http` | Smoke test for HTTP endpoints. |
| `bitmex-ws-data` | Smoke test for market data WebSocket. |
| `bitmex-ws-exec` | Smoke test for execution WebSocket. |

### Blockchain

| Binary | Description |
|--------|-------------|
| `blockchain-node-wallet` | Node wallet operations. |

### Bybit

| Binary | Description |
|--------|-------------|
| `bybit-flatten` | Cancel all derivatives orders and close all derivatives positions. |
| `bybit-http` | Smoke test for HTTP endpoints. |
| `bybit-ws-data` | Smoke test for public WebSocket market data. |
| `bybit-ws-exec` | Smoke test for execution WebSocket. |

### Coinbase

| Binary | Description |
|--------|-------------|
| `coinbase-cancel-all-open` | Cancel every open order on the authenticated key. |
| `coinbase-http-private` | Smoke test for authenticated REST API. |
| `coinbase-http-public` | Smoke test for public REST API. |

### Deribit

| Binary | Description |
|--------|-------------|
| `deribit-http-private` | Smoke test for private API endpoints. |
| `deribit-http-public` | Smoke test for public API endpoints. |
| `deribit-ws-data` | Smoke test for WebSocket data streaming. |

### Derive

| Binary | Description |
|--------|-------------|
| `derive-flatten` | Cancel all orders and close all perp/option positions. |
| `derive-spot-research` | Venue research probe for ERC-20 spot trading. |

### dYdX

| Binary | Description |
|--------|-------------|
| `dydx-grpc-exec` | Order submission test via gRPC to dYdX v4. |
| `dydx-http-private` | Smoke test for private HTTP endpoints (account data). |
| `dydx-http-public` | Smoke test for public HTTP endpoints (historical data). |
| `dydx-ws-data` | Smoke test for public WebSocket data streams. |
| `dydx-ws-exec` | Smoke test for private WebSocket (subaccount updates). |

### Hyperliquid

| Binary | Description |
|--------|-------------|
| `hyperliquid-builder-fee-approve` | One-time builder fee approval (0% rate, attribution only). |
| `hyperliquid-builder-fee-revoke` | Revoke/cap builder fee to zero. |
| `hyperliquid-capture-test-data` | Record real API responses for mock tests. |
| `hyperliquid-flatten` | Cancel all perp orders and close all perp positions. |
| `hyperliquid-http-exec` | Smoke test for order placement via HTTP. |
| `hyperliquid-http-outcome-order` | Smoke test for HIP-4 outcome order placement. |
| `hyperliquid-http-private` | Smoke test for authenticated HTTP endpoints. |
| `hyperliquid-http-public` | Smoke test for public HTTP endpoints. |
| `hyperliquid-http-user-outcome` | Smoke test for HIP-4 split/merge outcome operations. |
| `hyperliquid-ws-data` | Smoke test for market data WebSocket. |
| `hyperliquid-ws-exec` | Smoke test for execution WebSocket. |

### Kraken

| Binary | Description |
|--------|-------------|
| `kraken-http-spot-public` | Smoke test for Spot public HTTP endpoints. |
| `kraken-http-spot-raw` | Smoke test for raw Spot HTTP responses. |
| `kraken-ws-spot-data` | Smoke test for public WebSocket market data. |

### Lighter

| Binary | Description |
|--------|-------------|
| `lighter-flatten` | Cancel all orders and close positions with IOC market orders. |
| `lighter-integrator-revoke` | Revoke Nautilus integrator approval on departure. |
| `lighter-trades-probe` | Direct probe for the `/api/v1/trades` endpoint. |

### OKX

| Binary | Description |
|--------|-------------|
| `okx-flatten` | Cancel all swap/futures orders and close all swap/futures positions. |
| `okx-http-private` | Smoke test for authenticated HTTP endpoints. |
| `okx-http-public` | Smoke test for public HTTP endpoints. |
| `okx-ws-data` | Smoke test for market data WebSocket. |
| `okx-ws-exec` | Smoke test for execution WebSocket. |

### Polymarket

| Binary | Description |
|--------|-------------|
| `polymarket-composite-filter` | Demo: combined instrument loading with multiple filters. |
| `polymarket-create-api-key` | Create/derive CLOB API credentials via L1 auth. |
| `polymarket-event-discovery` | Demo: event-based market discovery. |
| `polymarket-search-markets` | Demo: text-based market search. |
| `polymarket-trending-markets` | Demo: market discovery with Gamma API query filters. |
| `polymarket-updown-markets` | Demo: dynamic Up/Down market instrument loading. |

### Tardis

| Binary | Description |
|--------|-------------|
| `tardis-example-csv` | Example: read Tardis CSV data. |
| `tardis-example-http` | Example: fetch data via Tardis HTTP API. |
| `tardis-example-replay` | Example: replay historical data from Tardis. |
| `tardis-stream-deltas-bench` | Benchmark: stream order book deltas from Tardis. |

## LiveNode examples

These are full end-to-end examples that build and run a `LiveNode` with
strategies, data clients, and execution clients.

| Path | Description |
|------|-------------|
| `examples/tutorials/src/bin/lighter_nvda_composite_mm.rs` | Lighter NVDA RWA market making with Databento signal. Builds a live node with Databento quotes as signal and Lighter perp as execution target. |
| `examples/quickstarts/lighter-rust-data-client/src/main.rs` | Quickstart: Lighter data client with a live node. |

Build with:

```bash
cargo build -p nautilus-tutorials --bins
cargo build -p lighter-rust-data-client
```

## Common patterns

Most adapter binaries fall into a few categories:

- **`flatten`** — Emergency tool: cancel all orders and close all positions on the venue.
- **`http_public`** — Smoke test for public REST endpoints (instruments, market data).
- **`http_private`** — Smoke test for authenticated REST endpoints (account, orders).
- **`http_exec`** — Smoke test for order placement/cancellation via REST.
- **`ws_data`** — Smoke test for market data WebSocket streams.
- **`ws_exec`** — Smoke test for execution/order event WebSocket streams.
- **`capture_*`** — Record real API responses to disk for use in mock tests.

All adapter binaries connect to the **real exchange** (mainnet or testnet). They
are not part of the automated test suite.
