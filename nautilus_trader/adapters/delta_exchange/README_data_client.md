# Delta Exchange Data Client

The `DeltaExchangeDataClient` provides comprehensive market data functionality for Delta Exchange, including real-time WebSocket subscriptions, historical data requests, and proper integration with the Nautilus Trader framework.

## Features

### Real-time Data Subscriptions
- **Quote Ticks**: Best bid/ask prices via `v2_ticker` channel
- **Trade Ticks**: All executed trades via `all_trades` channel  
- **Order Book**: Level 2 order book snapshots and updates via `l2_orderbook` and `l2_updates` channels
- **Mark Prices**: Mark price updates for derivatives via `mark_price` channel
- **Funding Rates**: Funding rate updates for perpetual futures via `funding_rate` channel
- **Bars/Candlesticks**: OHLCV data via `candlesticks` channel

### Historical Data Requests
- **Trade History**: Historical trade data with pagination support
- **Candlestick History**: Historical OHLCV data with multiple timeframes
- **Rate Limiting**: Automatic compliance with Delta Exchange API limits

### Connection Management
- **Automatic Reconnection**: Exponential backoff with configurable retry attempts
- **Health Monitoring**: Periodic health checks with ping/pong
- **Authentication**: Secure HMAC-SHA256 authentication for private channels
- **Error Handling**: Comprehensive error handling with graceful degradation

### Configuration Features
- **Environment Support**: Production, testnet, and sandbox environments
- **Symbol Filtering**: Glob pattern-based symbol filtering (`BTC*`, `ETH*`, etc.)
- **Default Channels**: Automatic subscription to configured channels on connect
- **Timeout Management**: Configurable WebSocket and HTTP timeouts

## Usage Examples

### Basic Usage

```python
import asyncio
from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeDataClientConfig
from nautilus_trader.adapters.delta_exchange.data import DeltaExchangeDataClient

async def main():
    # Create configuration
    config = DeltaExchangeDataClientConfig.testnet(
        api_key="your_testnet_api_key",
        api_secret="your_testnet_api_secret",
        default_channels=["v2_ticker", "all_trades"],
        symbol_filters=["BTC*", "ETH*"],
    )
    
    # Create and connect data client
    data_client = DeltaExchangeDataClient(
        loop=asyncio.get_event_loop(),
        client=http_client,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        instrument_provider=instrument_provider,
        config=config,
    )
    
    await data_client._connect()
    
    # Subscribe to market data
    await data_client._subscribe_quote_ticks(instrument_id)
    await data_client._subscribe_trade_ticks(instrument_id)
    
    # Request historical data
    await data_client._request_trade_ticks(
        instrument_id=instrument_id,
        limit=100,
        correlation_id=correlation_id,
    )

asyncio.run(main())
```

### Advanced Configuration

```python
# Production configuration with advanced settings
config = DeltaExchangeDataClientConfig(
    api_key="your_api_key",
    api_secret="your_api_secret",
    testnet=False,
    default_channels=["v2_ticker", "mark_price", "funding_rate"],
    symbol_filters=["BTC*", "ETH*", "SOL*"],
    ws_timeout_secs=60,
    heartbeat_interval_secs=30,
    max_reconnection_attempts=10,
    reconnection_delay_secs=5.0,
    auto_reconnect=True,
)
```

## Configuration Parameters

### Core Settings
- `api_key`: Delta Exchange API key
- `api_secret`: Delta Exchange API secret  
- `testnet`: Use testnet environment (default: False)
- `sandbox`: Use sandbox environment (default: False)

### WebSocket Settings
- `ws_timeout_secs`: WebSocket timeout in seconds (default: 30)
- `heartbeat_interval_secs`: Heartbeat interval in seconds (default: 20)
- `max_reconnection_attempts`: Maximum reconnection attempts (default: 5)
- `reconnection_delay_secs`: Delay between reconnection attempts (default: 2.0)
- `auto_reconnect`: Enable automatic reconnection (default: True)
- `max_queue_size`: Maximum message queue size (default: 10000)

