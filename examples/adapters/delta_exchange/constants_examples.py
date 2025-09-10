#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

"""
Delta Exchange Constants Usage Examples.

This module demonstrates how to use the Delta Exchange constants and enumerations
for various trading scenarios, configuration management, data model mapping,
and validation purposes.
"""

import re
from decimal import Decimal

from nautilus_trader.adapters.delta_exchange.constants import (
    DELTA_EXCHANGE_ALL_CONSTANTS,
    DELTA_EXCHANGE_API_KEY_PATTERN,
    DELTA_EXCHANGE_CLIENT_ID,
    DELTA_EXCHANGE_CURRENCY_MAP,
    DELTA_EXCHANGE_DEFAULT_CONFIG,
    DELTA_EXCHANGE_ERROR_CODES,
    DELTA_EXCHANGE_HTTP_URLS,
    DELTA_EXCHANGE_MAX_ORDER_PRICE,
    DELTA_EXCHANGE_MIN_ORDER_PRICE,
    DELTA_EXCHANGE_SUPPORTED_ORDER_TYPES,
    DELTA_EXCHANGE_TO_NAUTILUS_ORDER_STATUS,
    DELTA_EXCHANGE_TO_NAUTILUS_ORDER_TYPE,
    DELTA_EXCHANGE_VENUE,
    DELTA_EXCHANGE_WS_PUBLIC_CHANNELS,
    DELTA_EXCHANGE_WS_URLS,
    DeltaExchangeOrderStatus,
    DeltaExchangeOrderType,
    DeltaExchangeProductType,
    DeltaExchangeTimeInForce,
    DeltaExchangeTradingStatus,
    NAUTILUS_TO_DELTA_EXCHANGE_ORDER_TYPE,
)
from nautilus_trader.model.enums import OrderType


def venue_and_client_examples():
    """
    Demonstrate usage of venue and client identifier constants.
    
    These constants are used throughout the adapter for identifying
    the exchange and creating client instances.
    """
    print("=== Venue and Client Identifier Examples ===")
    
    # Using venue identifier
    print(f"Exchange venue: {DELTA_EXCHANGE_VENUE}")
    print(f"Venue string representation: {str(DELTA_EXCHANGE_VENUE)}")
    
    # Using client identifier
    print(f"Client ID: {DELTA_EXCHANGE_CLIENT_ID}")
    print(f"Client ID string: {str(DELTA_EXCHANGE_CLIENT_ID)}")
    
    # Checking venue equality
    from nautilus_trader.model.identifiers import Venue
    if DELTA_EXCHANGE_VENUE == Venue("DELTA_EXCHANGE"):
        print("✓ Venue identifier matches expected value")


def url_and_environment_examples():
    """
    Demonstrate usage of URL constants for different environments.
    
    These constants are used for configuring HTTP and WebSocket
    connections based on the trading environment.
    """
    print("\n=== URL and Environment Examples ===")
    
    # HTTP URLs for different environments
    print("HTTP URLs:")
    for env, url in DELTA_EXCHANGE_HTTP_URLS.items():
        print(f"  {env}: {url}")
    
    # WebSocket URLs for different environments
    print("\nWebSocket URLs:")
    for env, url in DELTA_EXCHANGE_WS_URLS.items():
        print(f"  {env}: {url}")
    
    # Environment-specific URL selection
    def get_urls_for_environment(environment: str, testnet: bool = False):
        """Get URLs for specific environment."""
        if testnet:
            environment = "testnet"
        elif environment == "sandbox":
            environment = "sandbox"
        else:
            environment = "production"
        
        return {
            "http": DELTA_EXCHANGE_HTTP_URLS[environment],
            "ws": DELTA_EXCHANGE_WS_URLS[environment],
        }
    
    # Example usage
    prod_urls = get_urls_for_environment("production")
    test_urls = get_urls_for_environment("production", testnet=True)
    
    print(f"\nProduction URLs: {prod_urls}")
    print(f"Testnet URLs: {test_urls}")


