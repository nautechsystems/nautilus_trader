# Delta Exchange Execution Client

The `DeltaExchangeExecutionClient` provides comprehensive order management and trade execution capabilities for the Delta Exchange derivatives platform. This client handles all trading operations including order submission, modification, cancellation, position tracking, and real-time execution updates.

## Features

### Core Functionality
- **Complete Order Management**: Submit, modify, and cancel orders with full lifecycle tracking
- **Real-time Execution Updates**: WebSocket-based order and position updates
- **Position Tracking**: Automatic position synchronization and monitoring
- **Risk Management**: Pre-trade risk checks and position limits
- **Batch Operations**: Efficient batch order submission and cancellation
- **Account Management**: Real-time account state and margin monitoring

### Supported Order Types
- **Limit Orders**: Standard limit orders with price and quantity
- **Market Orders**: Immediate execution at best available price
- **Stop Orders**: Stop-loss and take-profit orders with trigger prices
- **Bracket Orders**: Entry order with automatic stop-loss and take-profit
- **Order Lists**: Batch submission of multiple orders

### Risk Management Features
- **Position Limits**: Maximum position size per instrument
- **Order Size Limits**: Minimum and maximum order sizes
- **Daily Loss Limits**: Maximum daily loss thresholds
- **Position Value Limits**: Maximum total position value
- **Pre-trade Validation**: Comprehensive order validation before submission

## Configuration

### Basic Configuration
```python
from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeExecClientConfig

# Testnet configuration
config = DeltaExchangeExecClientConfig.testnet(
    api_key="your_testnet_api_key",
    api_secret="your_testnet_api_secret",
    account_id="test_account",
)

# Production configuration
config = DeltaExchangeExecClientConfig(
    api_key="your_api_key",
    api_secret="your_api_secret",
    testnet=False,
    account_id="prod_account",
)
```

### Advanced Configuration
```python
from decimal import Decimal

config = DeltaExchangeExecClientConfig(
    api_key="your_api_key",
    api_secret="your_api_secret",
    testnet=False,
    account_id="prod_account",
    
    # Connection settings
    max_retries=5,
    retry_delay_secs=2.0,
    http_timeout_secs=30.0,
    ws_timeout_secs=10.0,
    
    # Risk management
    position_limits={
        "BTCUSDT": Decimal("10.0"),
        "ETHUSDT": Decimal("100.0"),
    },
    order_size_limits={
        "BTCUSDT": (Decimal("0.001"), Decimal("1.0")),
        "ETHUSDT": (Decimal("0.01"), Decimal("10.0")),
    },
    daily_loss_limit=Decimal("5000.0"),
    max_position_value=Decimal("100000.0"),
    
    # Performance settings
    enable_order_book_deltas=True,
    enable_position_updates=True,
    enable_margin_updates=True,
)
```

## Usage Examples

### Basic Order Management
```python
import asyncio
from nautilus_trader.adapters.delta_exchange.execution import DeltaExchangeExecutionClient
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.enums import OrderSide, TimeInForce

async def basic_trading():
    # Create and connect execution client
    exec_client = DeltaExchangeExecutionClient(...)
    await exec_client._connect()
    
    # Create a limit order
    order = LimitOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("STRATEGY-001"),
        instrument_id=InstrumentId(Symbol("BTCUSDT"), DELTA_EXCHANGE),
        client_order_id=ClientOrderId("ORDER-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.1"),
        price=Price.from_str("45000.00"),
        time_in_force=TimeInForce.GTC,
        init_id=UUID4(),
        ts_init=clock.timestamp_ns(),
    )
    
    # Submit the order
    await exec_client._submit_order(order)
    
    # Wait for execution updates
    await asyncio.sleep(5)
    
    # Cancel the order if still open
    if order.venue_order_id:
        await exec_client._cancel_order(order)
```

### Batch Order Operations
```python
async def batch_trading():
    # Create multiple orders
    orders = [
        LimitOrder(...),  # BTC buy order
        LimitOrder(...),  # BTC sell order
        LimitOrder(...),  # ETH buy order
    ]
    
    # Create order list
    order_list = OrderList(
        order_list_id=OrderListId("BATCH-001"),
        orders=orders,
    )
    
    # Submit all orders at once
    await exec_client._submit_order_list(order_list)
    
    # Cancel all orders for a specific instrument
    await exec_client._cancel_all_orders(btc_instrument.id)
```

### Position Monitoring
```python
async def monitor_positions():
    # Generate position status reports
    position_reports = await exec_client.generate_position_status_reports()
    
    for report in position_reports:
        print(f"Position: {report.instrument_id}")
        print(f"Net Quantity: {report.net_qty}")
        print(f"Unrealized PnL: {report.unrealized_pnl}")
        print(f"Margin Used: {report.margin_used}")
```

### Account State Monitoring
```python
async def monitor_account():
    # Query current account state
    account_state = await exec_client.query_account()
    
    if account_state:
        print(f"Account ID: {account_state.account_id}")
        print(f"Base Currency: {account_state.base_currency}")
        
        for balance in account_state.balances:
            print(f"Currency: {balance.currency}")
            print(f"Total: {balance.total}")
            print(f"Free: {balance.free}")
            print(f"Locked: {balance.locked}")
```

