#!/usr/bin/env python3
"""
Delta Exchange Configuration Examples

This module demonstrates various configuration patterns for the Delta Exchange adapter,
including environment-specific setups, risk management configurations, and advanced
customization options.
"""

import os
from decimal import Decimal

from nautilus_trader.adapters.delta_exchange.config import (
    DeltaExchangeDataClientConfig,
    DeltaExchangeExecClientConfig,
    DeltaExchangeInstrumentProviderConfig,
)
from nautilus_trader.model.enums import TimeInForce


def basic_configuration_example():
    """
    Basic configuration using environment variables.
    
    This is the simplest setup that relies on environment variables for credentials.
    Set the following environment variables:
    - DELTA_EXCHANGE_API_KEY
    - DELTA_EXCHANGE_API_SECRET
    """
    print("=== Basic Configuration Example ===")
    
    # Data client configuration
    data_config = DeltaExchangeDataClientConfig()
    print(f"Data client venue: {data_config.venue}")
    print(f"Has credentials: {data_config.has_credentials()}")
    print(f"HTTP URL: {data_config.get_effective_http_url()}")
    print(f"WebSocket URL: {data_config.get_effective_ws_url()}")
    
    # Execution client configuration
    exec_config = DeltaExchangeExecClientConfig()
    print(f"Default time in force: {exec_config.default_time_in_force}")
    print(f"Default leverage: {exec_config.default_leverage}")
    print(f"Margin mode: {exec_config.margin_mode}")
    
    # Instrument provider configuration
    provider_config = DeltaExchangeInstrumentProviderConfig()
    print(f"Product types: {provider_config.get_product_type_filters()}")
    print(f"Cache validity: {provider_config.cache_validity_hours} hours")
    print()


def testnet_configuration_example():
    """
    Testnet configuration for development and testing.
    
    Set the following environment variables for testnet:
    - DELTA_EXCHANGE_TESTNET_API_KEY
    - DELTA_EXCHANGE_TESTNET_API_SECRET
    """
    print("=== Testnet Configuration Example ===")
    
    # Using factory methods for testnet
    data_config = DeltaExchangeDataClientConfig.testnet(
        heartbeat_interval_secs=60,
        default_channels=["v2_ticker", "l2_orderbook"],
        symbol_filters=["BTC*", "ETH*"],
        log_raw_messages=True,  # Enable debug logging for testnet
    )
    
    exec_config = DeltaExchangeExecClientConfig.testnet(
        default_leverage=5.0,
        max_leverage=20.0,
        post_only_default=True,
        log_order_events=True,
    )
    
    provider_config = DeltaExchangeInstrumentProviderConfig.testnet(
        product_types=["perpetual_futures"],
        update_instruments_interval_mins=30,
        cache_validity_hours=12,
    )
    
    print(f"Data config testnet: {data_config.testnet}")
    print(f"Exec config testnet: {exec_config.testnet}")
    print(f"Provider config testnet: {provider_config.testnet}")
    print(f"Data HTTP URL: {data_config.get_effective_http_url()}")
    print()


def production_configuration_example():
    """
    Production configuration with explicit credentials and conservative settings.
    """
    print("=== Production Configuration Example ===")
    
    # Production configuration with explicit credentials
    # In practice, these would come from secure credential management
    api_key = os.environ.get("DELTA_EXCHANGE_API_KEY", "your_production_api_key")
    api_secret = os.environ.get("DELTA_EXCHANGE_API_SECRET", "your_production_api_secret")
    
    data_config = DeltaExchangeDataClientConfig.production(
        api_key=api_key,
        api_secret=api_secret,
        http_timeout_secs=30,
        ws_timeout_secs=20,
        heartbeat_interval_secs=30,
        rate_limit_requests_per_second=50,  # Conservative rate limiting
        auto_reconnect=True,
        max_reconnection_attempts=5,
    )
    
    exec_config = DeltaExchangeExecClientConfig.production(
        api_key=api_key,
        api_secret=api_secret,
        default_time_in_force=TimeInForce.GTC,
        margin_mode="cross",
        default_leverage=1.0,  # Conservative leverage
        max_leverage=10.0,
        max_retries=3,
        retry_delay_initial_ms=2000,
        enable_order_state_reconciliation=True,
        reconciliation_interval_secs=30,
    )
    
    provider_config = DeltaExchangeInstrumentProviderConfig.production(
        api_key=api_key,
        api_secret=api_secret,
        load_active_only=True,
        cache_validity_hours=24,
        enable_auto_refresh=True,
        max_concurrent_requests=3,  # Conservative concurrent requests
    )
    
    print(f"Data config production: {not data_config.testnet and not data_config.sandbox}")
    print(f"Rate limit: {data_config.rate_limit_requests_per_second} req/sec")
    print(f"Max leverage: {exec_config.max_leverage}")
    print()


