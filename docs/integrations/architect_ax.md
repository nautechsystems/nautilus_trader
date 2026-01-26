# AX Exchange

[AX Exchange](https://architect.exchange) is the world's first centralized and regulated exchange
for perpetual futures on traditional underlying asset classes. It combines innovations from digital
asset perpetual exchanges with the safety, security, and risk management of traditional futures
exchanges.

:::warning
This integration is currently under construction and not yet ready for use.
:::

## Key features

- Perpetual contracts that never expire, eliminating rollover costs.
- Cross-margin trading across multiple assets.
- Real-time market data, positions, and risk visibility.
- Licensed under the [Bermuda Monetary Authority (BMA)](https://www.bma.bm/).

## Supported asset classes

| Asset Class      | Examples                   |
|------------------|----------------------------|
| Foreign exchange | EUR/USD, GBP/USD, USD/JPY. |
| Stock indexes    | S&P 500, Nasdaq 100.       |
| Interest rates   | SOFR, Treasury yields.     |
| Metals           | Gold, Silver.              |
| Energy           | Crude Oil, Natural Gas.    |

## Adapter overview

This adapter is implemented in Rust, with optional Python bindings for use in Python-based
workflows. The adapter uses REST for reference data and order management, with WebSocket for
real-time market data and execution updates.

## Components

The adapter includes multiple components which can be used together or separately depending on the
use case:

- `AxHttpClient`: Low-level HTTP API connectivity.
- `AxMdWebSocketClient`: Market data WebSocket connectivity.
- `AxOrdersWebSocketClient`: Orders WebSocket connectivity.
- `AxDataClient`: Market data feed manager.
- `AxInstrumentProvider`: Instrument parsing and loading functionality.

:::note
Most users will define a configuration for a live trading node and won't need to work with these
lower-level components directly.
:::

## AX Exchange documentation

AX Exchange provides documentation for users:

- [AX Exchange](https://architect.exchange/) - Main exchange website.
- [API Reference](https://docs.sandbox.x.architect.co/api-reference/) - Complete API documentation.

It's recommended you refer to the AX Exchange documentation in conjunction with this NautilusTrader
integration guide.

## API credentials

API credentials are required for authentication. Provide these via environment variables.

### Required credentials

| Environment Variable | Description                                    |
|----------------------|------------------------------------------------|
| `AX_API_KEY`         | Your AX Exchange API key (e.g., `ak_...`).     |
| `AX_API_SECRET`      | Your AX Exchange API secret.                   |

### Optional 2FA credentials

If your account has two-factor authentication (2FA) enabled:

| Environment Variable | Description                                       |
|----------------------|---------------------------------------------------|
| `AX_TOTP_SECRET`     | Base32 TOTP secret for auto-generating 2FA codes. |

This is the base32 secret displayed when you set up 2FA (often shown as a QR code or text).

### Environment selection

| Environment Variable | Description                                                              |
|----------------------|--------------------------------------------------------------------------|
| `AX_IS_SANDBOX`      | Set to `true` for sandbox environment (default), `false` for production. |

## Authentication

AX Exchange uses bearer token authentication via HTTP headers:

1. API key and secret (with optional TOTP) obtain a session token via `/authenticate`.
2. The session token is used as a bearer token for subsequent REST and WebSocket requests.

Session tokens expire after a configurable period (default: 3600 seconds).

## Configuration

### API endpoints

| Environment | HTTP API                                         | Market Data WebSocket                            | Orders WebSocket                                     |
|-------------|--------------------------------------------------|--------------------------------------------------|------------------------------------------------------|
| Sandbox     | `https://gateway.sandbox.architect.exchange/api` | `wss://gateway.sandbox.architect.exchange/md/ws` | `wss://gateway.sandbox.architect.exchange/orders/ws` |
| Production  | `https://gateway.architect.exchange/api`         | `wss://gateway.architect.exchange/md/ws`         | `wss://gateway.architect.exchange/orders/ws`         |

## Contributing

:::info
For additional features or to contribute to the Architect AX adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