## WebSocket Message Handling

The execution client automatically handles various WebSocket message types:

### Order Updates
- **Order Accepted**: When an order is accepted by the exchange
- **Order Rejected**: When an order is rejected with reason
- **Order Filled**: When an order is partially or fully filled
- **Order Cancelled**: When an order is successfully cancelled
- **Order Modified**: When an order modification is processed

### Position Updates
- **Position Opened**: When a new position is established
- **Position Updated**: When an existing position changes
- **Position Closed**: When a position is fully closed

### Account Updates
- **Margin Updates**: Real-time margin requirement changes
- **Balance Updates**: Account balance changes from trades
- **Portfolio Margin**: Portfolio-level margin calculations

## Error Handling

### Connection Management
```python
# Automatic reconnection with exponential backoff
config = DeltaExchangeExecClientConfig(
    max_retries=5,
    retry_delay_secs=2.0,
    reconnection_delay_secs=5.0,
)

# Health monitoring
health_ok = await exec_client._health_check()
if not health_ok:
    await exec_client._reconnect_if_needed()
```

### Order Error Handling
```python
# Orders are automatically rejected if they fail risk checks
# Error events are generated for failed operations
# Comprehensive logging for debugging

# Check execution statistics
stats = exec_client.stats
print(f"Orders submitted: {stats['orders_submitted']}")
print(f"Orders rejected: {stats['orders_rejected']}")
print(f"Errors: {stats['errors']}")
```

## Risk Management

### Position Limits
```python
config = DeltaExchangeExecClientConfig(
    position_limits={
        "BTCUSDT": Decimal("10.0"),    # Max 10 BTC position
        "ETHUSDT": Decimal("100.0"),   # Max 100 ETH position
    }
)
```

### Order Size Limits
```python
config = DeltaExchangeExecClientConfig(
    order_size_limits={
        "BTCUSDT": (Decimal("0.001"), Decimal("1.0")),  # Min 0.001, Max 1.0
        "ETHUSDT": (Decimal("0.01"), Decimal("10.0")),  # Min 0.01, Max 10.0
    }
)
```

### Daily Loss Limits
```python
config = DeltaExchangeExecClientConfig(
    daily_loss_limit=Decimal("5000.0"),  # Max $5000 daily loss
)
```

## Performance Optimization

### Rate Limiting
- Automatic rate limiting to comply with Delta Exchange API limits
- Configurable request throttling
- Efficient batch operations to minimize API calls

### Connection Management
- Persistent WebSocket connections with automatic reconnection
- Connection health monitoring and recovery
- Optimized message parsing using Rust bindings

### Memory Management
- Efficient order and position tracking
- Automatic cleanup of completed orders
- Optimized data structures for high-frequency trading

## Monitoring and Debugging

### Statistics Tracking
```python
# Get comprehensive statistics
stats = exec_client.stats
print(f"Orders submitted: {stats['orders_submitted']:,}")
print(f"Orders filled: {stats['orders_filled']:,}")
print(f"API calls: {stats['api_calls']:,}")
print(f"Errors: {stats['errors']:,}")
print(f"Reconnections: {stats['reconnections']:,}")
```

### Logging Configuration
```python
import logging

# Configure detailed logging
logging.getLogger("nautilus_trader.adapters.delta_exchange").setLevel(logging.DEBUG)

# Log execution state
exec_client._log_execution_state()
exec_client._log_statistics()
```

### Health Monitoring
```python
# Regular health checks
async def monitor_health():
    while True:
        health = await exec_client._health_check()
        if not health:
            print("Health check failed - attempting reconnection")
            await exec_client._reconnect_if_needed()
        
        await asyncio.sleep(30)  # Check every 30 seconds
```

## Security Considerations

### API Credentials
- Store API credentials securely using environment variables
- Use separate credentials for testnet and production
- Implement proper credential rotation procedures

### Risk Controls
- Always configure appropriate position and loss limits
- Implement circuit breakers for abnormal market conditions
- Monitor account exposure and margin requirements

### Network Security
- Use TLS/SSL for all connections
- Implement proper authentication for private channels
- Monitor for unusual trading patterns or API usage

## Troubleshooting

### Common Issues

1. **Connection Failures**
   - Check API credentials and permissions
   - Verify network connectivity
   - Check Delta Exchange service status

2. **Order Rejections**
   - Verify instrument symbols and parameters
   - Check account balance and margin requirements
   - Review risk management settings

3. **WebSocket Disconnections**
   - Monitor connection health regularly
   - Implement proper reconnection logic
   - Check for rate limiting issues

### Debug Mode
```python
# Enable debug logging
config = DeltaExchangeExecClientConfig(
    debug_mode=True,
    log_level="DEBUG",
)

# Monitor WebSocket messages
exec_client._log_execution_state()
```

For more examples and advanced usage patterns, see the `examples/adapters/delta_exchange/execution_examples.py` file.
