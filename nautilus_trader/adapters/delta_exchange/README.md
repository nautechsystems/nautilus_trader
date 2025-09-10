# Delta Exchange Adapter for Nautilus Trader

The Delta Exchange adapter provides comprehensive integration with Delta Exchange, a leading derivatives trading platform that offers perpetual futures and options trading. This adapter enables both manual and algorithmic trading through Nautilus Trader's unified API.

## Features

### Trading Capabilities
- **Perpetual Futures**: Full support for cryptocurrency perpetual futures contracts
- **Options Trading**: Complete options trading with calls and puts
- **Advanced Order Types**: Market, limit, stop-loss, and take-profit orders
- **Risk Management**: Position limits, daily loss limits, and Market Maker Protection (MMP)
- **Portfolio Margins**: Support for Delta Exchange's portfolio margin system

### Market Data
- **Real-time Feeds**: Live ticker, order book, and trade data via WebSocket
- **Historical Data**: Access to historical price and volume data
- **Mark Prices**: Real-time mark price updates for derivatives
- **Funding Rates**: Live funding rate data for perpetual contracts
- **Index Prices**: Underlying index price feeds

### Technical Features
- **High Performance**: Rust-based HTTP and WebSocket clients for optimal performance
- **Automatic Reconnection**: Robust connection management with exponential backoff
- **Rate Limiting**: Built-in rate limiting compliance with Delta Exchange API limits
- **Caching**: Intelligent caching for instruments and configuration data
- **Error Handling**: Comprehensive error handling and recovery mechanisms

## Installation

### Prerequisites
- Python 3.10 or higher
- Nautilus Trader 1.200.0 or higher
- Delta Exchange API credentials

### Install Dependencies
```bash
pip install nautilus_trader[delta_exchange]
```

### Rust Components
The adapter includes pre-compiled Rust bindings. If you need to build from source:
```bash
# Install Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build Rust components
cd nautilus_trader/adapters/delta_exchange
cargo build --release
```

## Quick Start

### 1. API Credentials
Obtain API credentials from Delta Exchange:
1. Log in to your Delta Exchange account
2. Navigate to API Management
3. Create a new API key with appropriate permissions
4. Note your API key and secret

### 2. Environment Variables
Set your credentials as environment variables:
```bash
export DELTA_EXCHANGE_API_KEY="your_api_key"
export DELTA_EXCHANGE_API_SECRET="your_api_secret"
```

### 3. Basic Configuration
```python
from nautilus_trader.adapters.delta_exchange import (
    DeltaExchangeDataClientConfig,
    DeltaExchangeExecClientConfig,
    DeltaExchangeLiveDataClientFactory,
    DeltaExchangeLiveExecClientFactory,
)

# Data client configuration
data_config = DeltaExchangeDataClientConfig(
    api_key="your_api_key",
    api_secret="your_api_secret",
    testnet=True,  # Use testnet for testing
    enable_private_channels=True,
    product_types=["perpetual_futures"],
    symbol_patterns=["BTC*", "ETH*"],
)

# Execution client configuration
exec_config = DeltaExchangeExecClientConfig(
    api_key="your_api_key",
    api_secret="your_api_secret",
    account_id="your_account_id",
    testnet=True,
    position_limits={"BTCUSDT": Decimal("10.0")},
    daily_loss_limit=Decimal("1000.0"),
)
```

### 4. Create Trading Node
```python
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode

# Create trading node configuration
config = TradingNodeConfig(
    trader_id="TRADER-001",
    data_engine={
        "data_clients": {
            "DELTA_EXCHANGE": data_config,
        }
    },
    exec_engine={
        "exec_clients": {
            "DELTA_EXCHANGE": exec_config,
        }
    },
)

# Create and start trading node
node = TradingNode(config=config)
node.build()
node.start()
```

## Configuration