def enumeration_examples():
    """
    Demonstrate usage of Delta Exchange enumerations.
    
    These enums provide type safety and clear semantics for
    Delta Exchange-specific values.
    """
    print("\n=== Enumeration Examples ===")
    
    # Product type enumeration
    print("Product Types:")
    for product_type in DeltaExchangeProductType:
        print(f"  {product_type.name}: {product_type.value}")
        print(f"    Is perpetual: {product_type.is_perpetual}")
        print(f"    Is option: {product_type.is_option}")
    
    # Order type enumeration
    print("\nOrder Types:")
    for order_type in DeltaExchangeOrderType:
        print(f"  {order_type.name}: {order_type.value}")
        print(f"    Is market: {order_type.is_market}")
        print(f"    Is limit: {order_type.is_limit}")
        print(f"    Is stop: {order_type.is_stop}")
    
    # Order status enumeration
    print("\nOrder Status:")
    for status in DeltaExchangeOrderStatus:
        print(f"  {status.name}: {status.value}")
        print(f"    Is active: {status.is_active}")
        print(f"    Is terminal: {status.is_terminal}")
    
    # Time in force enumeration
    print("\nTime in Force:")
    for tif in DeltaExchangeTimeInForce:
        print(f"  {tif.name}: {tif.value}")
        print(f"    Is immediate: {tif.is_immediate}")
    
    # Trading status enumeration
    print("\nTrading Status:")
    for status in DeltaExchangeTradingStatus:
        print(f"  {status.name}: {status.value}")
        print(f"    Is tradable: {status.is_tradable}")


def data_model_mapping_examples():
    """
    Demonstrate usage of data model mappings between Delta Exchange and Nautilus.
    
    These mappings are essential for converting between exchange-specific
    values and Nautilus Trader domain models.
    """
    print("\n=== Data Model Mapping Examples ===")
    
    # Order type mapping examples
    print("Order Type Mappings:")
    
    # Delta Exchange to Nautilus
    delta_order_type = "limit_order"
    nautilus_order_type = DELTA_EXCHANGE_TO_NAUTILUS_ORDER_TYPE[delta_order_type]
    print(f"Delta '{delta_order_type}' -> Nautilus '{nautilus_order_type}'")
    
    # Nautilus to Delta Exchange
    nautilus_type = OrderType.MARKET
    delta_type = NAUTILUS_TO_DELTA_EXCHANGE_ORDER_TYPE[nautilus_type]
    print(f"Nautilus '{nautilus_type}' -> Delta '{delta_type}'")
    
    # Order status mapping examples
    print("\nOrder Status Mappings:")
    delta_statuses = ["open", "pending", "closed", "cancelled"]
    for delta_status in delta_statuses:
        nautilus_status = DELTA_EXCHANGE_TO_NAUTILUS_ORDER_STATUS[delta_status]
        print(f"Delta '{delta_status}' -> Nautilus '{nautilus_status}'")
    
    # Practical mapping function
    def convert_order_type_to_nautilus(delta_order_type: str) -> OrderType:
        """Convert Delta Exchange order type to Nautilus OrderType."""
        if delta_order_type not in DELTA_EXCHANGE_TO_NAUTILUS_ORDER_TYPE:
            raise ValueError(f"Unsupported Delta Exchange order type: {delta_order_type}")
        return DELTA_EXCHANGE_TO_NAUTILUS_ORDER_TYPE[delta_order_type]
    
    # Example usage
    try:
        converted_type = convert_order_type_to_nautilus("market_order")
        print(f"\nConverted 'market_order' to: {converted_type}")
    except ValueError as e:
        print(f"Conversion error: {e}")


