# nautilus-coinbase

Production-ready NautilusTrader adapter for [Coinbase Advanced Trade API](https://docs.cdp.coinbase.com/advanced-trade/docs/welcome).

## Overview

This adapter provides a **complete, feature-rich** integration with Coinbase's Advanced Trade API for algorithmic trading. It includes HTTP REST API, WebSocket streaming, order book management, portfolio tracking, and full Python bindings.

## Features

### ✅ HTTP REST API
- **Account Management**: List accounts, get balances
- **Market Data**: Products, candles (OHLCV), trades, order book, best bid/ask
- **Order Management**: Create, cancel, edit, list orders
- **Advanced**: Order preview, position closing, stop-limit orders

### ✅ WebSocket API
- **9 Channels**: heartbeats, ticker, candles, market_trades, level2, status, user, ticker_batch, futures_balance_summary
- **Real-time Data**: Live price updates, order book updates, trade stream
- **Authenticated Channels**: User data, futures balances

### ✅ Order Book Management
- Local order book maintenance from Level2 updates
- Snapshot and incremental update handling
- Best bid/offer, mid price, spread calculations
- Top N levels retrieval

### ✅ Portfolio Tracking
- Portfolio snapshots with USD valuations
- Performance metrics (returns, annualized returns)
- Holdings breakdown by currency
- Historical tracking

### ✅ Python Bindings
- Full async/await support via PyO3
- All HTTP and WebSocket methods exposed
- JSON serialization for easy integration

## Quick Start

### Rust

```rust
use nautilus_coinbase::{
    config::CoinbaseHttpConfig,
    http::CoinbaseHttpClient,
};

#[tokio::main]
async fn main() -> Result<()> {
    let config = CoinbaseHttpConfig::new(api_key, api_secret);
    let client = CoinbaseHttpClient::new(config)?;

    // Get market data
    let products = client.list_products().await?;
    let accounts = client.list_accounts().await?;

    Ok(())
}
```

### Python

```python
from nautilus_trader.core.nautilus_pyo3 import CoinbaseHttpClient

client = CoinbaseHttpClient(api_key, api_secret)
products = await client.list_products()
```

## Examples

Run the included examples:

```bash
export COINBASE_API_KEY="your-api-key"
export COINBASE_API_SECRET="your-private-key"

cargo run --example test_http
cargo run --example test_websocket
cargo run --example test_orderbook
python examples/test_python_bindings.py
```

## Documentation

See [DOCUMENTATION.md](DOCUMENTATION.md) for complete documentation including:
- Authentication setup (JWT with ES256)
- API reference
- WebSocket channel descriptions
- Integration guide
- Troubleshooting tips

## Authentication

This adapter uses Cloud API Keys with JWT (ES256) authentication:
- **API Key**: `organizations/{org-id}/apiKeys/{key-id}`
- **Private Key**: EC private key in PEM format (P-256 curve)

See [Coinbase API Key documentation](https://docs.cdp.coinbase.com/advanced-trade/docs/rest-api-auth) for creating API keys.

## API Documentation

- [Coinbase Advanced Trade API](https://docs.cdp.coinbase.com/advanced-trade/docs/welcome)
- [REST API Reference](https://docs.cdp.coinbase.com/advanced-trade/reference)
- [WebSocket Documentation](https://docs.cdp.coinbase.com/advanced-trade/docs/websocket-overview)

## License

LGPL-3.0