### Data Client Configuration
```python
data_config = DeltaExchangeDataClientConfig(
    # Authentication
    api_key="your_api_key",
    api_secret="your_api_secret",
    
    # Environment
    testnet=False,  # Set to True for testnet
    sandbox=False,  # Set to True for sandbox
    
    # Data subscriptions
    enable_private_channels=True,
    product_types=["perpetual_futures", "call_options", "put_options"],
    symbol_patterns=["*"],  # Subscribe to all symbols
    
    # Performance tuning
    request_timeout_secs=60.0,
    ws_timeout_secs=10.0,
    max_retries=3,
    retry_delay_secs=1.0,
    heartbeat_interval_secs=30.0,
    
    # Reconnection settings
    max_reconnect_attempts=10,
    reconnect_delay_secs=5.0,
)
```

### Execution Client Configuration
```python
exec_config = DeltaExchangeExecClientConfig(
    # Authentication
    api_key="your_api_key",
    api_secret="your_api_secret",
    account_id="your_account_id",
    
    # Environment
    testnet=False,
    sandbox=False,
    
    # Risk management
    position_limits={
        "BTCUSDT": Decimal("10.0"),
        "ETHUSDT": Decimal("100.0"),
    },
    daily_loss_limit=Decimal("5000.0"),
    max_position_value=Decimal("100000.0"),
    
    # Market Maker Protection
    enable_mmp=True,
    mmp_delta_limit=Decimal("1000.0"),
    mmp_vega_limit=Decimal("10000.0"),
    mmp_frozen_time_secs=5,
    
    # Performance settings
    request_timeout_secs=60.0,
    max_retries=3,
    retry_delay_secs=1.0,
)
```

## Trading Examples

### Market Data Subscription
```python
import asyncio
from nautilus_trader.model.identifiers import InstrumentId, Symbol
from nautilus_trader.adapters.delta_exchange.constants import DELTA_EXCHANGE_VENUE

async def subscribe_to_market_data():
    # Create data client
    data_client = DeltaExchangeLiveDataClientFactory.create(
        loop=asyncio.get_event_loop(),
        name="DeltaExchange-Data",
        config=data_config,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    
    # Connect and subscribe
    await data_client.connect()
    
    # Subscribe to BTC perpetual
    btc_instrument = InstrumentId(Symbol("BTCUSDT"), DELTA_EXCHANGE_VENUE)
    await data_client.subscribe_quote_ticks(btc_instrument)
    await data_client.subscribe_trade_ticks(btc_instrument)
    await data_client.subscribe_order_book_deltas(btc_instrument)
```

### Order Submission
```python
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Quantity

async def submit_market_order():
    # Create execution client
    exec_client = DeltaExchangeLiveExecClientFactory.create(
        loop=asyncio.get_event_loop(),
        name="DeltaExchange-Exec",
        config=exec_config,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    
    # Connect
    await exec_client.connect()
    
    # Create market order
    order = MarketOrder(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=btc_instrument,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.1"),
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
    )
    
    # Submit order
    await exec_client.submit_order(order)
```

### Algorithmic Trading Strategy
```python
from nautilus_trader.trading.strategy import Strategy

class DeltaExchangeStrategy(Strategy):
    def __init__(self):
        super().__init__()
        self.instrument_id = InstrumentId(Symbol("BTCUSDT"), DELTA_EXCHANGE_VENUE)
    
    def on_start(self):
        # Subscribe to market data
        self.subscribe_quote_ticks(self.instrument_id)
        self.subscribe_trade_ticks(self.instrument_id)
    
    def on_quote_tick(self, tick: QuoteTick):
        # Implement trading logic
        if self.should_buy(tick):
            self.buy_market(
                instrument_id=self.instrument_id,
                quantity=Quantity.from_str("0.1"),
            )
    
    def should_buy(self, tick: QuoteTick) -> bool:
        # Implement your trading logic here
        return tick.bid_price < 50000.0
```

## Environment Configuration

### Production Environment
```python
config = DeltaExchangeDataClientConfig(
    api_key="your_production_api_key",
    api_secret="your_production_api_secret",
    testnet=False,
    sandbox=False,
    # ... other settings
)
```

### Testnet Environment
```python
config = DeltaExchangeDataClientConfig.testnet(
    api_key="your_testnet_api_key",
    api_secret="your_testnet_api_secret",
    # ... other settings
)
```

### Sandbox Environment
```python
config = DeltaExchangeDataClientConfig.sandbox(
    api_key="your_sandbox_api_key",
    api_secret="your_sandbox_api_secret",
    # ... other settings
)
```