def websocket_channel_examples():
    """
    Demonstrate usage of WebSocket channel constants.
    
    These constants are used for subscribing to real-time data feeds
    from Delta Exchange.
    """
    print("\n=== WebSocket Channel Examples ===")
    
    # Public channels (no authentication required)
    print("Public Channels:")
    for channel in DELTA_EXCHANGE_WS_PUBLIC_CHANNELS:
        print(f"  {channel}")
    
    # Channel selection for different data types
    market_data_channels = [
        "v2_ticker",      # Real-time ticker updates
        "l2_orderbook",   # Order book snapshots
        "l2_updates",     # Order book updates
        "all_trades",     # Trade executions
    ]
    
    price_data_channels = [
        "mark_price",     # Mark price updates
        "spot_price",     # Spot price updates
        "funding_rate",   # Funding rate updates
    ]
    
    print(f"\nMarket data channels: {market_data_channels}")
    print(f"Price data channels: {price_data_channels}")
    
    # Channel subscription example
    def create_subscription_message(channels: list[str], symbols: list[str]):
        """Create WebSocket subscription message."""
        return {
            "type": "subscribe",
            "channels": [
                {
                    "name": channel,
                    "symbols": symbols
                }
                for channel in channels
            ]
        }
    
    # Example subscription
    subscription = create_subscription_message(
        channels=["v2_ticker", "all_trades"],
        symbols=["BTCUSDT", "ETHUSDT"]
    )
    print(f"\nExample subscription: {subscription}")


def validation_examples():
    """
    Demonstrate usage of validation constants and patterns.
    
    These patterns are used for validating API credentials,
    symbols, and other input parameters.
    """
    print("\n=== Validation Examples ===")
    
    # API key validation
    api_key_pattern = re.compile(DELTA_EXCHANGE_API_KEY_PATTERN)
    
    test_api_keys = [
        "abcd1234efgh5678ijkl9012mnop3456",  # Valid
        "ABCD1234EFGH5678IJKL9012MNOP3456",  # Valid
        "short",                              # Invalid - too short
        "contains-special-chars!",            # Invalid - special chars
    ]
    
    print("API Key Validation:")
    for key in test_api_keys:
        is_valid = bool(api_key_pattern.match(key))
        print(f"  '{key}': {'✓ Valid' if is_valid else '✗ Invalid'}")
    
    # Price validation
    def validate_order_price(price: str) -> bool:
        """Validate order price against Delta Exchange limits."""
        try:
            price_decimal = Decimal(price)
            min_price = Decimal(DELTA_EXCHANGE_MIN_ORDER_PRICE)
            max_price = Decimal(DELTA_EXCHANGE_MAX_ORDER_PRICE)
            return min_price <= price_decimal <= max_price
        except (ValueError, TypeError):
            return False
    
    test_prices = ["0.000001", "100.50", "1000000000.0", "1000000001.0", "0.0000001"]
    
    print("\nPrice Validation:")
    for price in test_prices:
        is_valid = validate_order_price(price)
        print(f"  {price}: {'✓ Valid' if is_valid else '✗ Invalid'}")


def configuration_examples():
    """
    Demonstrate usage of configuration constants.
    
    These constants provide default values and feature flags
    for configuring the Delta Exchange adapter.
    """
    print("\n=== Configuration Examples ===")
    
    # Default configuration
    print("Default Configuration:")
    for key, value in DELTA_EXCHANGE_DEFAULT_CONFIG.items():
        print(f"  {key}: {value}")
    
    # Creating environment-specific configuration
    def create_testnet_config():
        """Create configuration for testnet environment."""
        config = DELTA_EXCHANGE_DEFAULT_CONFIG.copy()
        config.update({
            "testnet": True,
            "sandbox": False,
            "enable_private_channels": False,  # Start with public data only
            "product_types": ["perpetual_futures"],  # Limited product types
        })
        return config
    
    def create_production_config():
        """Create configuration for production environment."""
        config = DELTA_EXCHANGE_DEFAULT_CONFIG.copy()
        config.update({
            "testnet": False,
            "sandbox": False,
            "enable_private_channels": True,
            "product_types": ["perpetual_futures", "call_options", "put_options"],
        })
        return config
    
    testnet_config = create_testnet_config()
    production_config = create_production_config()
    
    print(f"\nTestnet config: {testnet_config}")
    print(f"Production config: {production_config}")


