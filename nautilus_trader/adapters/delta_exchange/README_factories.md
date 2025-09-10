# Delta Exchange Factory Classes

The Delta Exchange adapter provides comprehensive factory classes for creating and managing clients with proper dependency injection, configuration management, caching, and resource management. These factories handle all aspects of client instantiation and provide production-ready patterns for building trading systems.

## Factory Classes Overview

### Core Factory Classes
- **DeltaExchangeLiveDataClientFactory**: Creates data clients for market data subscriptions
- **DeltaExchangeLiveExecClientFactory**: Creates execution clients for trading operations
- **DeltaExchangeLiveDataEngineFactory**: Creates complete data engine configurations
- **DeltaExchangeLiveExecEngineFactory**: Creates complete execution engine configurations

### Key Features
- **Intelligent Caching**: Efficient resource usage with LRU caching for HTTP/WebSocket clients
- **Configuration Validation**: Comprehensive validation before client creation
- **Environment Management**: Support for production, testnet, and sandbox environments
- **Error Handling**: Robust error handling with clear error messages
- **Resource Management**: Proper cleanup and disposal mechanisms
- **Dependency Injection**: Clean separation of concerns with proper DI patterns

## Basic Usage

### Creating Individual Clients

```python
import asyncio
from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeDataClientConfig
from nautilus_trader.adapters.delta_exchange.factories import DeltaExchangeLiveDataClientFactory
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.test_kit.mocks import MockMessageBus

# Create configuration
config = DeltaExchangeDataClientConfig.testnet(
    api_key="your_testnet_api_key",
    api_secret="your_testnet_api_secret",
    enable_private_channels=True,
)

# Create components
loop = asyncio.get_event_loop()
msgbus = MockMessageBus()
cache = Cache()
clock = LiveClock()

# Create data client
data_client = DeltaExchangeLiveDataClientFactory.create(
    loop=loop,
    name="DeltaExchange-Data",
    config=config,
    msgbus=msgbus,
    cache=cache,
    clock=clock,
)
```

### Creating Execution Clients

```python
from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeExecClientConfig
from nautilus_trader.adapters.delta_exchange.factories import DeltaExchangeLiveExecClientFactory
from decimal import Decimal

# Create execution configuration with risk management
exec_config = DeltaExchangeExecClientConfig.testnet(
    api_key="your_testnet_api_key",
    api_secret="your_testnet_api_secret",
    account_id="testnet_account",
    position_limits={"BTCUSDT": Decimal("10.0")},
    daily_loss_limit=Decimal("5000.0"),
    max_position_value=Decimal("100000.0"),
)

# Create execution client
exec_client = DeltaExchangeLiveExecClientFactory.create(
    loop=loop,
    name="DeltaExchange-Exec",
    config=exec_config,
    msgbus=msgbus,
    cache=cache,
    clock=clock,
)
```

## Advanced Usage

### Using Utility Functions

```python
from nautilus_trader.adapters.delta_exchange.factories import create_delta_exchange_clients

# Create both clients with shared dependencies
data_client, exec_client = create_delta_exchange_clients(
    data_config=data_config,
    exec_config=exec_config,
    loop=loop,
    msgbus=msgbus,
    cache=cache,
    clock=clock,
)
```

### Environment-Specific Factory Creation

```python
from nautilus_trader.adapters.delta_exchange.factories import (
    create_testnet_factories,
    create_production_factories,
)

# Create testnet factories
testnet_data_factory, testnet_exec_factory = create_testnet_factories(
    api_key="testnet_key",
    api_secret="testnet_secret",
    account_id="testnet_account",
)

# Create production factories
prod_data_factory, prod_exec_factory = create_production_factories(
    api_key="production_key",
    api_secret="production_secret",
    account_id="production_account",
)
```

## Trading Node Integration

### Using Engine Factories

```python
from nautilus_trader.adapters.delta_exchange.factories import (
    DeltaExchangeLiveDataEngineFactory,
    DeltaExchangeLiveExecEngineFactory,
)
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode

# Create engine configurations
data_engine_config = DeltaExchangeLiveDataEngineFactory.create_config(
    api_key="your_api_key",
    api_secret="your_api_secret",
    testnet=True,
    product_types=["perpetual_futures"],
    symbol_patterns=["BTC*", "ETH*"],
)

exec_engine_config = DeltaExchangeLiveExecEngineFactory.create_config(
    api_key="your_api_key",
    api_secret="your_api_secret",
    account_id="your_account",
    testnet=True,
    position_limits={"BTCUSDT": Decimal("5.0")},
)

# Create trading node configuration
trading_config = TradingNodeConfig(
    trader_id="TRADER-001",
    data_engine=data_engine_config,
    exec_engine=exec_engine_config,
)

# Create and configure trading node
node = TradingNode(config=trading_config)

# Register factories
DeltaExchangeLiveDataEngineFactory.register_with_node(node)
DeltaExchangeLiveExecEngineFactory.register_with_node(node)

# Build and start the node
node.build()
node.start()
```

