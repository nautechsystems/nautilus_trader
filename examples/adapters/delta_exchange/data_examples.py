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
Delta Exchange Data Client Examples.

This module demonstrates how to use the DeltaExchangeDataClient for various
market data operations including real-time subscriptions, historical data
requests, and configuration management.
"""

import asyncio
import logging
from decimal import Decimal

from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeDataClientConfig
from nautilus_trader.adapters.delta_exchange.constants import DELTA_EXCHANGE
from nautilus_trader.adapters.delta_exchange.data import DeltaExchangeDataClient
from nautilus_trader.adapters.delta_exchange.providers import DeltaExchangeInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock, MessageBus
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AggregateSide, BarAggregation, BookType, PriceType
from nautilus_trader.model.identifiers import InstrumentId, Symbol
from nautilus_trader.test_kit.mocks import MockMessageBus


# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s - %(name)s - %(levelname)s - %(message)s"
)


async def basic_data_client_example():
    """
    Demonstrate basic Delta Exchange data client usage.
    
    This example shows how to:
    1. Create and configure a data client
    2. Connect to Delta Exchange WebSocket
    3. Subscribe to market data
    4. Handle real-time data updates
    """
    print("=== Basic Data Client Example ===")
    
    # Create configuration for testnet
    config = DeltaExchangeDataClientConfig.testnet(
        api_key="your_testnet_api_key",
        api_secret="your_testnet_api_secret",
        default_channels=["v2_ticker", "all_trades"],
        symbol_filters=["BTC*", "ETH*"],  # Only BTC and ETH instruments
        ws_timeout_secs=30,
        heartbeat_interval_secs=20,
    )
    
    # Create components
    loop = asyncio.get_event_loop()
    clock = LiveClock()
    msgbus = MockMessageBus()
    cache = Cache()
    
    # Create HTTP client
    http_client = nautilus_pyo3.DeltaExchangeHttpClient(
        api_key=config.get_effective_api_key(),
        api_secret=config.get_effective_api_secret(),
        base_url=config.get_effective_http_url(),
        timeout_secs=config.http_timeout_secs,
    )
    
    # Create instrument provider
    instrument_provider = DeltaExchangeInstrumentProvider(
        client=http_client,
        clock=clock,
        config=config.instrument_provider,
    )
    
    # Load instruments
    await instrument_provider.load_all_async()
    print(f"Loaded {len(instrument_provider.list_all())} instruments")
    
    # Create data client
    data_client = DeltaExchangeDataClient(
        loop=loop,
        client=http_client,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        instrument_provider=instrument_provider,
        config=config,
    )
    
    try:
        # Connect to Delta Exchange
        await data_client._connect()
        print("Connected to Delta Exchange WebSocket")
        
        # Get a test instrument
        instruments = instrument_provider.list_all()
        if instruments:
            test_instrument = instruments[0]
            print(f"Using test instrument: {test_instrument.id}")
            
            # Subscribe to quote ticks (best bid/ask)
            await data_client._subscribe_quote_ticks(test_instrument.id)
            print(f"Subscribed to quote ticks for {test_instrument.id}")
            
            # Subscribe to trade ticks
            await data_client._subscribe_trade_ticks(test_instrument.id)
            print(f"Subscribed to trade ticks for {test_instrument.id}")
            
            # Subscribe to order book
            await data_client._subscribe_order_book_deltas(
                test_instrument.id, 
                BookType.L2_MBP
            )
            print(f"Subscribed to order book for {test_instrument.id}")
            
            # Wait for some data
            print("Waiting for market data... (press Ctrl+C to stop)")
            await asyncio.sleep(30)
            
        # Print statistics
        stats = data_client.stats
        print("\nData Client Statistics:")
        for key, value in stats.items():
            print(f"  {key}: {value:,}")
            
    except KeyboardInterrupt:
        print("\nShutting down...")
    finally:
        await data_client._disconnect()
        print("Disconnected from Delta Exchange")


async def historical_data_example():
    """
    Demonstrate historical data requests.
    
    This example shows how to:
    1. Request historical trade data
    2. Request historical candlestick data
    3. Handle pagination and rate limiting
    """
    print("\n=== Historical Data Example ===")
    
    # Create configuration
    config = DeltaExchangeDataClientConfig.testnet(
        api_key="your_testnet_api_key",
        api_secret="your_testnet_api_secret",
    )
    
    # Create components
    loop = asyncio.get_event_loop()
    clock = LiveClock()
    msgbus = MockMessageBus()
    cache = Cache()
    
    # Create HTTP client
    http_client = nautilus_pyo3.DeltaExchangeHttpClient(
        api_key=config.get_effective_api_key(),
        api_secret=config.get_effective_api_secret(),
        base_url=config.get_effective_http_url(),
        timeout_secs=config.http_timeout_secs,
    )
    
    # Create instrument provider
    instrument_provider = DeltaExchangeInstrumentProvider(
        client=http_client,
        clock=clock,
        config=config.instrument_provider,
    )
    
    # Load instruments
    await instrument_provider.load_all_async()
    
    # Create data client
    data_client = DeltaExchangeDataClient(
        loop=loop,
        client=http_client,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        instrument_provider=instrument_provider,
        config=config,
    )
    
    try:
        # Get a test instrument
        instruments = instrument_provider.list_all()
        if instruments:
            test_instrument = instruments[0]
            print(f"Requesting historical data for: {test_instrument.id}")
            
            # Request historical trades
            correlation_id = msgbus.correlation_id()
            await data_client._request_trade_ticks(
                instrument_id=test_instrument.id,
                limit=100,
                correlation_id=correlation_id,
            )
            print("Requested 100 historical trades")
            
            # Request historical bars (1-hour candles)
            bar_type = BarType(
                instrument_id=test_instrument.id,
                bar_spec=BarSpecification(
                    step=1,
                    aggregation=BarAggregation.HOUR,
                    price_type=PriceType.LAST,
                ),
                aggregation_source=AggregateSide.NO_AGGRESSOR,
            )
            
            correlation_id = msgbus.correlation_id()
            await data_client._request_bars(
                bar_type=bar_type,
                limit=24,  # Last 24 hours
                correlation_id=correlation_id,
            )
            print("Requested 24 1-hour bars")
            
            # Wait for responses
            await asyncio.sleep(5)
            
            # Check responses
            responses = msgbus.sent
            print(f"Received {len(responses)} responses")
            
    except Exception as e:
        print(f"Error: {e}")


async def advanced_subscription_example():
    """
    Demonstrate advanced subscription management.
    
    This example shows how to:
    1. Subscribe to multiple data types
    2. Use symbol filtering
    3. Handle subscription state
    4. Monitor client health
    """
    print("\n=== Advanced Subscription Example ===")
    
    # Create configuration with advanced settings
    config = DeltaExchangeDataClientConfig(
        api_key="your_api_key",
        api_secret="your_api_secret",
        testnet=False,  # Production
        default_channels=["v2_ticker", "mark_price", "funding_rate"],
        symbol_filters=["BTC*", "ETH*", "SOL*"],  # Multiple filters
        ws_timeout_secs=60,
        heartbeat_interval_secs=30,
        max_reconnection_attempts=10,
        reconnection_delay_secs=5.0,
        auto_reconnect=True,
    )
    
    # Create components
    loop = asyncio.get_event_loop()
    clock = LiveClock()
    msgbus = MockMessageBus()
    cache = Cache()
    
    # Create HTTP client
    http_client = nautilus_pyo3.DeltaExchangeHttpClient(
        api_key=config.get_effective_api_key(),
        api_secret=config.get_effective_api_secret(),
        base_url=config.get_effective_http_url(),
        timeout_secs=config.http_timeout_secs,
    )
    
    # Create instrument provider with filtering
    instrument_provider = DeltaExchangeInstrumentProvider(
        client=http_client,
        clock=clock,
        config=config.instrument_provider,
    )
    
    # Load instruments
    await instrument_provider.load_all_async()
    
    # Create data client
    data_client = DeltaExchangeDataClient(
        loop=loop,
        client=http_client,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        instrument_provider=instrument_provider,
        config=config,
    )
    
    try:
        # Connect
        await data_client._connect()
        print("Connected to Delta Exchange (Production)")
        
        # Get filtered instruments
        instruments = [
            instr for instr in instrument_provider.list_all()
            if any(instr.id.symbol.value.startswith(prefix.rstrip('*')) 
                   for prefix in config.symbol_filters)
        ]
        
        print(f"Found {len(instruments)} matching instruments")
        
        # Subscribe to multiple data types for each instrument
        for instrument in instruments[:5]:  # Limit to first 5
            print(f"Subscribing to data for {instrument.id}")
            
            # Quote ticks (best bid/ask)
            await data_client._subscribe_quote_ticks(instrument.id)
            
            # Trade ticks
            await data_client._subscribe_trade_ticks(instrument.id)
            
            # Mark prices (for derivatives)
            if hasattr(instrument, 'is_inverse'):  # Perpetual futures
                await data_client._subscribe_mark_prices(instrument.id)
                await data_client._subscribe_funding_rates(instrument.id)
            
            # Small delay between subscriptions
            await asyncio.sleep(0.1)
        
        # Log subscription state
        data_client._log_subscription_state()
        
        # Monitor for a while
        print("Monitoring subscriptions... (press Ctrl+C to stop)")
        for i in range(60):  # 60 seconds
            await asyncio.sleep(1)
            
            # Check health every 10 seconds
            if i % 10 == 0:
                health = await data_client._health_check()
                print(f"Health check: {'OK' if health else 'FAILED'}")
                
                # Log statistics every 30 seconds
                if i % 30 == 0:
                    data_client._log_statistics()
        
    except KeyboardInterrupt:
        print("\nShutting down...")
    finally:
        await data_client._disconnect()
        print("Disconnected from Delta Exchange")


async def error_handling_example():
    """
    Demonstrate error handling and recovery.
    
    This example shows how to:
    1. Handle connection failures
    2. Implement retry logic
    3. Monitor client health
    4. Recover from errors
    """
    print("\n=== Error Handling Example ===")
    
    # Create configuration with aggressive retry settings
    config = DeltaExchangeDataClientConfig.testnet(
        api_key="invalid_key",  # Intentionally invalid
        api_secret="invalid_secret",
        max_reconnection_attempts=3,
        reconnection_delay_secs=2.0,
        auto_reconnect=True,
    )
    
    # Create components
    loop = asyncio.get_event_loop()
    clock = LiveClock()
    msgbus = MockMessageBus()
    cache = Cache()
    
    # Create HTTP client
    http_client = nautilus_pyo3.DeltaExchangeHttpClient(
        api_key=config.get_effective_api_key(),
        api_secret=config.get_effective_api_secret(),
        base_url=config.get_effective_http_url(),
        timeout_secs=config.http_timeout_secs,
    )
    
    # Create instrument provider
    instrument_provider = DeltaExchangeInstrumentProvider(
        client=http_client,
        clock=clock,
        config=config.instrument_provider,
    )
    
    # Create data client
    data_client = DeltaExchangeDataClient(
        loop=loop,
        client=http_client,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        instrument_provider=instrument_provider,
        config=config,
    )
    
    try:
        # Attempt connection (should fail)
        print("Attempting connection with invalid credentials...")
        await data_client._connect()
        
    except Exception as e:
        print(f"Expected connection failure: {e}")
        
        # Check error statistics
        stats = data_client.stats
        print(f"Connection attempts: {stats['connection_attempts']}")
        print(f"Errors: {stats['errors']}")
        
        # Demonstrate health check failure
        health = await data_client._health_check()
        print(f"Health check result: {'OK' if health else 'FAILED'}")


if __name__ == "__main__":
    """Run all examples."""
    print("Delta Exchange Data Client Examples")
    print("=" * 50)
    
    # Note: Replace API credentials with your actual credentials
    print("Note: Please replace API credentials with your actual credentials")
    print("      from Delta Exchange before running these examples.")
    print()
    
    # Run examples
    asyncio.run(basic_data_client_example())
    asyncio.run(historical_data_example())
    asyncio.run(advanced_subscription_example())
    asyncio.run(error_handling_example())
    
    print("\nAll examples completed!")