## Risk Management

### Position Limits
```python
exec_config = DeltaExchangeExecClientConfig(
    # ... authentication
    position_limits={
        "BTCUSDT": Decimal("10.0"),    # Max 10 BTC position
        "ETHUSDT": Decimal("100.0"),   # Max 100 ETH position
        "SOLUSDT": Decimal("1000.0"),  # Max 1000 SOL position
    },
    daily_loss_limit=Decimal("5000.0"),  # Max $5000 daily loss
    max_position_value=Decimal("100000.0"),  # Max $100k total position value
)
```

### Market Maker Protection (MMP)
```python
exec_config = DeltaExchangeExecClientConfig(
    # ... authentication
    enable_mmp=True,
    mmp_delta_limit=Decimal("1000.0"),    # Delta limit
    mmp_vega_limit=Decimal("10000.0"),    # Vega limit
    mmp_frozen_time_secs=5,               # Freeze time after trigger
)
```

## Performance Optimization

### Connection Pooling
The adapter automatically manages connection pooling for optimal performance:
- HTTP client connection reuse
- WebSocket connection management
- Automatic reconnection with exponential backoff

### Caching
Intelligent caching reduces API calls:
- Instrument metadata caching
- Configuration caching
- Rate limit tracking

### Rate Limiting
Built-in rate limiting ensures compliance:
- Automatic request throttling
- Queue management for burst requests
- Backoff strategies for rate limit violations

## Monitoring and Logging

### Enable Debug Logging
```python
import logging

# Enable debug logging for Delta Exchange adapter
logging.getLogger("nautilus_trader.adapters.delta_exchange").setLevel(logging.DEBUG)
```

### Performance Metrics
The adapter provides comprehensive performance metrics:
- Request/response latencies
- WebSocket message throughput
- Error rates and types
- Connection stability metrics

### Health Checks
Built-in health checks monitor:
- API connectivity
- WebSocket connection status
- Authentication status
- Rate limit status

## Troubleshooting

### Common Issues

#### Authentication Errors
```
Error: 401 Unauthorized
```
**Solution**: Verify your API credentials and ensure they have the required permissions.

#### Rate Limiting
```
Error: 429 Too Many Requests
```
**Solution**: The adapter handles rate limiting automatically. If you see this error, reduce your request frequency.

#### Connection Issues
```
Error: WebSocket connection failed
```
**Solution**: Check your internet connection and firewall settings. The adapter will automatically reconnect.

#### Invalid Symbols
```
Error: Product not found
```
**Solution**: Verify the symbol exists on Delta Exchange and is active for trading.

### Debug Mode
Enable debug mode for detailed logging:
```python
config = DeltaExchangeDataClientConfig(
    # ... other settings
    debug=True,
)
```

