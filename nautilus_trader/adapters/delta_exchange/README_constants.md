# Delta Exchange Constants and Enumerations

The Delta Exchange adapter provides comprehensive constants and enumerations that ensure type safety, clear semantics, and proper integration with the Nautilus Trader framework. This document covers all available constants, their usage patterns, and best practices.

## Overview

The constants module (`nautilus_trader.adapters.delta_exchange.constants`) provides:

- **Venue Identifiers**: Standard venue and client identifiers
- **API Constants**: URLs, endpoints, and configuration values
- **WebSocket Constants**: Channel names and connection parameters
- **Enumerations**: Type-safe enums for Delta Exchange-specific values
- **Data Model Mappings**: Conversion between Delta Exchange and Nautilus types
- **Validation Patterns**: Regular expressions for input validation
- **Error Codes**: Comprehensive error code mappings
- **Configuration Defaults**: Standard configuration values

## Core Constants

### Venue and Client Identifiers

```python
from nautilus_trader.adapters.delta_exchange.constants import (
    DELTA_EXCHANGE,
    DELTA_EXCHANGE_VENUE,
    DELTA_EXCHANGE_CLIENT_ID,
)

# Usage
venue = DELTA_EXCHANGE_VENUE  # Venue("DELTA_EXCHANGE")
client_id = DELTA_EXCHANGE_CLIENT_ID  # ClientId("DELTA_EXCHANGE")
```

### API URLs and Endpoints

```python
from nautilus_trader.adapters.delta_exchange.constants import (
    DELTA_EXCHANGE_HTTP_URLS,
    DELTA_EXCHANGE_WS_URLS,
    DELTA_EXCHANGE_PRODUCTS_ENDPOINT,
)

# Environment-specific URLs
production_url = DELTA_EXCHANGE_HTTP_URLS["production"]
testnet_url = DELTA_EXCHANGE_HTTP_URLS["testnet"]
sandbox_url = DELTA_EXCHANGE_HTTP_URLS["sandbox"]

# WebSocket URLs
ws_production = DELTA_EXCHANGE_WS_URLS["production"]
ws_testnet = DELTA_EXCHANGE_WS_URLS["testnet"]
```

## Enumerations

### Product Types

```python
from nautilus_trader.adapters.delta_exchange.constants import DeltaExchangeProductType

# Available product types
product_type = DeltaExchangeProductType.PERPETUAL_FUTURES
call_option = DeltaExchangeProductType.CALL_OPTIONS
put_option = DeltaExchangeProductType.PUT_OPTIONS

# Type checking
if product_type.is_perpetual:
    print("This is a perpetual futures contract")

if call_option.is_option:
    print("This is an options contract")
```

### Order Types

```python
from nautilus_trader.adapters.delta_exchange.constants import DeltaExchangeOrderType

# Available order types
limit_order = DeltaExchangeOrderType.LIMIT_ORDER
market_order = DeltaExchangeOrderType.MARKET_ORDER
stop_loss = DeltaExchangeOrderType.STOP_LOSS_ORDER
take_profit = DeltaExchangeOrderType.TAKE_PROFIT_ORDER

# Type checking
if market_order.is_market:
    print("This is a market order")

if stop_loss.is_stop:
    print("This is a stop-related order")
```

### Order Status

```python
from nautilus_trader.adapters.delta_exchange.constants import DeltaExchangeOrderStatus

# Available order statuses
open_status = DeltaExchangeOrderStatus.OPEN
filled_status = DeltaExchangeOrderStatus.FILLED
cancelled_status = DeltaExchangeOrderStatus.CANCELLED

# Status checking
if open_status.is_active:
    print("Order is still active")

if filled_status.is_terminal:
    print("Order has reached a final state")
```

### Time in Force

```python
from nautilus_trader.adapters.delta_exchange.constants import DeltaExchangeTimeInForce

# Available time-in-force values
gtc = DeltaExchangeTimeInForce.GTC  # Good Till Cancel
ioc = DeltaExchangeTimeInForce.IOC  # Immediate or Cancel
fok = DeltaExchangeTimeInForce.FOK  # Fill or Kill
gtd = DeltaExchangeTimeInForce.GTD  # Good Till Date

# Execution checking
if ioc.is_immediate:
    print("This order requires immediate execution")
```

## Data Model Mappings

### Order Type Conversion