### Subscription Settings
- `default_channels`: Channels to subscribe to on connect
- `symbol_filters`: Glob patterns for symbol filtering
- `subscribe_to_public_channels`: Subscribe to public channels (default: True)

### HTTP Settings
- `http_timeout_secs`: HTTP request timeout in seconds (default: 30)
- `max_retries`: Maximum HTTP request retries (default: 3)
- `retry_delay_secs`: Delay between HTTP retries (default: 1.0)

## Supported Data Types

### Market Data
- `QuoteTick`: Best bid/ask prices with timestamps
- `TradeTick`: Individual trade executions with price, size, and side
- `OrderBookSnapshot`: Full order book state
- `OrderBookDeltas`: Incremental order book updates
- `Bar`: OHLCV candlestick data
- `MarkPriceUpdate`: Mark price updates for derivatives
- `FundingRateUpdate`: Funding rate updates for perpetual futures

### Timeframes (Bars)
- 1m, 5m, 15m, 30m (minutes)
- 1h, 2h, 4h, 6h, 12h (hours)  
- 1d (daily)
- 1w (weekly)

## Error Handling

The data client provides comprehensive error handling:

### Connection Errors
- Automatic reconnection with exponential backoff
- Connection state tracking and health monitoring
- Graceful degradation on connection failures

### API Errors
- Rate limiting compliance with automatic delays
- HTTP error classification and retry logic
- WebSocket error handling with reconnection

### Data Errors
- Message parsing error handling
- Invalid data filtering and logging
- Subscription state recovery after errors

## Monitoring and Debugging

### Statistics
The client tracks comprehensive statistics:
```python
stats = data_client.stats
print(f"Messages received: {stats['messages_received']:,}")
print(f"Messages processed: {stats['messages_processed']:,}")
print(f"Errors: {stats['errors']:,}")
print(f"Reconnections: {stats['reconnections']:,}")
```

### Health Checks
```python
health = await data_client._health_check()
print(f"Client health: {'OK' if health else 'FAILED'}")
```

### Subscription State
```python
data_client._log_subscription_state()
data_client._log_statistics()
```

## Performance Considerations

### Message Processing
- Rust-based message parsing for high performance
- Asynchronous message handling with concurrent processing
- Efficient memory management with object reuse

### Rate Limiting
- Automatic compliance with Delta Exchange API limits
- Request queuing and throttling
- Intelligent retry logic with backoff

### Connection Management
- Persistent WebSocket connections with automatic reconnection
- Connection pooling for HTTP requests
- Efficient subscription management

## Security

### Credential Management
- Secure HMAC-SHA256 authentication
- Environment variable support for credentials
- Automatic credential cleanup on disconnect

### Network Security
- TLS/SSL encryption for all connections
- Certificate validation
- Secure WebSocket connections (WSS)

## Troubleshooting

### Common Issues

1. **Connection Failures**
   - Verify API credentials are correct
   - Check network connectivity
   - Ensure firewall allows WebSocket connections

2. **Authentication Errors**
   - Verify API key has required permissions
   - Check API secret is correct
   - Ensure testnet/production environment matches credentials

3. **Subscription Issues**
   - Check symbol filters are not too restrictive
   - Verify instruments are loaded correctly
   - Check WebSocket connection is established

4. **Data Issues**
   - Verify message parsing is working correctly
   - Check for rate limiting errors
   - Monitor error statistics

### Debug Logging
Enable debug logging for detailed troubleshooting:
```python
import logging
logging.getLogger('nautilus_trader.adapters.delta_exchange').setLevel(logging.DEBUG)
```

## Integration with Nautilus Trader

The data client integrates seamlessly with the Nautilus Trader framework:

- **Event-driven Architecture**: All data is emitted as Nautilus events
- **Type Safety**: Full type conversion from Delta Exchange to Nautilus models
- **Caching**: Automatic instrument and data caching
- **Message Bus**: Integration with Nautilus message bus system
- **Lifecycle Management**: Proper startup and shutdown handling

For complete examples, see `examples/adapters/delta_exchange/data_examples.py`.