## Caching and Performance

### Cache Management

```python
from nautilus_trader.adapters.delta_exchange.factories import (
    get_delta_exchange_factory_info,
    clear_delta_exchange_caches,
)

# Get cache information
cache_info = get_delta_exchange_factory_info()
print(f"HTTP client cache hits: {cache_info['http_client_cache']['hits']}")
print(f"Current cache size: {cache_info['http_client_cache']['currsize']}")

# Clear caches when needed
clear_delta_exchange_caches()
```

### Cache Benefits

The factory caching system provides several benefits:

1. **Resource Efficiency**: Reuses HTTP and WebSocket clients with identical parameters
2. **Connection Pooling**: Maintains persistent connections for better performance
3. **Memory Optimization**: Avoids duplicate client instances
4. **Startup Performance**: Faster client creation for subsequent requests

## Configuration Validation

### Automatic Validation

All factory methods perform comprehensive configuration validation:

```python
# This will raise a ValueError
try:
    invalid_config = DeltaExchangeDataClientConfig(
        testnet=True,
        sandbox=True,  # Invalid: both testnet and sandbox
    )
    client = DeltaExchangeLiveDataClientFactory.create(...)
except RuntimeError as e:
    print(f"Configuration error: {e}")
```

### Validation Checks

- **API Credentials**: Required for private channels and execution
- **Environment Settings**: Cannot use both testnet and sandbox
- **Timeout Values**: Must be positive numbers
- **Risk Management**: Position and loss limits must be positive
- **Account Settings**: Account ID required for execution clients

## Error Handling

### Comprehensive Error Handling

```python
try:
    client = DeltaExchangeLiveDataClientFactory.create(
        loop=loop,
        name="test_client",
        config=config,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
except RuntimeError as e:
    # Handle client creation failure
    print(f"Failed to create client: {e}")
except ValueError as e:
    # Handle configuration validation error
    print(f"Invalid configuration: {e}")
```

### Error Types

- **RuntimeError**: Client creation failures, dependency issues
- **ValueError**: Configuration validation failures
- **ConnectionError**: Network connectivity issues
- **AuthenticationError**: Invalid API credentials

## Environment Management

### Supported Environments

1. **Production**: Live trading with real money
2. **Testnet**: Paper trading with test funds
3. **Sandbox**: Development and testing environment

### Environment Configuration

```python
# Production configuration
prod_config = DeltaExchangeDataClientConfig(
    api_key="prod_key",
    api_secret="prod_secret",
    testnet=False,
    sandbox=False,
)

# Testnet configuration
testnet_config = DeltaExchangeDataClientConfig.testnet(
    api_key="testnet_key",
    api_secret="testnet_secret",
)

# Sandbox configuration
sandbox_config = DeltaExchangeDataClientConfig.sandbox(
    api_key="sandbox_key",
    api_secret="sandbox_secret",
)
```

## Monitoring and Debugging

### Factory Information

```python
from nautilus_trader.adapters.delta_exchange.factories import (
    get_delta_exchange_factory_info,
    validate_factory_environment,
)

# Get comprehensive factory information
info = get_delta_exchange_factory_info()
print(f"Supported environments: {info['supported_environments']}")
print(f"Factory classes: {info['factory_classes']}")

# Validate factory environment
validation = validate_factory_environment()
for component, status in validation.items():
    print(f"{component}: {'✓' if status else '✗'}")
```

### Debug Logging

```python
import logging

# Enable debug logging for factories
logging.getLogger("nautilus_trader.adapters.delta_exchange.factories").setLevel(logging.DEBUG)
```

## Best Practices

### 1. Configuration Management
- Use environment-specific factory methods (`testnet()`, `production()`)
- Validate configurations before client creation
- Store sensitive credentials securely using environment variables

### 2. Resource Management
- Use factory caching for efficient resource usage
- Clear caches when switching environments or credentials
- Monitor cache statistics for performance optimization

### 3. Error Handling
- Always wrap factory calls in try-catch blocks
- Handle specific error types appropriately
- Log errors for debugging and monitoring

### 4. Testing
- Use testnet environment for development and testing
- Clear caches between test runs for isolation
- Validate factory environment before running tests

### 5. Production Deployment
- Use production factories for live trading
- Implement proper monitoring and alerting
- Have fallback mechanisms for factory failures

## Troubleshooting

### Common Issues

1. **Cache-related Issues**
   - Clear caches if experiencing stale client behavior
   - Monitor cache sizes for memory usage

2. **Configuration Errors**
   - Validate all required fields are provided
   - Check environment variable settings
   - Ensure API credentials are correct

3. **Dependency Issues**
   - Verify Rust bindings are properly installed
   - Check that all required packages are available
   - Validate factory environment before use

For more examples and advanced usage patterns, see the `examples/adapters/delta_exchange/factory_examples.py` file.