```python
from nautilus_trader.adapters.delta_exchange.constants import (
    DELTA_EXCHANGE_TO_NAUTILUS_ORDER_TYPE,
    NAUTILUS_TO_DELTA_EXCHANGE_ORDER_TYPE,
)
from nautilus_trader.model.enums import OrderType

# Delta Exchange to Nautilus
delta_type = "limit_order"
nautilus_type = DELTA_EXCHANGE_TO_NAUTILUS_ORDER_TYPE[delta_type]
# Result: OrderType.LIMIT

# Nautilus to Delta Exchange
nautilus_type = OrderType.MARKET
delta_type = NAUTILUS_TO_DELTA_EXCHANGE_ORDER_TYPE[nautilus_type]
# Result: "market_order"
```

### Order Status Conversion

```python
from nautilus_trader.adapters.delta_exchange.constants import (
    DELTA_EXCHANGE_TO_NAUTILUS_ORDER_STATUS,
)

# Convert Delta Exchange status to Nautilus
delta_status = "open"
nautilus_status = DELTA_EXCHANGE_TO_NAUTILUS_ORDER_STATUS[delta_status]
# Result: OrderStatus.ACCEPTED
```

### Time in Force Conversion

```python
from nautilus_trader.adapters.delta_exchange.constants import (
    DELTA_EXCHANGE_TO_NAUTILUS_TIME_IN_FORCE,
    NAUTILUS_TO_DELTA_EXCHANGE_TIME_IN_FORCE,
)

# Bidirectional conversion
delta_tif = "gtc"
nautilus_tif = DELTA_EXCHANGE_TO_NAUTILUS_TIME_IN_FORCE[delta_tif]
back_to_delta = NAUTILUS_TO_DELTA_EXCHANGE_TIME_IN_FORCE[nautilus_tif]
```

## WebSocket Constants

### Channel Names

```python
from nautilus_trader.adapters.delta_exchange.constants import (
    DELTA_EXCHANGE_WS_PUBLIC_CHANNELS,
    DELTA_EXCHANGE_WS_PRIVATE_CHANNELS,
    DELTA_EXCHANGE_WS_ALL_CHANNELS,
)

# Public channels (no authentication required)
public_channels = DELTA_EXCHANGE_WS_PUBLIC_CHANNELS
# ['v2_ticker', 'l2_orderbook', 'all_trades', ...]

# Private channels (authentication required)
private_channels = DELTA_EXCHANGE_WS_PRIVATE_CHANNELS
# ['orders', 'positions', 'margins', ...]

# All channels
all_channels = DELTA_EXCHANGE_WS_ALL_CHANNELS
```

### Channel Selection

```python
# Market data channels
market_data = ["v2_ticker", "l2_orderbook", "all_trades"]

# Price data channels
price_data = ["mark_price", "spot_price", "funding_rate"]

# Trading channels
trading_data = ["orders", "positions", "user_trades"]
```

## Validation Constants

### API Credential Validation

```python
import re
from nautilus_trader.adapters.delta_exchange.constants import (
    DELTA_EXCHANGE_API_KEY_PATTERN,
    DELTA_EXCHANGE_API_SECRET_PATTERN,
)

# Validate API key
api_key_pattern = re.compile(DELTA_EXCHANGE_API_KEY_PATTERN)
is_valid_key = bool(api_key_pattern.match("your_api_key"))

# Validate API secret
api_secret_pattern = re.compile(DELTA_EXCHANGE_API_SECRET_PATTERN)
is_valid_secret = bool(api_secret_pattern.match("your_api_secret"))
```

### Symbol and Price Validation

```python
from decimal import Decimal
from nautilus_trader.adapters.delta_exchange.constants import (
    DELTA_EXCHANGE_SYMBOL_PATTERN,
    DELTA_EXCHANGE_MIN_ORDER_PRICE,
    DELTA_EXCHANGE_MAX_ORDER_PRICE,
)

# Validate symbol
symbol_pattern = re.compile(DELTA_EXCHANGE_SYMBOL_PATTERN)
is_valid_symbol = bool(symbol_pattern.match("BTCUSDT"))

# Validate price
def validate_price(price_str: str) -> bool:
    try:
        price = Decimal(price_str)
        min_price = Decimal(DELTA_EXCHANGE_MIN_ORDER_PRICE)
        max_price = Decimal(DELTA_EXCHANGE_MAX_ORDER_PRICE)
        return min_price <= price <= max_price
    except (ValueError, TypeError):
        return False
```

## Error Handling

### Error Code Mappings

```python
from nautilus_trader.adapters.delta_exchange.constants import DELTA_EXCHANGE_ERROR_CODES

def handle_api_error(error_code: int) -> str:
    """Get human-readable error message."""
    return DELTA_EXCHANGE_ERROR_CODES.get(
        error_code, 
        f"Unknown error code: {error_code}"
    )

# Common error codes
auth_error = handle_api_error(401)  # "Unauthorized"
rate_limit = handle_api_error(429)  # "Too Many Requests"
server_error = handle_api_error(500)  # "Internal Server Error"
```

