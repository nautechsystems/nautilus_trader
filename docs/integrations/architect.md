# Architect

[Architect](https://architect.exchange) is a financial technology company building modern
infrastructure for derivatives trading. This integration provides connectivity to two products:

- **AX Exchange** (`AX`): A regulated perpetual futures exchange for traditional asset classes.
- **Architect Brokerage** (`ARCHITECT`): A US-regulated multi-asset brokerage for equities, futures, and options.

:::warning
This integration is currently under construction and not yet ready for use.
:::

## AX Exchange

[AX Exchange](https://architect.exchange) is the world's first centralized and regulated exchange
for perpetual futures on traditional underlying asset classes. It combines innovations from digital
asset perpetual exchanges with the safety, security, and risk management of traditional futures
exchanges.

Key features:

- Perpetual contracts that never expire, eliminating rollover costs.
- Cross-margin trading across multiple assets.
- Real-time market data, positions, and risk visibility.
- Licensed under the [Bermuda Monetary Authority (BMA)](https://www.bma.bm/).

Supported asset classes:

| Asset Class      | Examples                   |
|------------------|----------------------------|
| Foreign exchange | EUR/USD, GBP/USD, USD/JPY. |
| Stock indexes    | S&P 500, Nasdaq 100.       |
| Interest rates   | SOFR, Treasury yields.     |
| Metals           | Gold, Silver.              |
| Energy           | Crude Oil, Natural Gas.    |

## Architect Brokerage

[Architect](https://architect.co) operates a US-regulated multi-asset brokerage offering equities,
futures, and options trading with full-featured APIs.

Key features:

- Multi-language SDK support (Rust, Python, JavaScript).
- Real-time market data streaming.
- Execution algorithms.
- Paper trading capabilities.

Regulatory status:

- Architect Securities LLC: SEC-registered broker-dealer, FINRA/SIPC member (equities/options).
- Architect Financial Derivatives LLC: NFA-registered introducing broker (futures).

Connected venues: CME Group, Cboe, Nasdaq, NYSE, Coinbase Derivatives.

:::note
The Architect brokerage integration is planned for future development. The current implementation
focuses on AX Exchange perpetual futures.
:::

## Overview

This adapter is implemented in Rust, with optional Python bindings for use in Python-based
workflows. The adapter uses REST for reference data and order management, with WebSocket for
real-time market data and execution updates.

## Components

The adapter includes multiple components which can be used together or separately depending on the
use case:

- `ArchitectHttpClient`: Low-level HTTP API connectivity.
- `ArchitectWebSocketClient`: Low-level WebSocket API connectivity.
- `ArchitectInstrumentProvider`: Instrument parsing and loading functionality.
- `ArchitectDataClient`: Market data feed manager.
- `ArchitectExecutionClient`: Account management and trade execution gateway.

:::note
Most users will define a configuration for a live trading node and won't need to work with these
lower-level components directly.
:::

## Product support

| Integration | Product Type      | Data Feed | Trading | Notes                                       |
|-------------|-------------------|-----------|---------|---------------------------------------------|
| AX          | Perpetual Futures | ✓         | ✓       | FX, rates, metals, and traditional assets.  |
| ARCHITECT   | Futures           | -         | -       | *Not yet supported.* Planned.               |
| ARCHITECT   | Equities          | -         | -       | *Not yet supported.* Planned.               |
| ARCHITECT   | Options           | -         | -       | *Not yet supported.* Planned.               |

## Architect documentation

Architect provides documentation for users:

- [AX Exchange](https://architect.exchange/) - Main exchange website.
- [Architect Brokerage](https://architect.co/) - Brokerage platform website.
- [API Reference](https://docs.sandbox.x.architect.co/api-reference/) - Complete API documentation.

It's recommended you refer to the Architect documentation in conjunction with this NautilusTrader
integration guide.

## API credentials

API credentials are required for authentication. Provide these via environment variables.

### Required credentials

| Environment Variable    | Description                              |
|-------------------------|------------------------------------------|
| `ARCHITECT_API_KEY`     | Your Architect API key (e.g., `ak_...`). |
| `ARCHITECT_API_SECRET`  | Your Architect API secret.               |

### Optional 2FA credentials

If your account has two-factor authentication (2FA) enabled:

| Environment Variable    | Description                                       |
|-------------------------|---------------------------------------------------|
| `ARCHITECT_TOTP_SECRET` | Base32 TOTP secret for auto-generating 2FA codes. |

This is the base32 secret displayed when you set up 2FA (often shown as a QR code or text).

### Environment selection

| Environment Variable    | Description                                                              |
|-------------------------|--------------------------------------------------------------------------|
| `ARCHITECT_IS_SANDBOX`  | Set to `true` for sandbox environment (default), `false` for production. |

## Authentication

Architect uses bearer token authentication via HTTP headers:

1. API key and secret (with optional TOTP) obtain a session token via `/authenticate`.
2. The session token is used as a bearer token for subsequent REST and WebSocket requests.

Session tokens expire after a configurable period (default: 3600 seconds).

## Configuration

### AX Exchange API endpoints

| Environment | HTTP API                                         | Market Data WebSocket                            | Orders WebSocket                                     |
|-------------|--------------------------------------------------|--------------------------------------------------|------------------------------------------------------|
| Sandbox     | `https://gateway.sandbox.architect.exchange/api` | `wss://gateway.sandbox.architect.exchange/md/ws` | `wss://gateway.sandbox.architect.exchange/orders/ws` |
| Production  | `https://gateway.architect.exchange/api`         | `wss://gateway.architect.exchange/md/ws`         | `wss://gateway.architect.exchange/orders/ws`         |
