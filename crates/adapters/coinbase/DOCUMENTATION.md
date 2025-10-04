# Coinbase Advanced Trade API Adapter for NautilusTrader

Complete documentation for the Coinbase Advanced Trade API adapter.

⚠️ **Note**: This adapter is for **Coinbase Advanced Trade** (US and select regions), not Coinbase Pro, Coinbase International, or Coinbase Prime.

## Table of Contents

1. [Overview](#overview)
2. [Features](#features)
3. [Installation](#installation)
4. [Authentication](#authentication)
5. [HTTP REST API](#http-rest-api)
6. [WebSocket API](#websocket-api)
7. [Order Book Management](#order-book-management)
8. [Portfolio Tracking](#portfolio-tracking)
9. [Python Bindings](#python-bindings)
10. [Examples](#examples)
11. [Troubleshooting](#troubleshooting)

---

## Overview

The Coinbase Advanced Trade API adapter provides a complete integration with Coinbase's Advanced Trade API for algorithmic trading. It includes:

- **HTTP REST API** - Account management, market data, order placement
- **WebSocket API** - Real-time market data streams
- **Order Book Management** - Local order book maintenance
- **Portfolio Tracking** - Portfolio analytics and performance metrics
- **Python Bindings** - Full Python integration via PyO3

---

## Features

### ✅ HTTP REST API
- Account management (list accounts, get balances)
- Market data (products, candles, trades, order book)
- Order management (create, cancel, edit, list orders)
- Order preview and position closing
- Best bid/ask retrieval

### ✅ WebSocket API
- **9 channels supported:**
  - `heartbeats` - Keep-alive messages
  - `ticker` - Real-time price updates
  - `candles` - OHLCV data
  - `market_trades` - Trade stream
  - `level2` (l2_data) - Order book updates
  - `status` - Product status
  - `user` - User-specific data (requires auth)
  - `ticker_batch` - Batch ticker updates
  - `futures_balance_summary` - Futures balances

### ✅ Order Book Management
- Snapshot and incremental update handling
- Bid/ask tracking with automatic sorting
- Best bid/offer calculations
- Mid price and spread calculations
- Spread in basis points

### ✅ Portfolio Tracking
- Portfolio snapshots with USD valuations
- Performance metrics (returns, annualized returns)
- Holdings breakdown by currency
- Historical tracking

### ✅ Python Bindings
- Full async/await support
- JSON serialization for easy integration
- All HTTP and WebSocket methods exposed

---

## Installation

### Rust

Add to your `Cargo.toml`:

```toml
[dependencies]
nautilus-coinbase = "0.51.0"
```

### Python

The adapter is included in NautilusTrader. Install with:

```bash
pip install nautilus-trader
```

---

## Authentication

### Creating API Keys

1. Go to [Coinbase Advanced Trade](https://www.coinbase.com/settings/api)
2. Click "New API Key"
3. Select permissions:
   - **View** - Read account data and market data
   - **Trade** - Place and cancel orders
4. Save your API key name and private key

### API Key Format

- **API Key Name**: `organizations/{org-id}/apiKeys/{key-id}`
- **Private Key**: EC private key in PEM format (P-256 curve)

Example (NOT REAL - for illustration only):
```
API Key: organizations/00000000-0000-0000-0000-000000000000/apiKeys/11111111-1111-1111-1111-111111111111
Private Key: -----BEGIN EC PRIVATE KEY-----
MHcCAQEEIEXAMPLE1234567890ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567oAoGCCqGSM49
AwEHoUQDQgAEEXAMPLEKEY1234567890ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890AB
CDEFGHIJKLMNOPQRSTUVWXYZ1234567890ABCDEFGHIJKLMNOPQRSTUVWXYZ==
-----END EC PRIVATE KEY-----
```

⚠️ **Note**: The above credentials are EXAMPLE ONLY and will not work. Replace with your actual API credentials.

### JWT Authentication

The adapter uses JWT (JSON Web Tokens) with ES256 (ECDSA with P-256 curve and SHA-256) for authentication:

- **Algorithm**: ES256
- **Claims**: `sub`, `iss`, `nbf`, `exp`, `uri` (HTTP only)
- **Header**: `kid` (API key name), `nonce` (random 64-char hex), `typ: "JWT"`, `alg: "ES256"`
- **Expiration**: 120 seconds (2 minutes)

---

## HTTP REST API

### Rust Example

```rust
use nautilus_coinbase::{
    config::CoinbaseHttpConfig,
    http::CoinbaseHttpClient,
};

#[tokio::main]
async fn main() -> Result<()> {
    let config = CoinbaseHttpConfig::new(api_key, api_secret);
    let client = CoinbaseHttpClient::new(config)?;
    
    // List products
    let products = client.list_products().await?;
    
    // Get account balances
    let accounts = client.list_accounts().await?;
    
    // Create a market order
    let order_request = CreateOrderRequest {
        client_order_id: "my-order-1".to_string(),
        product_id: "BTC-USD".to_string(),
        side: OrderSide::Buy,
        order_configuration: OrderConfiguration::Market {
            market_market_ioc: MarketOrderConfig {
                quote_size: Some("100.00".to_string()),
                base_size: None,
            },
        },
    };
    let order = client.create_order(&order_request).await?;
    
    Ok(())
}
```

### Available Methods

#### Market Data
- `list_products()` - Get all trading pairs
- `get_product(product_id)` - Get specific product
- `get_candles(product_id, granularity, start, end)` - Get OHLCV data
- `get_market_trades(product_id, limit)` - Get recent trades
- `get_product_book(product_id, limit)` - Get order book
- `get_best_bid_ask(product_ids)` - Get best bid/ask for multiple products

#### Account Management
- `list_accounts()` - Get all accounts
- `get_account(account_uuid)` - Get specific account

#### Order Management
- `create_order(request)` - Create an order
- `cancel_orders(order_ids)` - Cancel orders
- `edit_order(request)` - Edit an order
- `get_order(order_id)` - Get order details
- `list_orders(product_id)` - List orders
- `preview_order(request)` - Preview order before placing
- `close_position(client_order_id, product_id, size)` - Close a position

---

## WebSocket API

### Rust Example

```rust
use nautilus_coinbase::websocket::{
    client::CoinbaseWebSocketClient,
    Channel,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize Rustls crypto provider (REQUIRED)
    rustls::crypto::aws_lc_rs::default_provider().install_default()?;
    
    let client = CoinbaseWebSocketClient::new_market_data(api_key, api_secret);
    
    // Connect
    client.connect().await?;
    
    // Subscribe to channels
    client.subscribe_heartbeats().await?;
    client.subscribe(vec!["BTC-USD".to_string()], Channel::Ticker).await?;
    client.subscribe(vec!["BTC-USD".to_string()], Channel::Level2).await?;
    
    // Receive messages
    while let Some(msg) = client.receive_message().await? {
        println!("Received: {}", msg);
    }
    
    Ok(())
}
```

### Channel Descriptions

| Channel | Description | Auth Required | Product IDs Required |
|---------|-------------|---------------|---------------------|
| `heartbeats` | Keep-alive messages | No | No |
| `ticker` | Real-time price updates | No | Yes |
| `candles` | OHLCV data | No | Yes |
| `market_trades` | Trade stream | No | Yes |
| `level2` | Order book updates | No | Yes |
| `status` | Product status | No | Yes |
| `user` | User-specific data | Yes | No |
| `ticker_batch` | Batch ticker updates | No | Yes |
| `futures_balance_summary` | Futures balances | Yes | No |

### WebSocket Endpoints

- **Market Data**: `wss://advanced-trade-ws.coinbase.com`
- **User Data**: `wss://advanced-trade-ws-user.coinbase.com`

---

## Order Book Management

### Rust Example

```rust
use nautilus_coinbase::orderbook::OrderBook;

let mut orderbook = OrderBook::new("BTC-USD".to_string());

// Process Level2 events from WebSocket
for event in level2_events {
    orderbook.process_event(&event)?;
}

// Get market metrics
let best_bid = orderbook.best_bid();
let best_ask = orderbook.best_ask();
let mid_price = orderbook.mid_price();
let spread = orderbook.spread();
let spread_bps = orderbook.spread_bps();

// Get top levels
let top_5_bids = orderbook.bids.top_levels(5);
let top_5_asks = orderbook.asks.top_levels(5);
```

### Features

- **Automatic sorting**: Bids (highest first), Asks (lowest first)
- **Snapshot handling**: Full order book initialization
- **Incremental updates**: Efficient delta processing
- **Market metrics**: Best bid/ask, mid price, spread, spread in bps

---

## Portfolio Tracking

### Rust Example

```rust
use nautilus_coinbase::portfolio::PortfolioTracker;

let mut tracker = PortfolioTracker::new(client);

// Take snapshots
let snapshot = tracker.take_snapshot().await?;
println!("Total value: ${}", snapshot.total_usd_value);

// Get performance metrics
if let Some(metrics) = tracker.performance_metrics() {
    println!("Return: {:.2}%", metrics.percentage_return);
    println!("Annualized: {:.2}%", metrics.annualized_return);
}

// Get holdings breakdown
if let Some(breakdown) = tracker.holdings_breakdown() {
    for (currency, stats) in breakdown {
        println!("{}: ${} ({:.2}%)", currency, stats.usd_value, stats.percentage);
    }
}
```

---

## Python Bindings

### Python Example

```python
import asyncio
from nautilus_trader.core.nautilus_pyo3 import CoinbaseHttpClient, CoinbaseWebSocketClient

async def main():
    # HTTP Client
    http_client = CoinbaseHttpClient(api_key, api_secret)
    products_json = await http_client.list_products()
    
    # WebSocket Client
    ws_client = CoinbaseWebSocketClient(api_key, api_secret)
    await ws_client.connect()
    await ws_client.subscribe(["BTC-USD"], "ticker")
    
    message = await ws_client.receive_message()
    print(message)

asyncio.run(main())
```

See `examples/test_python_bindings.py` for a complete example.

---

## Examples

All examples are in `crates/adapters/coinbase/examples/`:

1. **test_http.rs** - HTTP REST API demonstration
2. **test_websocket.rs** - WebSocket streaming
3. **test_orderbook.rs** - Order book management
4. **test_python_bindings.py** - Python integration

Run examples:
```bash
# Set environment variables
export COINBASE_API_KEY="your-api-key"
export COINBASE_API_SECRET="your-private-key"

# Run Rust examples
cargo run --example test_http
cargo run --example test_websocket
cargo run --example test_orderbook

# Run Python example
python examples/test_python_bindings.py
```

---

## Troubleshooting

### Common Issues

#### 1. "Could not automatically determine CryptoProvider"
**Solution**: Initialize Rustls crypto provider before using WebSocket:
```rust
rustls::crypto::aws_lc_rs::default_provider().install_default()?;
```

#### 2. "Invalid signature" or "Unauthorized"
**Causes**:
- Incorrect API key format
- Wrong private key
- System clock skew

**Solution**: Verify API credentials and ensure system time is accurate.

#### 3. Level2 channel not working
**Note**: The API returns channel name as `l2_data`, not `level2`. The adapter handles this automatically.

#### 4. Heartbeat counter type mismatch
**Note**: Heartbeat counter is `u64`, not `String`. The adapter handles this correctly.

### Rate Limits

Coinbase Advanced Trade API has rate limits. The adapter does not currently implement rate limiting, so you should implement your own rate limiting logic if needed.

### Support

For issues or questions:
- GitHub: [nautechsystems/nautilus_trader](https://github.com/nautechsystems/nautilus_trader)
- Documentation: [nautilustrader.io](https://nautilustrader.io)
- Coinbase API Docs: [docs.cdp.coinbase.com/advanced-trade](https://docs.cdp.coinbase.com/advanced-trade/docs/welcome)

---

**Last Updated**: 2025-10-03
**Version**: 0.51.0
**License**: LGPL-3.0