def risk_management_configuration_example():
    """
    Configuration with comprehensive risk management settings.
    """
    print("=== Risk Management Configuration Example ===")
    
    exec_config = DeltaExchangeExecClientConfig(
        api_key="your_api_key",
        api_secret="your_api_secret_with_sufficient_length",
        # Order size limits
        max_order_size=1000.0,  # Maximum 1000 units per order
        max_position_size=5000.0,  # Maximum 5000 units position
        max_notional_per_order=50000.0,  # Maximum $50,000 per order
        
        # Leverage and margin settings
        margin_mode="isolated",  # Use isolated margin for better risk control
        default_leverage=2.0,
        max_leverage=10.0,
        
        # Order defaults for risk management
        post_only_default=True,  # Default to maker orders
        reduce_only_default=False,
        auto_reduce_only_on_close=True,
        
        # Retry and reconciliation settings
        max_retries=2,  # Limited retries to avoid over-trading
        enable_order_state_reconciliation=True,
        reconciliation_interval_secs=15,  # Frequent reconciliation
        enable_position_reconciliation=True,
        position_reconciliation_interval_secs=10,
        
        # Rate limiting for safety
        rate_limit_requests_per_second=25,  # Conservative rate limiting
        
        # Audit and logging
        log_order_events=True,
        client_order_id_prefix="RISK_MANAGED",
    )
    
    # Validate risk parameters
    try:
        exec_config.validate_risk_parameters()
        print("✓ Risk parameters validation passed")
    except Exception as e:
        print(f"✗ Risk parameters validation failed: {e}")
    
    retry_config = exec_config.get_order_retry_config()
    print(f"Retry config: {retry_config}")
    print(f"Max order size: {exec_config.max_order_size}")
    print(f"Max position size: {exec_config.max_position_size}")
    print(f"Margin mode: {exec_config.margin_mode}")
    print()


def advanced_data_configuration_example():
    """
    Advanced data client configuration with custom channels and filtering.
    """
    print("=== Advanced Data Configuration Example ===")
    
    data_config = DeltaExchangeDataClientConfig(
        api_key="your_api_key",
        api_secret="your_api_secret_with_sufficient_length",
        
        # WebSocket settings
        ws_timeout_secs=45,
        heartbeat_interval_secs=20,
        auto_reconnect=True,
        reconnection_delay_secs=3,
        max_reconnection_attempts=15,
        max_queue_size=50000,  # Large queue for high-frequency data
        
        # Subscription settings
        default_channels=[
            "v2_ticker",
            "l2_orderbook",
            "l2_updates",
            "all_trades",
            "mark_price",
            "funding_rate",
        ],
        symbol_filters=[
            "BTC*",  # All BTC instruments
            "ETH*",  # All ETH instruments
            "*USDT", # All USDT-settled instruments
        ],
        
        # Performance settings
        rate_limit_requests_per_second=75,  # Near maximum for data
        log_raw_messages=False,  # Disable for production performance
    )
    
    print(f"Default channels: {data_config.default_channels}")
    print(f"Symbol filters: {data_config.symbol_filters}")
    print(f"Max queue size: {data_config.max_queue_size}")
    print(f"Heartbeat interval: {data_config.heartbeat_interval_secs}s")
    print()


