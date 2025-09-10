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
Delta Exchange Factory Examples.

This module demonstrates how to use the Delta Exchange factory classes for
creating clients, managing configurations, and setting up complete trading
systems with proper dependency injection and resource management.
"""

import asyncio
import logging
from decimal import Decimal

from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeDataClientConfig
from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeExecClientConfig
from nautilus_trader.adapters.delta_exchange.constants import DELTA_EXCHANGE
from nautilus_trader.adapters.delta_exchange.factories import (
    DeltaExchangeLiveDataClientFactory,
    DeltaExchangeLiveDataEngineFactory,
    DeltaExchangeLiveExecClientFactory,
    DeltaExchangeLiveExecEngineFactory,
    clear_delta_exchange_caches,
    create_delta_exchange_clients,
    create_production_factories,
    create_testnet_factories,
    get_delta_exchange_factory_info,
    validate_factory_environment,
)
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.test_kit.mocks import MockMessageBus


# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s - %(name)s - %(levelname)s - %(message)s"
)


async def basic_factory_usage_example():
    """
    Demonstrate basic Delta Exchange factory usage.
    
    This example shows how to:
    1. Create individual clients using factories
    2. Use caching for efficient resource management
    3. Handle different environments (testnet, production)
    4. Validate configurations and handle errors
    """
    print("=== Basic Factory Usage Example ===")
    
    # Create components
    loop = asyncio.get_event_loop()
    msgbus = MockMessageBus()
    cache = Cache()
    clock = LiveClock()
    
    # Create testnet data client configuration
    data_config = DeltaExchangeDataClientConfig.testnet(
        api_key="your_testnet_api_key",
        api_secret="your_testnet_api_secret",
        enable_private_channels=True,
        product_types=["perpetual_futures"],
        symbol_patterns=["BTC*", "ETH*"],
    )
    
    try:
        # Create data client using factory
        data_client = DeltaExchangeLiveDataClientFactory.create(
            loop=loop,
            name="DeltaExchange-Data-Testnet",
            config=data_config,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )
        
        print(f"Created data client: {data_client.id}")
        print(f"Venue: {data_client.venue}")
        print(f"Is connected: {data_client.is_connected}")
        
        # Create execution client configuration
        exec_config = DeltaExchangeExecClientConfig.testnet(
            api_key="your_testnet_api_key",
            api_secret="your_testnet_api_secret",
            account_id="testnet_account",
            position_limits={"BTCUSDT": Decimal("1.0")},
            daily_loss_limit=Decimal("1000.0"),
        )
        
        # Create execution client using factory
        exec_client = DeltaExchangeLiveExecClientFactory.create(
            loop=loop,
            name="DeltaExchange-Exec-Testnet",
            config=exec_config,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )
        
        print(f"Created execution client: {exec_client.id}")
        print(f"Account ID: {exec_client.account_id}")
        print(f"Account type: {exec_client.account_type}")
        
        # Demonstrate caching - create another client with same config
        data_client2 = DeltaExchangeLiveDataClientFactory.create(
            loop=loop,
            name="DeltaExchange-Data-Testnet-2",
            config=data_config,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )
        
        print(f"Created second data client: {data_client2.id}")
        
        # Check factory cache information
        cache_info = get_delta_exchange_factory_info()
        print(f"\nFactory Cache Information:")
        print(f"HTTP client cache hits: {cache_info['http_client_cache']['hits']}")
        print(f"HTTP client cache size: {cache_info['http_client_cache']['currsize']}")
        
    except Exception as e:
        print(f"Error creating clients: {e}")


async def advanced_factory_configuration_example():
    """
    Demonstrate advanced factory configuration and management.
    
    This example shows how to:
    1. Use utility functions for client creation
    2. Handle multiple environments
    3. Manage factory caches
    4. Validate factory environment
    """
    print("\n=== Advanced Factory Configuration Example ===")
    
    # Validate factory environment first
    validation_results = validate_factory_environment()
    print("Factory Environment Validation:")
    for component, status in validation_results.items():
        print(f"  {component}: {'✓' if status else '✗'}")
    
    if not all(validation_results.values()):
        print("Warning: Some factory components are not available")
        return
    
    # Create components
    loop = asyncio.get_event_loop()
    msgbus = MockMessageBus()
    cache = Cache()
    clock = LiveClock()
    
    # Create configurations for different environments
    testnet_data_config = DeltaExchangeDataClientConfig.testnet(
        api_key="testnet_key",
        api_secret="testnet_secret",
        enable_private_channels=False,  # Public data only for testing
    )
    
    testnet_exec_config = DeltaExchangeExecClientConfig.testnet(
        api_key="testnet_key",
        api_secret="testnet_secret",
        account_id="testnet_account",
        max_retries=3,
        retry_delay_secs=1.0,
    )
    
    production_data_config = DeltaExchangeDataClientConfig(
        api_key="production_key",
        api_secret="production_secret",
        testnet=False,
        enable_private_channels=True,
        product_types=["perpetual_futures", "call_options", "put_options"],
    )
    
    production_exec_config = DeltaExchangeExecClientConfig(
        api_key="production_key",
        api_secret="production_secret",
        account_id="production_account",
        testnet=False,
        position_limits={
            "BTCUSDT": Decimal("10.0"),
            "ETHUSDT": Decimal("100.0"),
        },
        daily_loss_limit=Decimal("10000.0"),
        max_position_value=Decimal("500000.0"),
    )
    
    try:
        # Create testnet clients using utility function
        print("\nCreating testnet clients...")
        testnet_data_client, testnet_exec_client = create_delta_exchange_clients(
            data_config=testnet_data_config,
            exec_config=testnet_exec_config,
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )
        
        print(f"Testnet data client: {testnet_data_client.id}")
        print(f"Testnet exec client: {testnet_exec_client.id}")
        
        # Create production clients
        print("\nCreating production clients...")
        production_data_client, production_exec_client = create_delta_exchange_clients(
            data_config=production_data_config,
            exec_config=production_exec_config,
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )
        
        print(f"Production data client: {production_data_client.id}")
        print(f"Production exec client: {production_exec_client.id}")
        
        # Show cache statistics
        cache_info = get_delta_exchange_factory_info()
        print(f"\nCache Statistics After Creating Multiple Clients:")
        for cache_type, stats in cache_info.items():
            if isinstance(stats, dict) and "hits" in stats:
                print(f"  {cache_type}:")
                print(f"    Hits: {stats['hits']}")
                print(f"    Misses: {stats['misses']}")
                print(f"    Current size: {stats['currsize']}")
        
        # Clear caches and show the difference
        print("\nClearing factory caches...")
        clear_delta_exchange_caches()
        
        cache_info_after = get_delta_exchange_factory_info()
        print("Cache sizes after clearing:")
        for cache_type, stats in cache_info_after.items():
            if isinstance(stats, dict) and "currsize" in stats:
                print(f"  {cache_type}: {stats['currsize']}")
        
    except Exception as e:
        print(f"Error in advanced configuration: {e}")


async def trading_node_integration_example():
    """
    Demonstrate integration with Nautilus Trader TradingNode.
    
    This example shows how to:
    1. Use engine factories for complete system setup
    2. Register factories with trading nodes
    3. Create comprehensive trading configurations
    4. Handle multiple venues and clients
    """
    print("\n=== Trading Node Integration Example ===")
    
    try:
        # Create engine configurations
        data_engine_config = DeltaExchangeLiveDataEngineFactory.create_config(
            api_key="your_api_key",
            api_secret="your_api_secret",
            testnet=True,
            enable_private_channels=True,
            product_types=["perpetual_futures"],
            symbol_patterns=["BTC*", "ETH*", "SOL*"],
        )
        
        exec_engine_config = DeltaExchangeLiveExecEngineFactory.create_config(
            api_key="your_api_key",
            api_secret="your_api_secret",
            account_id="your_account",
            testnet=True,
            position_limits={
                "BTCUSDT": Decimal("5.0"),
                "ETHUSDT": Decimal("50.0"),
                "SOLUSDT": Decimal("1000.0"),
            },
            daily_loss_limit=Decimal("5000.0"),
        )
        
        print("Created engine configurations:")
        print(f"Data clients: {list(data_engine_config['data_clients'].keys())}")
        print(f"Exec clients: {list(exec_engine_config['exec_clients'].keys())}")
        
        # Create complete trading node configuration
        trading_config = TradingNodeConfig(
            trader_id="TRADER-001",
            data_engine=data_engine_config,
            exec_engine=exec_engine_config,
            cache={},
            message_bus={},
            logging={
                "log_level": "INFO",
                "log_file_format": "json",
            },
        )
        
        print(f"\nCreated trading node configuration for trader: {trading_config.trader_id}")
        
        # Create and configure trading node
        node = TradingNode(config=trading_config)
        
        # Register factories with the node
        DeltaExchangeLiveDataEngineFactory.register_with_node(node)
        DeltaExchangeLiveExecEngineFactory.register_with_node(node)
        
        print("Registered Delta Exchange factories with trading node")
        
        # Build the node (this would normally start all clients)
        # node.build()  # Commented out for example
        print("Trading node ready for building and starting")
        
    except Exception as e:
        print(f"Error in trading node integration: {e}")


async def factory_patterns_and_best_practices_example():
    """
    Demonstrate factory patterns and best practices.
    
    This example shows how to:
    1. Use factory helper functions
    2. Handle different deployment scenarios
    3. Implement proper error handling
    4. Manage resources efficiently
    """
    print("\n=== Factory Patterns and Best Practices Example ===")
    
    # Example 1: Using factory helper functions
    print("1. Using factory helper functions:")
    
    try:
        # Create testnet factories
        testnet_data_factory, testnet_exec_factory = create_testnet_factories(
            api_key="testnet_key",
            api_secret="testnet_secret",
            account_id="testnet_account",
        )
        
        print(f"Testnet factories created: {type(testnet_data_factory).__name__}, {type(testnet_exec_factory).__name__}")
        
        # Create production factories
        production_data_factory, production_exec_factory = create_production_factories(
            api_key="production_key",
            api_secret="production_secret",
            account_id="production_account",
        )
        
        print(f"Production factories created: {type(production_data_factory).__name__}, {type(production_exec_factory).__name__}")
        
    except Exception as e:
        print(f"Error creating factories: {e}")
    
    # Example 2: Configuration validation patterns
    print("\n2. Configuration validation patterns:")
    
    try:
        # Valid configuration
        valid_config = DeltaExchangeDataClientConfig.testnet(
            api_key="valid_key",
            api_secret="valid_secret",
        )
        print("✓ Valid configuration created successfully")
        
        # Invalid configuration (will raise error)
        try:
            invalid_config = DeltaExchangeDataClientConfig(
                testnet=True,
                sandbox=True,  # Invalid: both testnet and sandbox
            )
            print("✗ Invalid configuration should have failed")
        except ValueError as e:
            print(f"✓ Invalid configuration properly rejected: {e}")
        
    except Exception as e:
        print(f"Error in configuration validation: {e}")
    
    # Example 3: Resource management patterns
    print("\n3. Resource management patterns:")
    
    try:
        # Get initial cache state
        initial_info = get_delta_exchange_factory_info()
        print(f"Initial cache sizes: HTTP={initial_info['http_client_cache']['currsize']}")
        
        # Create multiple clients to populate caches
        loop = asyncio.get_event_loop()
        msgbus = MockMessageBus()
        cache = Cache()
        clock = LiveClock()
        
        configs = [
            DeltaExchangeDataClientConfig.testnet(api_key=f"key_{i}", api_secret=f"secret_{i}")
            for i in range(3)
        ]
        
        clients = []
        for i, config in enumerate(configs):
            client = DeltaExchangeLiveDataClientFactory.create(
                loop=loop,
                name=f"client_{i}",
                config=config,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
            )
            clients.append(client)
        
        # Check cache growth
        after_info = get_delta_exchange_factory_info()
        print(f"Cache sizes after creating clients: HTTP={after_info['http_client_cache']['currsize']}")
        
        # Clean up resources
        clear_delta_exchange_caches()
        final_info = get_delta_exchange_factory_info()
        print(f"Cache sizes after cleanup: HTTP={final_info['http_client_cache']['currsize']}")
        
    except Exception as e:
        print(f"Error in resource management: {e}")
    
    # Example 4: Error handling patterns
    print("\n4. Error handling patterns:")
    
    try:
        # Attempt to create client with missing dependencies
        incomplete_config = DeltaExchangeExecClientConfig(
            # Missing required fields
        )
        
        try:
            DeltaExchangeLiveExecClientFactory.create(
                loop=asyncio.get_event_loop(),
                name="incomplete_client",
                config=incomplete_config,
                msgbus=MockMessageBus(),
                cache=Cache(),
                clock=LiveClock(),
            )
            print("✗ Should have failed with incomplete config")
        except RuntimeError as e:
            print(f"✓ Properly handled incomplete config: {e}")
        
    except Exception as e:
        print(f"Error in error handling example: {e}")


if __name__ == "__main__":
    """Run all factory examples."""
    print("Delta Exchange Factory Examples")
    print("=" * 50)
    
    # Note: Replace API credentials with your actual credentials
    print("Note: Please replace API credentials with your actual credentials")
    print("      from Delta Exchange before running these examples.")
    print()
    
    # Run examples
    asyncio.run(basic_factory_usage_example())
    asyncio.run(advanced_factory_configuration_example())
    asyncio.run(trading_node_integration_example())
    asyncio.run(factory_patterns_and_best_practices_example())
    
    print("\nAll factory examples completed!")
