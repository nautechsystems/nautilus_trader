# nautilus-gateio2

Rust integration adapter for the [Gate.io](https://www.gate.io/) cryptocurrency exchange.

This crate provides Nautilus Trader integration for the Gate.io exchange, enabling automated trading and market data consumption across spot, futures, margin, and options markets.

## Features

- 🚀 **High-Performance Rust Core**: Critical operations implemented in Rust for maximum performance
- 🔌 **Python Bindings**: Easy-to-use Python API via PyO3
- 📊 **Real-time Market Data**: WebSocket streaming for order books, trades, and account updates
- 💼 **Order Management**: Full support for order placement, modification, and cancellation
- 🏛️ **Portfolio Tracking**: Real-time account and position monitoring
- 🔐 **Secure Authentication**: HMAC-SHA512 signature-based authentication
- ⚡ **Async/Await**: Built on Tokio for efficient async operations
- 🌐 **Multi-Market Support**: Spot, USDT-margined perpetual futures, delivery futures, and options

## Architecture

The adapter follows a hybrid Rust/Python architecture:

```
Python Layer (nautilus_trader/adapters/gateio2/)
├── InstrumentProvider: Load instrument definitions
├── DataClient: Market data subscriptions
└── ExecutionClient: Order management
           ↓ PyO3 Bindings
Rust Core (crates/adapters/gateio2/src/)
├── HTTP Client: REST API communication
├── WebSocket Client: Real-time data streaming
├── Parsers: Data conversion to Nautilus types
└── Common: Enums, models, and utilities
```

## API Coverage

### REST API ✅
- ✅ Spot currency pairs and instruments
- ✅ Futures contracts (USDT, BTC settlement)
- ✅ Account information (spot, futures)
- ✅ Order book snapshots
- ✅ Trade history
- ✅ Order management
- ✅ HMAC-SHA512 authentication

### WebSocket Streams 🚧
- ✅ Subscription management
- ✅ Channel definitions (spot, futures, options)
- 🚧 Real-time message streaming (to be implemented)
- 🚧 Automatic reconnection (to be implemented)

## Installation

### Rust

Add this crate to your `Cargo.toml`:

```toml
[dependencies]
nautilus-gateio2 = "0.52.0"
```

### Python

The adapter is included with Nautilus Trader. Install via:

```bash
pip install nautilus_trader
```

Or install from source:

```bash
cd nautilus_trader
make install
```

## Usage

### Rust Examples

#### HTTP Client (Public Endpoints)

```rust
use nautilus_gateio2::http::GateioHttpClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create client (no credentials needed for public endpoints)
    let client = GateioHttpClient::new(
        None,  // Use default HTTP URL
        None,  // Use default spot WS URL
        None,  // Use default futures WS URL
        None,  // Use default options WS URL
        None,  // No credentials for public endpoints
    );

    // Fetch spot currency pairs
    let pairs = client.request_spot_currency_pairs().await?;
    println!("Found {} spot currency pairs", pairs.len());

    // Load all instruments (spot + futures)
    let instruments = client.load_instruments().await?;
    println!("Loaded {} instruments", instruments.len());

    // Get order book
    let order_book = client.request_spot_order_book("BTC_USDT").await?;
    println!("Order book: {:?}", order_book);

    Ok(())
}
```

#### HTTP Client (Authenticated)

```rust
use nautilus_gateio2::{
    http::GateioHttpClient,
    common::credential::GateioCredentials,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create credentials
    let credentials = GateioCredentials::new(
        "your_api_key".to_string(),
        "your_api_secret".to_string(),
    )?;

    // Create authenticated client
    let client = GateioHttpClient::new(
        None,
        None,
        None,
        None,
        Some(credentials),
    );

    // Fetch account info
    let account = client.request_spot_account().await?;
    println!("Account: {:?}", account);

    // Fetch futures account
    let futures_account = client.request_futures_account("usdt").await?;
    println!("Futures account: {:?}", futures_account);

    Ok(())
}
```

#### WebSocket Client

```rust
use nautilus_gateio2::{
    websocket::GateioWebSocketClient,
    common::enums::GateioWsChannel,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create WebSocket client
    let client = GateioWebSocketClient::new(
        None, None, None, None, None,
    );

    // Subscribe to channels
    client.subscribe(GateioWsChannel::SpotTicker {
        currency_pair: "BTC_USDT".to_string(),
    }).await?;

    client.subscribe(GateioWsChannel::FuturesOrderBook {
        contract: "BTC_USDT".to_string(),
    }).await?;

    println!("Active subscriptions: {}", client.subscription_count().await);

    Ok(())
}
```

### Python Examples

#### Basic Setup

```python
from nautilus_trader.adapters.gateio2 import (
    GateioDataClientConfig,
    GateioExecClientConfig,
    GateioInstrumentProvider,
)
from nautilus_trader.live.node import TradingNode

# Configure data client
data_config = GateioDataClientConfig(
    api_key="your_api_key",
    api_secret="your_api_secret",
)

# Configure execution client
exec_config = GateioExecClientConfig(
    api_key="your_api_key",
    api_secret="your_api_secret",
)

# Create trading node
node = TradingNode(
    data_clients={
        "GATEIO": data_config,
    },
    exec_clients={
        "GATEIO": exec_config,
    },
)

# Start the node
node.start()
```

#### Using the Rust Client Directly

```python
from nautilus_trader.core.nautilus_pyo3 import GateioHttpClient

# Create HTTP client
http_client = GateioHttpClient(
    base_http_url=None,  # Use defaults
    base_ws_spot_url=None,
    base_ws_futures_url=None,
    base_ws_options_url=None,
    api_key="your_api_key",
    api_secret="your_api_secret",
)

# Load instruments
instruments = await http_client.load_instruments()
print(f"Loaded {len(instruments)} instruments")
```

## Authentication

The adapter uses Gate.io APIv4 signature-based authentication:

1. **API Key**: Your Gate.io API key
2. **API Secret**: Your Gate.io API secret for signing requests

### Authentication Method

Gate.io uses HMAC-SHA512 signatures:
```
Sign = HMAC-SHA512(secret, payload)
payload = method + "\n" + url_path + "\n" + query_string + "\n" + hashed_payload + "\n" + timestamp
```

### Environment Variables

You can set credentials via environment variables:

```bash
export GATEIO_API_KEY="your_api_key"
export GATEIO_API_SECRET="your_api_secret"
```

## Rate Limits

Gate.io has the following rate limits:

- **Default**: 200 requests per 10 seconds for most endpoints
- **Spot Orders**: 10 requests per second
- **Futures Orders**: 100 requests per second
- **WebSocket**: 10 requests per second (spot), 100 requests per second (futures)
- **Connections**: ≤300 per IP address

## Running Examples

```bash
# HTTP client example (spot markets)
cargo run --bin gateio2-http-spot

# HTTP client example (futures markets)
cargo run --bin gateio2-http-futures

# WebSocket client example (spot)
cargo run --bin gateio2-ws-spot

# WebSocket client example (futures)
cargo run --bin gateio2-ws-futures
```

## Testing

```bash
# Run all tests
cargo test -p nautilus-gateio2

# Run with logging
RUST_LOG=debug cargo test -p nautilus-gateio2

# Run specific test
cargo test -p nautilus-gateio2 test_credentials_creation
```

## Configuration

### HTTP Client Options
- `base_http_url`: Base URL for REST API (default: https://api.gateio.ws/api/v4)
- `base_ws_spot_url`: WebSocket URL for spot (default: wss://api.gateio.ws/ws/v4/)
- `base_ws_futures_url`: WebSocket URL for futures (default: wss://fx-ws.gateio.ws/v4/ws/usdt)
- `base_ws_options_url`: WebSocket URL for options (default: wss://op-ws.gateio.ws/v4/ws/btc)
- `credentials`: Optional authentication credentials

### WebSocket Channels

The adapter supports the following WebSocket channels:

**Spot Markets:**
- `SpotTicker`: Real-time ticker updates
- `SpotOrderBook`: Order book updates
- `SpotTrades`: Trade data
- `SpotUserTrades`: User trade updates (authenticated)
- `SpotUserOrders`: User order updates (authenticated)

**Futures Markets:**
- `FuturesTicker`: Real-time ticker updates
- `FuturesOrderBook`: Order book updates
- `FuturesTrades`: Trade data
- `FuturesUserTrades`: User trade updates (authenticated)
- `FuturesUserOrders`: User order updates (authenticated)
- `FuturesPositions`: Position updates (authenticated)

## Development

### Project Structure

```
crates/adapters/gateio2/
├── src/
│   ├── lib.rs                 # Library root
│   ├── common/                # Shared utilities
│   │   ├── consts.rs          # Constants
│   │   ├── credential.rs      # Authentication
│   │   ├── enums.rs           # Gate.io-specific types
│   │   ├── models.rs          # Data structures
│   │   ├── parse.rs           # Type conversions
│   │   └── urls.rs            # URL management
│   ├── http/                  # REST API client
│   │   ├── client.rs          # HTTP implementation
│   │   └── error.rs           # Error types
│   ├── websocket/             # WebSocket client
│   │   ├── client.rs          # WS implementation
│   │   ├── error.rs           # WS errors
│   │   ├── messages.rs        # Message types
│   │   └── subscription.rs    # Subscription management
│   └── python/                # PyO3 bindings
│       ├── enums.rs           # Enum wrappers
│       ├── http.rs            # HTTP client bindings
│       └── websocket.rs       # WS client bindings
├── bin/                       # Example binaries
├── tests/                     # Integration tests
└── Cargo.toml                 # Package manifest
```

### Building

```bash
# Debug build
cargo build -p nautilus-gateio2

# Release build
cargo build -p nautilus-gateio2 --release

# With Python bindings
cargo build -p nautilus-gateio2 --features python
```

## Resources

- [Gate.io Exchange](https://www.gate.io/)
- [Gate.io API Documentation](https://www.gate.com/docs/developers/apiv4/en/)
- [Gate.io Python SDK](https://github.com/gateio/gateapi-python)
- [Gate.io WebSocket SDK](https://github.com/gateio/gatews)
- [Nautilus Trader Documentation](https://nautilustrader.io/)
- [Nautilus Adapter Guide](https://nautilustrader.io/docs/latest/developer_guide/adapters)

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests
5. Submit a pull request

## License

Licensed under the GNU Lesser General Public License v3.0.

See [LICENSE](../../../LICENSE) for details.

## Support

For issues and questions:
- GitHub Issues: [nautilus_trader/issues](https://github.com/nautilus-trader/nautilus_trader/issues)
- Discord: [Nautilus Trader Community](https://discord.gg/nautilustrader)

## Disclaimer

This software is for educational and research purposes. Use at your own risk.
Trading cryptocurrencies carries significant risk and may result in the loss of your capital.