def error_handling_examples():
    """
    Demonstrate usage of error code constants.
    
    These constants help with proper error handling and
    user-friendly error messages.
    """
    print("\n=== Error Handling Examples ===")
    
    # Error code lookup
    def get_error_message(error_code: int) -> str:
        """Get human-readable error message for error code."""
        return DELTA_EXCHANGE_ERROR_CODES.get(error_code, f"Unknown error code: {error_code}")
    
    # Common error scenarios
    common_errors = [400, 401, 429, 500, 1001, 2001, 3001, 4001]
    
    print("Common Error Codes:")
    for code in common_errors:
        message = get_error_message(code)
        print(f"  {code}: {message}")
    
    # Error handling function
    def handle_api_error(error_code: int, context: str = ""):
        """Handle API error with appropriate response."""
        message = get_error_message(error_code)
        
        if error_code == 401:
            return f"Authentication failed{' in ' + context if context else ''}: {message}"
        elif error_code == 429:
            return f"Rate limit exceeded{' in ' + context if context else ''}: {message}"
        elif error_code >= 500:
            return f"Server error{' in ' + context if context else ''}: {message}"
        else:
            return f"Client error{' in ' + context if context else ''}: {message}"
    
    # Example error handling
    print("\nError Handling Examples:")
    for code in [401, 429, 500, 2001]:
        handled_message = handle_api_error(code, "order submission")
        print(f"  {code}: {handled_message}")


def supported_types_examples():
    """
    Demonstrate usage of supported type constants.
    
    These constants define which Nautilus Trader types are
    supported by the Delta Exchange adapter.
    """
    print("\n=== Supported Types Examples ===")
    
    # Supported order types
    print("Supported Order Types:")
    for order_type in DELTA_EXCHANGE_SUPPORTED_ORDER_TYPES:
        print(f"  {order_type}")
    
    # Order type validation
    def is_order_type_supported(order_type: OrderType) -> bool:
        """Check if order type is supported by Delta Exchange."""
        return order_type in DELTA_EXCHANGE_SUPPORTED_ORDER_TYPES
    
    # Test various order types
    test_order_types = [
        OrderType.MARKET,
        OrderType.LIMIT,
        OrderType.STOP_MARKET,
        OrderType.STOP_LIMIT,
        OrderType.MARKET_TO_LIMIT,  # Not supported
    ]
    
    print("\nOrder Type Support Check:")
    for order_type in test_order_types:
        is_supported = is_order_type_supported(order_type)
        print(f"  {order_type}: {'✓ Supported' if is_supported else '✗ Not supported'}")


def comprehensive_constants_examples():
    """
    Demonstrate usage of the comprehensive constants collection.
    
    This collection provides access to all Delta Exchange constants
    in a structured format for validation and testing.
    """
    print("\n=== Comprehensive Constants Examples ===")
    
    # Access all constants
    all_constants = DELTA_EXCHANGE_ALL_CONSTANTS
    
    print("Available constant categories:")
    for category in all_constants.keys():
        print(f"  {category}")
    
    # Validate configuration against constants
    def validate_configuration(config: dict) -> list[str]:
        """Validate configuration against Delta Exchange constants."""
        errors = []
        
        # Check product types
        if "product_types" in config:
            valid_types = all_constants["product_types"]
            for product_type in config["product_types"]:
                if product_type not in valid_types:
                    errors.append(f"Invalid product type: {product_type}")
        
        # Check environments
        if "environment" in config:
            valid_envs = all_constants["environments"]
            if config["environment"] not in valid_envs:
                errors.append(f"Invalid environment: {config['environment']}")
        
        return errors
    
    # Example validation
    test_config = {
        "product_types": ["perpetual_futures", "invalid_type"],
        "environment": "testnet",
    }
    
    validation_errors = validate_configuration(test_config)
    print(f"\nConfiguration validation errors: {validation_errors}")


if __name__ == "__main__":
    """Run all Delta Exchange constants examples."""
    print("Delta Exchange Constants Usage Examples")
    print("=" * 50)
    
    venue_and_client_examples()
    url_and_environment_examples()
    enumeration_examples()
    data_model_mapping_examples()
    websocket_channel_examples()
    validation_examples()
    configuration_examples()
    error_handling_examples()
    supported_types_examples()
    comprehensive_constants_examples()
    
    print("\nAll examples completed successfully!")
