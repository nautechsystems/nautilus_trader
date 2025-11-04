# nautilus-lighter2

Rust integration adapter for the [Lighter](https://lighter.xyz/) decentralized derivatives exchange.

This crate provides Nautilus Trader integration for the Lighter exchange, enabling automated trading and market data consumption.

## Features

- 🚀 **High-Performance Rust Core**: Critical operations implemented in Rust for maximum performance
- 🔌 **Python Bindings**: Easy-to-use Python API via PyO3
- 📊 **Real-time Market Data**: WebSocket streaming for order books, trades, and account updates
- 💼 **Order Management**: Full support for order placement, modification, and cancellation
- 🏛️ **Portfolio Tracking**: Real-time account and position monitoring
- 🔐 **Secure Authentication**: API key and Ethereum private key management
- ⚡ **Async/Await**: Built on Tokio for efficient async operations

## Architecture

The adapter follows a hybrid Rust/Python architecture:

```
Python Layer (nautilus_trader/adapters/lighter2/)
├── InstrumentProvider: Load instrument definitions
├── DataClient: Market data subscriptions
└── ExecutionClient: Order management
           ↓ PyO3 Bindings
Rust Core (crates/adapters/lighter2/src/)
├── HTTP Client: REST API communication
├── WebSocket Client: Real-time data streaming
├── Parsers: Data conversion to Nautilus types
└── Common: Enums, models, and utilities
```

## API Coverage

### REST API ✅
- ✅ Market/instrument data
- ✅ Account information
- ✅ Order book snapshots
- ✅ Trade history
- ✅ Order management
- ✅ Nonce management for transactions

### WebSocket Streams ✅
- ✅ Order book updates (real-time)
- ✅ Trade data
- ✅ Account balance updates
- ✅ Order status updates
- ✅ Automatic reconnection
- ✅ Subscription management

## Installation

### Rust

Add this crate to your `Cargo.toml`:

```toml
[dependencies]
nautilus-lighter2 = "0.52.0"
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
use nautilus_lighter2::http::LighterHttpClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create client
    let client = LighterHttpClient::new(
        None,  // Use default mainnet URL
        None,  // Use default WS URL
        false, // Not testnet
        None,  // No credentials for public endpoints
    );

    // Fetch markets
    let markets = client.request_markets().await?;
    println!("Found {} markets", markets.len());

    // Load instruments
    let instruments = client.load_instruments().await?;
    println!("Loaded {} instruments", instruments.len());

    // Get order book
    let order_book = client.request_order_book(0).await?;
    println!("Order book: {}", order_book);

    Ok(())
}
```

#### HTTP Client (Authenticated)

```rust
use nautilus_lighter2::{
    http::LighterHttpClient,
    common::credential::LighterCredentials,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create credentials
    let credentials = LighterCredentials::new(
        "your_api_key_private_key".to_string(),
        "your_eth_private_key".to_string(),
        2,  // API key index (2-254)
        1,  // Account index
    )?;

    // Create authenticated client
    let client = LighterHttpClient::new(
        None,
        None,
        false,
        Some(credentials),
    );

    // Fetch account info
    let account = client.request_account(Some(1)).await?;
    println!("Account: {:?}", account);

    // Get nonce for transactions
    let nonce = client.get_next_nonce().await?;
    println!("Next nonce: {}", nonce);

    Ok(())
}
```

#### WebSocket Client (Real-time Data)

```rust
use nautilus_lighter2::{
    websocket::LighterWebSocketClient,
    common::enums::LighterWsChannel,
};
use futures_util::StreamExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create WebSocket client
    let client = LighterWebSocketClient::new(
        None, None, false, None,
    );

    // Subscribe to channels
    client.subscribe(LighterWsChannel::OrderBook { market_id: 0 }).await?;
    client.subscribe(LighterWsChannel::Trades { market_id: 0 }).await?;

    // Connect and stream messages
    let mut stream = client.connect().await?;

    while let Some(result) = stream.next().await {
        match result {
            Ok(message) => println!("Received: {:?}", message),
            Err(e) => eprintln!("Error: {}", e),
        }
    }

    Ok(())
}
```

### Python Examples

#### Basic Setup

```python
from nautilus_trader.adapters.lighter2 import (
    LighterDataClientConfig,
    LighterExecClientConfig,
    LighterInstrumentProvider,
)
from nautilus_trader.live.node import TradingNode

# Configure data client
data_config = LighterDataClientConfig(
    api_key_private_key="your_api_key",
    eth_private_key="your_eth_key",
    api_key_index=2,
    account_index=1,
    is_testnet=False,
)

# Configure execution client
exec_config = LighterExecClientConfig(
    api_key_private_key="your_api_key",
    eth_private_key="your_eth_key",
    api_key_index=2,
    account_index=1,
    is_testnet=False,
)

# Create trading node
node = TradingNode(
    data_clients={
        "LIGHTER": data_config,
    },
    exec_clients={
        "LIGHTER": exec_config,
    },
)

# Start the node
node.start()
```

#### Using the Rust Client Directly

```python
from nautilus_trader.core.nautilus_pyo3.lighter2 import (
    LighterHttpClient,
    LighterWebSocketClient,
)

# Create HTTP client
http_client = LighterHttpClient(
    base_http_url=None,  # Use defaults
    base_ws_url=None,
    is_testnet=False,
    api_key_private_key="your_key",
    eth_private_key="your_eth_key",
    api_key_index=2,
    account_index=1,
)

# Load instruments
instruments = await http_client.load_instruments()
print(f"Loaded {len(instruments)} instruments")

# Get account info
account = await http_client.get_account(account_id=1)
print(f"Account: {account}")

# Create WebSocket client
ws_client = LighterWebSocketClient(
    base_http_url=None,
    base_ws_url=None,
    is_testnet=False,
)

# Subscribe to order book
await ws_client.subscribe_order_book(market_id=0)

# Check subscription count
count = await ws_client.subscription_count()
print(f"Active subscriptions: {count}")
```

## Authentication

The adapter requires two types of authentication:

1. **API Key Private Key**: For REST API access
2. **Ethereum Private Key**: For transaction signing

### Environment Variables

You can set credentials via environment variables:

```bash
export LIGHTER_API_KEY_PRIVATE_KEY="your_api_key_private_key"
export LIGHTER_ETH_PRIVATE_KEY="your_ethereum_private_key"
```

### API Key Index

Lighter supports up to 253 API keys per account (indices 2-254):
- Index 0: Reserved for desktop clients
- Index 1: Reserved for mobile clients
- Indices 2-254: Available for custom applications
- Index 255: Retrieves all API key data

### Account Types

- **Standard**: Fee-less trading (default)
- **Premium**: 0.2 bps maker fee, 2 bps taker fees

## Running Examples

```bash
# HTTP client example (public endpoints)
cargo run --bin lighter2-http-public

# WebSocket client example
cargo run --bin lighter2-ws-data
```

## Testing

```bash
# Run all tests
cargo test

# Run with logging
RUST_LOG=debug cargo test

# Run specific test
cargo test test_credentials_creation
```

## Configuration

### HTTP Client Options
- `base_http_url`: Base URL for REST API (default: mainnet)
- `base_ws_url`: Base URL for WebSocket (default: mainnet)
- `is_testnet`: Connect to testnet instead of mainnet
- `credentials`: Optional authentication credentials

### WebSocket Client Options
- `base_http_url`: HTTP URL (for future use)
- `base_ws_url`: WebSocket URL
- `is_testnet`: Use testnet
- `credentials`: Optional for private channels

## Development

### Project Structure

```
crates/adapters/lighter2/
├── src/
│   ├── lib.rs                 # Library root
│   ├── common/                # Shared utilities
│   │   ├── consts.rs          # Constants
│   │   ├── credential.rs      # Authentication
│   │   ├── enums.rs           # Lighter-specific types
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
cargo build -p nautilus-lighter2

# Release build
cargo build -p nautilus-lighter2 --release

# With Python bindings
cargo build -p nautilus-lighter2 --features python
```

## Resources

- [Lighter Exchange](https://lighter.xyz/)
- [Lighter API Documentation](https://apidocs.lighter.xyz/)
- [Lighter Python SDK](https://github.com/elliottech/lighter-python)
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