## Configuration Constants

### Default Configuration

```python
from nautilus_trader.adapters.delta_exchange.constants import (
    DELTA_EXCHANGE_DEFAULT_CONFIG,
    DELTA_EXCHANGE_FEATURE_FLAGS,
)

# Get default configuration
default_config = DELTA_EXCHANGE_DEFAULT_CONFIG.copy()

# Check feature flags
if DELTA_EXCHANGE_FEATURE_FLAGS["enable_portfolio_margins"]:
    print("Portfolio margins are enabled")

if DELTA_EXCHANGE_FEATURE_FLAGS["enable_options_trading"]:
    print("Options trading is enabled")
```

### Environment-Specific Configuration

```python
def create_environment_config(environment: str) -> dict:
    """Create configuration for specific environment."""
    config = DELTA_EXCHANGE_DEFAULT_CONFIG.copy()
    
    if environment == "testnet":
        config.update({
            "testnet": True,
            "sandbox": False,
            "enable_private_channels": False,
        })
    elif environment == "production":
        config.update({
            "testnet": False,
            "sandbox": False,
            "enable_private_channels": True,
        })
    
    return config
```

## Supported Types

### Nautilus Trader Integration

```python
from nautilus_trader.adapters.delta_exchange.constants import (
    DELTA_EXCHANGE_SUPPORTED_ORDER_TYPES,
    DELTA_EXCHANGE_SUPPORTED_TIME_IN_FORCE,
    DELTA_EXCHANGE_SUPPORTED_ORDER_SIDES,
)
from nautilus_trader.model.enums import OrderType, TimeInForce, OrderSide

# Check if order type is supported
def is_supported_order_type(order_type: OrderType) -> bool:
    return order_type in DELTA_EXCHANGE_SUPPORTED_ORDER_TYPES

# Check if time-in-force is supported
def is_supported_tif(tif: TimeInForce) -> bool:
    return tif in DELTA_EXCHANGE_SUPPORTED_TIME_IN_FORCE

# Supported types
supported_orders = DELTA_EXCHANGE_SUPPORTED_ORDER_TYPES
# {OrderType.MARKET, OrderType.LIMIT, OrderType.STOP_MARKET, OrderType.STOP_LIMIT}

supported_tif = DELTA_EXCHANGE_SUPPORTED_TIME_IN_FORCE
# {TimeInForce.GTC, TimeInForce.IOC, TimeInForce.FOK, TimeInForce.GTD}
```

## Best Practices

### 1. Type Safety
Always use the provided enumerations instead of string literals:

```python
# Good
order_type = DeltaExchangeOrderType.LIMIT_ORDER

# Avoid
order_type = "limit_order"
```

### 2. Validation
Use validation patterns for user input:

```python
def validate_api_credentials(api_key: str, api_secret: str) -> bool:
    key_pattern = re.compile(DELTA_EXCHANGE_API_KEY_PATTERN)
    secret_pattern = re.compile(DELTA_EXCHANGE_API_SECRET_PATTERN)
    
    return (
        bool(key_pattern.match(api_key)) and 
        bool(secret_pattern.match(api_secret))
    )
```

### 3. Error Handling
Use error code constants for consistent error handling:

```python
def process_api_response(response):
    if response.status_code in DELTA_EXCHANGE_ERROR_CODES:
        error_message = DELTA_EXCHANGE_ERROR_CODES[response.status_code]
        raise RuntimeError(f"API Error {response.status_code}: {error_message}")
```

### 4. Configuration
Use default configuration as a base and override specific values:

```python
def create_custom_config(**overrides):
    config = DELTA_EXCHANGE_DEFAULT_CONFIG.copy()
    config.update(overrides)
    return config
```

## Troubleshooting

### Common Issues

1. **Import Errors**: Ensure you're importing from the correct module path
2. **Type Mismatches**: Use the provided mappings for type conversion
3. **Validation Failures**: Check input against validation patterns
4. **Configuration Errors**: Verify configuration against default values

### Debug Information

```python
from nautilus_trader.adapters.delta_exchange.constants import DELTA_EXCHANGE_ALL_CONSTANTS

# Get comprehensive information about all constants
all_constants = DELTA_EXCHANGE_ALL_CONSTANTS

# Check available categories
print("Available constant categories:")
for category in all_constants.keys():
    print(f"  {category}")

# Validate configuration
def debug_configuration(config: dict):
    """Debug configuration against constants."""
    for key, value in config.items():
        print(f"{key}: {value} (type: {type(value).__name__})")
```

For more examples and advanced usage patterns, see the `examples/adapters/delta_exchange/constants_examples.py` file.