### Support
For additional support:
- Check the [Nautilus Trader documentation](https://docs.nautilustrader.io)
- Visit the [Delta Exchange API documentation](https://docs.delta.exchange)
- Join the [Nautilus Trader Discord](https://discord.gg/nautilus-trader)

## API Rate Limits

Delta Exchange enforces the following rate limits:
- **REST API**: 100 requests per second
- **WebSocket**: 150 connections per IP
- **Order Submission**: 50 orders per second

The adapter automatically manages these limits with:
- Request queuing and throttling
- Automatic backoff on rate limit violations
- Connection pooling for optimal resource usage

## Security Best Practices

1. **API Key Security**:
   - Never hardcode API keys in your code
   - Use environment variables or secure key management
   - Regularly rotate your API keys

2. **Network Security**:
   - Use HTTPS/WSS connections (enforced by adapter)
   - Consider using VPN for additional security
   - Monitor for unusual API activity

3. **Risk Management**:
   - Always set position limits
   - Use stop-loss orders for risk control
   - Monitor your positions regularly
   - Test strategies on testnet first

## License

This adapter is part of Nautilus Trader and is licensed under the GNU Lesser General Public License Version 3.0 (LGPL-3.0).

## Migration Guide

### From Other Exchange Adapters

If you're migrating from another exchange adapter, here are the key differences:

#### From Binance
- Delta Exchange focuses on derivatives (perpetuals and options)
- Different symbol naming conventions (BTCUSDT vs BTCUSD_PERP)
- Portfolio margin system instead of isolated margins
- Market Maker Protection (MMP) features

#### From OKX
- Similar derivatives focus but different API structure
- Delta Exchange uses product IDs for instrument identification
- Different order types and time-in-force options
- Unique funding rate calculation methods

#### Configuration Migration
```python
# Old Binance config
binance_config = BinanceDataClientConfig(
    api_key="key",
    api_secret="secret",
    testnet=True,
)

# New Delta Exchange config
delta_config = DeltaExchangeDataClientConfig(
    api_key="key",
    api_secret="secret",
    testnet=True,
    product_types=["perpetual_futures"],  # Specify product types
    enable_private_channels=True,         # Enable private data
)
```

### Symbol Mapping
Common symbol mappings between exchanges:

| Binance | OKX | Delta Exchange |
|---------|-----|----------------|
| BTCUSDT | BTC-USDT-SWAP | BTCUSDT |
| ETHUSDT | ETH-USDT-SWAP | ETHUSDT |
| ADAUSDT | ADA-USDT-SWAP | ADAUSDT |

## Advanced Configuration

### Custom Instrument Filtering
```python
config = DeltaExchangeDataClientConfig(
    # ... authentication
    product_types=["perpetual_futures", "call_options"],
    symbol_patterns=[
        "BTC*",     # All BTC instruments
        "ETH*",     # All ETH instruments
        "*USDT",    # All USDT-settled instruments
    ],
    exclude_patterns=[
        "*_OLD",    # Exclude old contracts
        "TEST*",    # Exclude test instruments
    ],
)
```

### Advanced Risk Management
```python
exec_config = DeltaExchangeExecClientConfig(
    # ... authentication

    # Position limits by instrument type
    position_limits={
        "BTCUSDT": Decimal("10.0"),
        "ETHUSDT": Decimal("100.0"),
        "BTC_CALL_*": Decimal("5.0"),   # Pattern-based limits
        "ETH_PUT_*": Decimal("50.0"),
    },

    # Time-based limits
    daily_loss_limit=Decimal("5000.0"),
    hourly_loss_limit=Decimal("1000.0"),

    # Portfolio-level limits
    max_position_value=Decimal("100000.0"),
    max_leverage=Decimal("10.0"),

    # Order-level limits
    max_order_size=Decimal("1.0"),
    max_orders_per_minute=50,
)
```

### Performance Tuning
```python
config = DeltaExchangeDataClientConfig(
    # ... authentication

    # Connection settings
    request_timeout_secs=30.0,
    ws_timeout_secs=10.0,
    heartbeat_interval_secs=30.0,

    # Retry settings
    max_retries=5,
    retry_delay_secs=1.0,
    exponential_backoff=True,

    # Reconnection settings
    max_reconnect_attempts=10,
    reconnect_delay_secs=5.0,
    reconnect_exponential_backoff=True,

    # Performance settings
    enable_message_compression=True,
    max_message_queue_size=10000,
    enable_batch_processing=True,
)
```

## Testing

### Unit Tests
Run the unit test suite:
```bash
pytest tests/unit_tests/adapters/delta_exchange/ -v
```

### Integration Tests
Run integration tests (requires API credentials):
```bash
export DELTA_EXCHANGE_API_KEY="your_testnet_key"
export DELTA_EXCHANGE_API_SECRET="your_testnet_secret"
pytest tests/integration_tests/adapters/delta_exchange/ -v
```

### Performance Tests
Run performance benchmarks:
```bash
pytest tests/performance/adapters/delta_exchange/ -v --benchmark
```

## Contributing

Contributions are welcome! Please see the [Nautilus Trader contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md) for details.

### Development Setup
1. Fork the repository
2. Create a feature branch
3. Install development dependencies:
   ```bash
   pip install -e ".[dev]"
   ```
4. Run tests to ensure everything works
5. Make your changes
6. Add tests for new functionality
7. Submit a pull request

### Code Style
- Follow PEP 8 for Python code
- Use type hints for all function signatures
- Add docstrings for all public methods
- Ensure all tests pass before submitting
