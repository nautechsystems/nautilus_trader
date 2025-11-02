# nautilus-hyperliquid

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-hyperliquid)](https://docs.rs/nautilus-hyperliquid/latest/nautilus-hyperliquid/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-hyperliquid.svg)](https://crates.io/crates/nautilus-hyperliquid)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

A high-performance Hyperliquid DEX adapter for the NautilusTrader algorithmic trading platform, built in Rust with Python bindings.

## Overview

The Hyperliquid adapter provides seamless integration with [Hyperliquid DEX](https://hyperliquid.xyz/), a decentralized perpetual futures exchange built on the Hyperliquid L1 blockchain. This adapter enables:

- **Real-time Market Data**: Live quotes, trades, and order book data streaming via WebSocket
- **Order Execution**: Full trading capabilities including market, limit, and stop orders
- **Position Management**: Real-time position tracking and risk management
- **Account Information**: Portfolio balances, margin, and P&L monitoring
- **Testnet Support**: Full testnet integration for safe development and testing

## Features

### Market Data
- ✅ **Quote Ticks**: Real-time bid/ask price updates
- ✅ **Trade Ticks**: Executed trade data with aggressor side information
- ✅ **Order Book**: Level 2 order book snapshots and deltas
- ✅ **Instrument Data**: Dynamic instrument loading and updates
- ✅ **WebSocket Streaming**: High-performance, low-latency data feeds

### Order Management
- ✅ **Market Orders**: Immediate execution at current market price
- ✅ **Limit Orders**: Execution at specified price or better
- ✅ **Stop Orders**: Stop-loss and take-profit order types
- ✅ **Order Modifications**: Real-time order updates and cancellations
- ✅ **Position Management**: Automated position tracking and updates

### Risk & Compliance
- ✅ **Account Monitoring**: Real-time balance and margin tracking
- ✅ **Position Limits**: Configurable position size controls
- ✅ **Rate Limiting**: Built-in API rate limit compliance
- ✅ **Error Handling**: Comprehensive error recovery and logging

## Architecture

The Hyperliquid adapter follows NautilusTrader's hybrid Rust/Python architecture:

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│   Python API    │◄──►│  Rust Core       │◄──►│  Hyperliquid    │
│   (Trading      │    │  (Performance    │    │  DEX API        │
│    Logic)       │    │   Critical)      │    │                 │
└─────────────────┘    └──────────────────┘    └─────────────────┘
         │                        │                       │
         ▼                        ▼                       ▼
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│ Strategy Engine │    │ WebSocket Client │    │ HTTP Client     │
│ Event System    │    │ Message Parser   │    │ REST API        │
└─────────────────┘    └──────────────────┘    └─────────────────┘
```

## Installation

### Prerequisites
- Rust 1.87.0 or higher
- Python 3.11-3.13
- NautilusTrader development environment

### From Source
```bash
# Clone the repository
git clone https://github.com/nautechsystems/nautilus_trader.git
cd nautilus_trader

# Install dependencies
make install

# Build the Hyperliquid adapter
cd crates/adapters/hyperliquid
cargo build --release
```

## Configuration

### Data Client Configuration

```python
from nautilus_trader.adapters.hyperliquid2 import HyperliquidDataClientConfig
from nautilus_trader.config import InstrumentProviderConfig

# Public market data (no authentication required)
config = HyperliquidDataClientConfig(
    testnet=True,  # Use testnet for development
    instrument_provider=InstrumentProviderConfig(load_all=True),
    http_timeout_secs=60,
    update_instruments_interval_mins=60,
)

# Private data (requires authentication)
config = HyperliquidDataClientConfig(
    private_key="your_private_key_here",
    wallet_address="your_wallet_address_here", 
    testnet=True,
    instrument_provider=InstrumentProviderConfig(load_all=True),
)
```

### Execution Client Configuration

```python
from nautilus_trader.adapters.hyperliquid2 import HyperliquidExecClientConfig

config = HyperliquidExecClientConfig(
    private_key="your_private_key_here",
    wallet_address="your_wallet_address_here",
    testnet=True,  # Always start with testnet
    http_timeout_secs=60,
)
```

### Environment Variables

For enhanced security, use environment variables for credentials:

```bash
export HYPERLIQUID_PRIVATE_KEY="your_private_key_here"
export HYPERLIQUID_WALLET_ADDRESS="your_wallet_address_here"
```

## Quick Start

### Live Data Subscription

```python
#!/usr/bin/env python3

from nautilus_trader.adapters.hyperliquid2 import HYPERLIQUID
from nautilus_trader.adapters.hyperliquid2 import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid2 import HyperliquidLiveDataClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import TraderId

# Configure trading node
config = TradingNodeConfig(
    trader_id=TraderId("HYPERLIQUID_TRADER-001"),
    logging=LoggingConfig(log_level="INFO"),
    data_clients={
        HYPERLIQUID: HyperliquidDataClientConfig(
            testnet=True,
            instrument_provider=InstrumentProviderConfig(load_all=True),
        )
    },
)

# Create and run node
node = TradingNode(config=config)
node.add_data_client_factory(HYPERLIQUID, HyperliquidLiveDataClientFactory)
node.build()

try:
    node.run()
except KeyboardInterrupt:
    pass
finally:
    node.dispose()
```

### Basic Trading Strategy

```python
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.trading.strategy import Strategy
from nautilus_trader.model.data import QuoteTick, TradeTick

class HyperliquidStrategy(Strategy):
    def __init__(self, config):
        super().__init__(config)
        
        # Subscribe to BTC-PERP
        self.instrument_id = InstrumentId.from_str("BTC-PERP.HYPERLIQUID")
    
    def on_start(self):
        # Subscribe to market data
        self.subscribe_quote_ticks(self.instrument_id)
        self.subscribe_trade_ticks(self.instrument_id)
        
    def on_quote_tick(self, tick: QuoteTick):
        # Handle real-time quotes
        self.log.info(f"Quote: {tick.instrument_id} | "
                     f"Bid: ${tick.bid_price.as_double():.2f} | "
                     f"Ask: ${tick.ask_price.as_double():.2f}")
        
    def on_trade_tick(self, tick: TradeTick):
        # Handle real-time trades
        self.log.info(f"Trade: {tick.instrument_id} | "
                     f"Price: ${tick.price.as_double():.2f} | "
                     f"Size: {tick.size.as_double():.6f}")
```

## Available Instruments

The Hyperliquid adapter supports all perpetual futures available on the platform:

### Major Cryptocurrencies
- `BTC-PERP.HYPERLIQUID` - Bitcoin Perpetual
- `ETH-PERP.HYPERLIQUID` - Ethereum Perpetual  
- `SOL-PERP.HYPERLIQUID` - Solana Perpetual

### Altcoins
- `ARB-PERP.HYPERLIQUID` - Arbitrum Perpetual
- `OP-PERP.HYPERLIQUID` - Optimism Perpetual
- `AVAX-PERP.HYPERLIQUID` - Avalanche Perpetual
- And many more...

Instruments are automatically loaded and updated from the Hyperliquid API.

## WebSocket API Integration

The adapter uses Hyperliquid's WebSocket API for real-time data:

### Supported Channels
- **`allMids`** - All instrument mid-prices
- **`l2Book`** - Level 2 order book data
- **`trades`** - Real-time trade executions
- **`userOrders`** - Private user order updates
- **`userFills`** - Private user trade fills

### Connection Management
- Automatic reconnection with exponential backoff
- Heartbeat monitoring and connection health checks
- Message queue handling during reconnections
- Rate limit compliance and throttling

## HTTP API Integration

REST API integration for:
- Account information and balances
- Order placement and management
- Position data and P&L
- Historical data requests
- Instrument metadata

## Examples

The adapter includes comprehensive examples:

### Rust Examples
- **`websocket_example.rs`** - WebSocket client demonstration
- **`trading_example.rs`** - Complete trading workflow

### Python Examples
- **`hyperliquid_live_data_subscriber.py`** - Market data streaming
- **`hyperliquid_execution_tester.py`** - Order execution testing
- **`binance_hyperliquid_live_data_subscriber.py`** - Cross-exchange comparison

### Running Examples

```bash
# Rust WebSocket example
cd crates/adapters/hyperliquid
cargo run --example websocket_example

# Python data subscriber
cd examples/live/hyperliquid
python hyperliquid_live_data_subscriber.py
```

## Testing

### Unit Tests
```bash
cd crates/adapters/hyperliquid
cargo test
```

### Integration Tests
```bash
# Test with live testnet connection
cargo test --features testnet -- --ignored
```

### Python Tests
```bash
# From nautilus_trader root
make pytest
```

## Performance

The Hyperliquid adapter is optimized for high-performance trading:

- **WebSocket Latency**: Sub-millisecond message processing
- **Memory Usage**: Minimal heap allocations in hot paths  
- **CPU Usage**: Efficient message parsing with zero-copy deserialization
- **Throughput**: Handles thousands of messages per second

### Benchmarks
```bash
cd crates/adapters/hyperliquid
cargo bench
```

## Security

### Best Practices
- **Testnet First**: Always test strategies on testnet before mainnet
- **Environment Variables**: Store private keys securely
- **Network Security**: Use secure connections (WSS/HTTPS only)
- **Error Handling**: Comprehensive error logging and monitoring

### Private Key Management
```python
# ✅ Good - Use environment variables
config = HyperliquidExecClientConfig(
    testnet=True,
    # private_key loaded from HYPERLIQUID_PRIVATE_KEY env var
)

# ❌ Bad - Hardcoded credentials
config = HyperliquidExecClientConfig(
    private_key="0x1234...",  # Never do this!
    testnet=True,
)
```

## Troubleshooting

### Common Issues

#### Connection Failures
```
Error: Failed to connect to WebSocket
```
**Solution**: Check network connectivity and ensure testnet parameter matches your intention.

#### Authentication Errors
```
Error: Invalid signature
```
**Solution**: Verify private key and wallet address are correct and properly formatted.

#### Rate Limiting
```
Error: Rate limit exceeded
```
**Solution**: Implement proper request throttling and respect API limits.

#### Instrument Not Found
```
Error: Unknown instrument BTC-PERP.HYPERLIQUID
```
**Solution**: Ensure instrument provider is configured to load instruments: `load_all=True`.

### Debug Mode
Enable debug logging for troubleshooting:

```python
config = TradingNodeConfig(
    logging=LoggingConfig(log_level="DEBUG"),
    # ... other config
)
```

### Log Analysis
Key log patterns to monitor:
- `Connected to Hyperliquid WebSocket` - Successful connection
- `Received message type: l2Book` - Market data flowing
- `Order filled` - Trade execution confirmation
- `Reconnection attempt` - Connection recovery

## API Limits

Hyperliquid API limits (as of 2025):
- **WebSocket**: 100 connections per IP
- **HTTP REST**: 10 requests/second per API key
- **Order Placement**: 20 orders/second
- **Order Cancellation**: 50 cancellations/second

The adapter automatically handles rate limiting and queuing.

## Contributing

We welcome contributions to improve the Hyperliquid adapter:

1. **Fork the repository**
2. **Create a feature branch**: `git checkout -b feature/my-improvement` 
3. **Make changes**: Follow existing code patterns and add tests
4. **Run tests**: `cargo test && make pytest`
5. **Submit PR**: Create a pull request with clear description

### Development Setup
```bash
# Install development dependencies
make install-debug

# Run pre-commit hooks
make pre-commit

# Format code
make format

# Run linting
make clippy
```

## Platform

[NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
algorithmic trading platform, providing quantitative traders with the ability to backtest
portfolios of automated trading strategies on historical data with an event-driven engine,
and also deploy those same strategies live, with no code changes.

## Documentation

See [the docs](https://docs.rs/nautilus-hyperliquid) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
Contributions to the project are welcome and require the completion of a standard [Contributor License Agreement (CLA)](https://github.com/nautechsystems/nautilus_trader/blob/develop/CLA.md).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

<img src="https://nautilustrader.io/nautilus-logo-white.png" alt="logo" width="400" height="auto"/>

<span style="font-size: 0.8em; color: #999;">© 2015-2025 Nautech Systems Pty Ltd. All rights reserved.</span>
