# Bitget

Bitget is a centralized crypto exchange supporting spot and derivatives trading.
This integration targets the **Bitget Classic v2 API** and supports both mainnet and demo environments.

## Overview

The Bitget adapter currently provides these components:

- `BitgetHttpClient`: low-level HTTP connectivity plus public market-data requests and signed private trading/report requests.
- `BitgetWebSocketClient`: Bitget-specific WebSocket message builders plus Rust-owned URL, heartbeat, and websocket-config helpers.
- `BitgetInstrumentProvider`: instrument loading and normalization for Bitget spot, USDT futures, COIN futures, and USDC futures instruments.
- `BitgetDataClient`: live public market data for quote ticks, trade ticks, L2 order book deltas, bars, mark prices, index prices, and funding rates, with REST snapshot recovery on book desync.
- `BitgetExecutionClient`: trading REST flows plus private WebSocket authentication and account, order, fill, and position stream handling.
- `BitgetLiveDataClientFactory` / `BitgetLiveExecClientFactory`: trading-node factory bindings.

## Products

| Product Type | Supported | Notes |
|--------------|-----------|-------|
| Spot | ✓ | Classic v2 REST/WS. |
| USDT-FUTURES | ✓ | Perpetual + delivery futures. |
| COIN-FUTURES | ✓ | Perpetual + delivery futures. |
| USDC-FUTURES | ✓ | Perpetual + delivery futures. |

## Symbology

Bitget raw symbols are normalized to avoid collisions across product types:

- Spot: `BTCUSDT.BITGET`
- Perpetual futures: `BTCUSDT-PERP.BITGET`
- Delivery futures: `BTCUSDT-260626.BITGET`

## Environments

| Environment | HTTP Base URL | WS Public URL | WS Private URL |
|-------------|---------------|---------------|----------------|
| Mainnet | `https://api.bitget.com` | `wss://ws.bitget.com/v2/ws/public` | `wss://ws.bitget.com/v2/ws/private` |
| Demo | `https://api.bitget.com` | `wss://wspap.bitget.com/v2/ws/public` | `wss://wspap.bitget.com/v2/ws/private` |

For demo REST requests, Bitget requires header `paptrading: 1`.

## Current capability matrix

| Capability | Spot | USDT-FUTURES | COIN-FUTURES | USDC-FUTURES | Notes |
|------------|------|--------------|--------------|--------------|-------|
| Load instruments | ✓ | ✓ | ✓ | ✓ | Public HTTP instrument endpoints. |
| Public quote ticks | ✓ | ✓ | ✓ | ✓ | Public ticker channel. |
| Public trade ticks | ✓ | ✓ | ✓ | ✓ | Public trade channel. |
| Public L2 order book deltas | ✓ | ✓ | ✓ | ✓ | Public WebSocket stream with checksum validation. |
| Public order book snapshot recovery | ✓ | ✓ | ✓ | ✓ | REST snapshot fallback on book desync. |
| Public bars | ✓ | ✓ | ✓ | ✓ | Live WS candles plus REST historical bar requests. |
| Public mark prices | n/a | ✓ | ✓ | ✓ | Futures ticker channel. |
| Public index prices | n/a | ✓ | ✓ | ✓ | Futures ticker channel. |
| Public funding rates | n/a | ✓ | ✓ | ✓ | Live ticker updates plus REST current/history requests. |
| Private account stream | ✓ | ✓ | ✓ | ✓ | Private WebSocket stream. |
| Private order stream | ✓ | ✓ | ✓ | ✓ | Private WebSocket stream. |
| Private fill stream | ✓ | ✓ | ✓ | ✓ | Private WebSocket stream. |
| Private positions stream | n/a | ✓ | ✓ | ✓ | Non-spot private WebSocket stream. |
| Submit / modify / cancel orders | ✓ | ✓ | ✓ | ✓ | Trading REST requests are implemented for all supported Bitget products. |
| Cancel all / batch cancel | ✓ | ✓ | ✓ | ✓ | Symbol-scoped cancel-all and grouped batch cancel are implemented. |
| Order / fill / position status report requests | ✓ | ✓ | ✓ | ✓ | REST reconciliation/report surfaces are implemented. |

## Configuration

Environment variables:

- `BITGET_API_KEY`
- `BITGET_API_SECRET`
- `BITGET_API_PASSPHRASE`

The live data client can run without credentials for public data. The execution client requires credentials for private streams.

## Examples

Live examples are available under `examples/live/bitget/`:

- `bitget_data_tester.py`: public data smoke test for quotes, trades, L2 book deltas, bars, mark prices, index prices, and funding rates.
- `bitget_exec_tester.py`: execution smoke test for private streams plus REST order entry/cancel flows.

## Known limitations

- Historical quote-tick requests are not supported because Bitget does not publish a true historical quote endpoint.
- UTA v3 private trading/account APIs are out of scope for this integration.