def instrument_provider_configuration_example():
    """
    Comprehensive instrument provider configuration.
    """
    print("=== Instrument Provider Configuration Example ===")
    
    provider_config = DeltaExchangeInstrumentProviderConfig(
        api_key="your_api_key",
        api_secret="your_api_secret_with_sufficient_length",
        
        # Product filtering
        product_types=["perpetual_futures", "call_options"],
        load_active_only=True,
        load_expired=False,
        
        # Symbol filtering
        symbol_filters=[
            "BTC*",
            "ETH*",
            "SOL*",
        ],
        trading_status_filters=["active", "trading"],
        
        # Caching settings
        cache_validity_hours=12,  # Refresh cache every 12 hours
        enable_instrument_caching=True,
        cache_directory="/tmp/nautilus_cache",
        cache_file_prefix="delta_instruments_prod",
        
        # Update settings
        update_instruments_interval_mins=30,  # Update every 30 minutes
        enable_auto_refresh=True,
        refresh_on_start=True,
        
        # Performance settings
        max_concurrent_requests=8,
        request_delay_ms=50,  # Small delay between requests
        
        # Logging
        log_instrument_loading=True,
    )
    
    print(f"Product types: {provider_config.get_product_type_filters()}")
    print(f"Cache file path: {provider_config.get_cache_file_path()}")
    print(f"Cache valid: {provider_config.is_cache_valid()}")
    
    # Test instrument filtering
    should_load_btc = provider_config.should_load_instrument(
        "BTCUSD", "perpetual_futures", "active"
    )
    should_load_ada = provider_config.should_load_instrument(
        "ADAUSD", "perpetual_futures", "active"
    )
    
    print(f"Should load BTCUSD: {should_load_btc}")
    print(f"Should load ADAUSD: {should_load_ada}")
    print()


def environment_specific_examples():
    """
    Examples of environment-specific configurations.
    """
    print("=== Environment-Specific Examples ===")
    
    # Development environment
    dev_config = DeltaExchangeDataClientConfig.testnet(
        heartbeat_interval_secs=60,
        log_raw_messages=True,
        rate_limit_requests_per_second=25,  # Conservative for development
    )
    
    # Staging environment
    staging_config = DeltaExchangeDataClientConfig.sandbox(
        heartbeat_interval_secs=30,
        log_raw_messages=False,
        rate_limit_requests_per_second=50,
    )
    
    # Production environment
    prod_config = DeltaExchangeDataClientConfig.production(
        heartbeat_interval_secs=20,
        log_raw_messages=False,
        rate_limit_requests_per_second=75,
        auto_reconnect=True,
        max_reconnection_attempts=10,
    )
    
    print(f"Dev environment: testnet={dev_config.testnet}")
    print(f"Staging environment: sandbox={staging_config.sandbox}")
    print(f"Prod environment: production={not prod_config.testnet and not prod_config.sandbox}")
    print()


def configuration_validation_examples():
    """
    Examples of configuration validation and error handling.
    """
    print("=== Configuration Validation Examples ===")
    
    try:
        # This should fail - both testnet and sandbox
        invalid_config = DeltaExchangeDataClientConfig(testnet=True, sandbox=True)
    except Exception as e:
        print(f"✓ Expected validation error: {e}")
    
    try:
        # This should fail - invalid timeout
        invalid_config = DeltaExchangeDataClientConfig(http_timeout_secs=500)
    except Exception as e:
        print(f"✓ Expected validation error: {e}")
    
    try:
        # This should fail - invalid leverage
        invalid_config = DeltaExchangeExecClientConfig(
            default_leverage=50.0,
            max_leverage=10.0
        )
    except Exception as e:
        print(f"✓ Expected validation error: {e}")
    
    try:
        # This should fail - invalid product type
        invalid_config = DeltaExchangeInstrumentProviderConfig(
            product_types=["invalid_product_type"]
        )
    except Exception as e:
        print(f"✓ Expected validation error: {e}")
    
    print()


if __name__ == "__main__":
    """Run all configuration examples."""
    print("Delta Exchange Configuration Examples")
    print("=" * 50)
    print()
    
    basic_configuration_example()
    testnet_configuration_example()
    production_configuration_example()
    risk_management_configuration_example()
    advanced_data_configuration_example()
    instrument_provider_configuration_example()
    environment_specific_examples()
    configuration_validation_examples()
    
    print("All examples completed successfully!")
    print()
    print("Next steps:")
    print("1. Set up your environment variables")
    print("2. Choose appropriate configurations for your use case")
    print("3. Test with testnet before moving to production")
    print("4. Monitor and adjust rate limits and timeouts as needed")
