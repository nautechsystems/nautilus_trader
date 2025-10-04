# Coinbase Advanced Trade Adapter (US)

This adapter provides integration with [Coinbase Advanced Trade API](https://docs.cdp.coinbase.com/advanced-trade/docs/welcome) for NautilusTrader.

⚠️ **Note**: This adapter is for **Coinbase Advanced Trade** (US and select regions), not Coinbase Pro, Coinbase International, or Coinbase Prime.

## Features

- **Spot Trading**: Trade cryptocurrency spot pairs (BTC-USD, ETH-USD, etc.)
- **Real-time Market Data**: WebSocket streams for tickers, order books, and trades
- **Order Management**: Submit, cancel, and track orders
- **Account Management**: Query balances and account information

## Prerequisites

- **Rust 1.90.0 or higher** (required for building the adapter)
- **Python 3.11-3.13**
- **Coinbase Account** with API credentials

## Installation

### 1. Update Rust (if needed)

The adapter requires Rust 1.90.0 or higher. Check your version:

```bash
rustc --version
```

If you need to update:

```bash
# Using rustup (recommended)
rustup update

# Or using Homebrew on macOS
brew upgrade rust
```

### 2. Build the Adapter

From the NautilusTrader root directory:

```bash
# Build the Rust crate
cargo build -p nautilus-coinbase --release

# Build the Python extension
poetry install
```

## Configuration

### API Credentials

You'll need to create API credentials from your Coinbase account:

1. Go to [Coinbase Advanced Trade](https://www.coinbase.com/settings/api)
2. Create a new API key with the following permissions:
   - **View** (for market data and account info)
   - **Trade** (for order placement and management)
3. Save your API Key and API Secret securely

### Environment Variables

Set your credentials as environment variables:

```bash
export COINBASE_API_KEY="your_api_key_here"
export COINBASE_API_SECRET="your_api_secret_here"
```

Or add them to your `.env` file:

```
COINBASE_API_KEY=your_api_key_here
COINBASE_API_SECRET=your_api_secret_here
```

## Usage

### Basic Example

```python
from nautilus_trader.adapters.coinbase import CoinbaseDataClientConfig
from nautilus_trader.adapters.coinbase import CoinbaseExecClientConfig
from nautilus_trader.adapters.coinbase import CoinbaseLiveDataClientFactory
from nautilus_trader.adapters.coinbase import CoinbaseLiveExecClientFactory
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode

# Configure data client
data_config = CoinbaseDataClientConfig(
    api_key=None,  # Will use COINBASE_API_KEY env var
    api_secret=None,  # Will use COINBASE_API_SECRET env var
)

# Configure execution client
exec_config = CoinbaseExecClientConfig(
    api_key=None,  # Will use COINBASE_API_KEY env var
    api_secret=None,  # Will use COINBASE_API_SECRET env var
)

# Create trading node configuration
config = TradingNodeConfig(
    data_clients={
        "COINBASE": data_config,
    },
    exec_clients={
        "COINBASE": exec_config,
    },
)

# Create and start the trading node
node = TradingNode(config=config)
node.start()
```

### Subscribe to Market Data

```python
from nautilus_trader.model.identifiers import InstrumentId

# Subscribe to trades
instrument_id = InstrumentId.from_str("BTC/USD.COINBASE")
node.subscribe_trade_ticks(instrument_id)

# Subscribe to order book
node.subscribe_order_book_deltas(instrument_id)

# Subscribe to quotes
node.subscribe_quote_ticks(instrument_id)
```

### Place Orders

```python
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Price, Quantity
from nautilus_trader.model.orders import LimitOrder

# Create a limit order
order = LimitOrder(
    trader_id=node.trader_id,
    strategy_id=strategy_id,
    instrument_id=InstrumentId.from_str("BTC/USD.COINBASE"),
    client_order_id=ClientOrderId("O-123456"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_str("0.001"),
    price=Price.from_str("50000.00"),
    time_in_force=TimeInForce.GTC,
)

# Submit the order
node.submit_order(order)
```

## Configuration Options

### Data Client Config

- `venue`: Venue identifier (default: `COINBASE`)
- `api_key`: API key (optional, uses env var if not provided)
- `api_secret`: API secret (optional, uses env var if not provided)
- `base_url_http`: HTTP API base URL (optional, defaults to production)
- `base_url_ws`: WebSocket URL (optional, defaults to production)
- `http_timeout_secs`: HTTP request timeout in seconds (default: 60)
- `update_instruments_interval_mins`: Instrument update interval (optional)

### Execution Client Config

Same as Data Client Config.

## Supported Order Types

- **Market Orders**: Execute immediately at best available price
- **Limit Orders**: Execute at specified price or better
  - Supports `post_only` flag for maker-only orders

## Supported Instruments

All spot trading pairs available on Coinbase Advanced Trade, including:

- BTC/USD, BTC/EUR, BTC/GBP
- ETH/USD, ETH/EUR, ETH/BTC
- And many more cryptocurrency pairs

## WebSocket Channels

The adapter subscribes to the following WebSocket channels:

- **ticker**: Real-time price updates
- **level2**: Order book updates
- **market_trades**: Trade executions
- **user**: Order and fill updates (authenticated)
- **heartbeats**: Connection health monitoring

## API Rate Limits

Coinbase enforces rate limits on API requests. The adapter handles these automatically, but be aware:

- **Public endpoints**: ~10 requests per second
- **Private endpoints**: ~15 requests per second

## Troubleshooting

### Authentication Errors

If you see authentication errors:

1. Verify your API credentials are correct
2. Ensure your API key has the required permissions
3. Check that your system clock is synchronized (authentication uses timestamps)

### Connection Issues

If WebSocket connections fail:

1. Check your internet connection
2. Verify firewall settings allow WebSocket connections
3. Check Coinbase API status at [status.coinbase.com](https://status.coinbase.com)

### Build Errors

If you encounter build errors:

1. Ensure Rust 1.90.0+ is installed: `rustc --version`
2. Update dependencies: `cargo update`
3. Clean and rebuild: `cargo clean && cargo build`

## API Documentation

- [Coinbase Advanced Trade API](https://docs.cdp.coinbase.com/advanced-trade/docs/welcome)
- [REST API Reference](https://docs.cdp.coinbase.com/advanced-trade/reference)
- [WebSocket Documentation](https://docs.cdp.coinbase.com/advanced-trade/docs/websocket-overview)
- [Authentication Guide](https://docs.cdp.coinbase.com/advanced-trade/docs/rest-api-auth)

## Support

For issues specific to this adapter, please open an issue on the [NautilusTrader GitHub repository](https://github.com/nautechsystems/nautilus_trader/issues).

For Coinbase API issues, contact [Coinbase Support](https://help.coinbase.com/).

## License

This adapter is part of NautilusTrader and is licensed under the LGPL-3.0 license.

