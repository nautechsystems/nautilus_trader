#!/usr/bin/env python3
"""
Delta Exchange Instrument Provider Examples

This module demonstrates various usage patterns for the Delta Exchange instrument provider,
including different configuration options, filtering strategies, and caching mechanisms.
"""

import asyncio
import os
from decimal import Decimal

from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeInstrumentProviderConfig
from nautilus_trader.adapters.delta_exchange.providers import DeltaExchangeInstrumentProvider
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.identifiers import InstrumentId, Symbol
from nautilus_trader.model.instruments import CryptoPerpetual, CryptoOption


async def basic_provider_example():
    """
    Basic instrument provider usage example.
    
    This example shows how to create and use the instrument provider
    with default configuration settings.
    """
    print("=== Basic Provider Example ===")
    
    # Create HTTP client (this would be properly configured in real usage)
    client = nautilus_pyo3.DeltaExchangeHttpClient(
        api_key=os.environ.get("DELTA_EXCHANGE_API_KEY", "demo_key"),
        api_secret=os.environ.get("DELTA_EXCHANGE_API_SECRET", "demo_secret"),
        base_url="https://api.delta.exchange",
        timeout_secs=30,
    )
    
    # Create clock
    clock = LiveClock()
    
    # Create provider with default configuration
    provider = DeltaExchangeInstrumentProvider(
        client=client,
        clock=clock,
    )
    
    try:
        # Load all instruments
        print("Loading all instruments...")
        await provider.load_all_async()
        
        # Display results
        instruments = provider.list_all()
        print(f"Loaded {len(instruments)} instruments")
        
        # Show statistics
        stats = provider.stats
        print(f"Statistics: {stats}")
        
        # Show some example instruments
        for i, instrument in enumerate(instruments[:5]):
            print(f"  {i+1}. {instrument.id} ({type(instrument).__name__})")
            
    except Exception as e:
        print(f"Error: {e}")
    
    print()


async def filtered_provider_example():
    """
    Filtered instrument provider example.
    
    This example shows how to load only specific types of instruments
    using product type and symbol filters.
    """
    print("=== Filtered Provider Example ===")
    
    # Create HTTP client
    client = nautilus_pyo3.DeltaExchangeHttpClient(
        api_key=os.environ.get("DELTA_EXCHANGE_API_KEY", "demo_key"),
        api_secret=os.environ.get("DELTA_EXCHANGE_API_SECRET", "demo_secret"),
        base_url="https://api.delta.exchange",
        timeout_secs=30,
    )
    
    clock = LiveClock()
    
    # Configure to load only BTC and ETH perpetual futures
    config = DeltaExchangeInstrumentProviderConfig(
        product_types=["perpetual_futures"],
        symbol_filters=["BTC*", "ETH*"],
        load_active_only=True,
        log_instrument_loading=True,
    )
    
    provider = DeltaExchangeInstrumentProvider(
        client=client,
        clock=clock,
        config=config,
    )
    
    try:
        print("Loading filtered instruments (BTC/ETH perpetuals only)...")
        await provider.load_all_async()
        
        instruments = provider.list_all()
        print(f"Loaded {len(instruments)} filtered instruments")
        
        # Show perpetual futures
        perpetuals = [i for i in instruments if isinstance(i, CryptoPerpetual)]
        print(f"Perpetual futures: {len(perpetuals)}")
        
        for perp in perpetuals[:3]:
            print(f"  - {perp.id}: {perp.base_currency}/{perp.quote_currency}")
            print(f"    Price increment: {perp.price_increment}")
            print(f"    Size increment: {perp.size_increment}")
            print(f"    Multiplier: {perp.multiplier}")
            
    except Exception as e:
        print(f"Error: {e}")
    
    print()


async def options_provider_example():
    """
    Options instrument provider example.
    
    This example shows how to load and work with Delta Exchange options.
    """
    print("=== Options Provider Example ===")
    
    # Create HTTP client
    client = nautilus_pyo3.DeltaExchangeHttpClient(
        api_key=os.environ.get("DELTA_EXCHANGE_API_KEY", "demo_key"),
        api_secret=os.environ.get("DELTA_EXCHANGE_API_SECRET", "demo_secret"),
        base_url="https://api.delta.exchange",
        timeout_secs=30,
    )
    
    clock = LiveClock()
    
    # Configure to load only BTC options
    config = DeltaExchangeInstrumentProviderConfig(
        product_types=["call_options", "put_options"],
        symbol_filters=["BTC*"],
        load_active_only=True,
        load_expired=False,  # Only active options
        log_instrument_loading=True,
    )
    
    provider = DeltaExchangeInstrumentProvider(
        client=client,
        clock=clock,
        config=config,
    )
    
    try:
        print("Loading BTC options...")
        await provider.load_all_async()
        
        instruments = provider.list_all()
        options = [i for i in instruments if isinstance(i, CryptoOption)]
        
        print(f"Loaded {len(options)} BTC options")
        
        # Group by option kind
        calls = [opt for opt in options if opt.option_kind.name == "CALL"]
        puts = [opt for opt in options if opt.option_kind.name == "PUT"]
        
        print(f"  Calls: {len(calls)}")
        print(f"  Puts: {len(puts)}")
        
        # Show some call options
        print("\nSample call options:")
        for call in calls[:3]:
            print(f"  - {call.id}")
            print(f"    Strike: {call.strike_price}")
            print(f"    Expiry: {call.expiry_ns}")
            print(f"    Underlying: {call.underlying}")
            
    except Exception as e:
        print(f"Error: {e}")
    
    print()


async def cached_provider_example():
    """
    Cached instrument provider example.
    
    This example shows how to use caching to improve performance
    and reduce API calls.
    """
    print("=== Cached Provider Example ===")
    
    # Create HTTP client
    client = nautilus_pyo3.DeltaExchangeHttpClient(
        api_key=os.environ.get("DELTA_EXCHANGE_API_KEY", "demo_key"),
        api_secret=os.environ.get("DELTA_EXCHANGE_API_SECRET", "demo_secret"),
        base_url="https://api.delta.exchange",
        timeout_secs=30,
    )
    
    clock = LiveClock()
    
    # Configure with caching enabled
    config = DeltaExchangeInstrumentProviderConfig(
        enable_instrument_caching=True,
        cache_validity_hours=12,  # Cache valid for 12 hours
        cache_directory="/tmp/nautilus_cache",
        cache_file_prefix="delta_instruments_example",
        log_instrument_loading=True,
    )
    
    provider = DeltaExchangeInstrumentProvider(
        client=client,
        clock=clock,
        config=config,
    )
    
    try:
        print("First load (will hit API and cache results)...")
        await provider.load_all_async()
        
        stats1 = provider.stats
        print(f"First load stats: {stats1}")
        print(f"Cache file: {config.get_cache_file_path()}")
        print(f"Cache valid: {config.is_cache_valid()}")
        
        # Create new provider instance to test cache loading
        provider2 = DeltaExchangeInstrumentProvider(
            client=client,
            clock=clock,
            config=config,
        )
        
        print("\nSecond load (should hit cache)...")
        await provider2.load_all_async()
        
        stats2 = provider2.stats
        print(f"Second load stats: {stats2}")
        
        # Compare results
        instruments1 = provider.list_all()
        instruments2 = provider2.list_all()
        
        print(f"First provider loaded: {len(instruments1)} instruments")
        print(f"Second provider loaded: {len(instruments2)} instruments")
        print(f"Results match: {len(instruments1) == len(instruments2)}")
        
    except Exception as e:
        print(f"Error: {e}")
    
    print()


async def specific_instruments_example():
    """
    Specific instruments loading example.
    
    This example shows how to load only specific instruments by ID.
    """
    print("=== Specific Instruments Example ===")
    
    # Create HTTP client
    client = nautilus_pyo3.DeltaExchangeHttpClient(
        api_key=os.environ.get("DELTA_EXCHANGE_API_KEY", "demo_key"),
        api_secret=os.environ.get("DELTA_EXCHANGE_API_SECRET", "demo_secret"),
        base_url="https://api.delta.exchange",
        timeout_secs=30,
    )
    
    clock = LiveClock()
    
    # Configure provider
    config = DeltaExchangeInstrumentProviderConfig(
        enable_instrument_caching=False,
        log_instrument_loading=True,
    )
    
    provider = DeltaExchangeInstrumentProvider(
        client=client,
        clock=clock,
        config=config,
    )
    
    try:
        # Define specific instruments to load
        from nautilus_trader.adapters.delta_exchange.constants import DELTA_EXCHANGE
        
        instrument_ids = [
            InstrumentId(Symbol("BTCUSD"), DELTA_EXCHANGE),
            InstrumentId(Symbol("ETHUSD"), DELTA_EXCHANGE),
            InstrumentId(Symbol("SOLUSD"), DELTA_EXCHANGE),
        ]
        
        print(f"Loading {len(instrument_ids)} specific instruments...")
        await provider.load_ids_async(instrument_ids)
        
        instruments = provider.list_all()
        print(f"Successfully loaded {len(instruments)} instruments")
        
        for instrument in instruments:
            print(f"  - {instrument.id}")
            print(f"    Type: {type(instrument).__name__}")
            print(f"    Price precision: {instrument.price_precision}")
            print(f"    Size precision: {instrument.size_precision}")
            
        # Load single instrument
        print("\nLoading single instrument...")
        single_provider = DeltaExchangeInstrumentProvider(client, clock, config)
        
        btc_id = InstrumentId(Symbol("BTCUSD"), DELTA_EXCHANGE)
        await single_provider.load_async(btc_id)
        
        single_instruments = single_provider.list_all()
        print(f"Single load result: {len(single_instruments)} instrument(s)")
        
        if single_instruments:
            btc_instrument = single_instruments[0]
            print(f"  {btc_instrument.id}: {type(btc_instrument).__name__}")
            
    except Exception as e:
        print(f"Error: {e}")
    
    print()


async def performance_monitoring_example():
    """
    Performance monitoring example.
    
    This example shows how to monitor provider performance and statistics.
    """
    print("=== Performance Monitoring Example ===")
    
    # Create HTTP client
    client = nautilus_pyo3.DeltaExchangeHttpClient(
        api_key=os.environ.get("DELTA_EXCHANGE_API_KEY", "demo_key"),
        api_secret=os.environ.get("DELTA_EXCHANGE_API_SECRET", "demo_secret"),
        base_url="https://api.delta.exchange",
        timeout_secs=30,
    )
    
    clock = LiveClock()
    
    # Configure with performance settings
    config = DeltaExchangeInstrumentProviderConfig(
        max_concurrent_requests=3,  # Limit concurrent requests
        request_delay_ms=200,       # Add delay between requests
        log_instrument_loading=True,
    )
    
    provider = DeltaExchangeInstrumentProvider(
        client=client,
        clock=clock,
        config=config,
    )
    
    try:
        import time
        
        print("Loading instruments with performance monitoring...")
        start_time = time.time()
        
        await provider.load_all_async()
        
        end_time = time.time()
        load_time = end_time - start_time
        
        # Display performance metrics
        stats = provider.stats
        instruments = provider.list_all()
        
        print(f"\nPerformance Results:")
        print(f"  Load time: {load_time:.2f} seconds")
        print(f"  Instruments loaded: {len(instruments)}")
        print(f"  Load rate: {len(instruments) / load_time:.1f} instruments/second")
        print(f"  API requests: {stats['api_requests']}")
        print(f"  Errors: {stats['errors']}")
        print(f"  Cache hits: {stats['cache_hits']}")
        print(f"  Cache misses: {stats['cache_misses']}")
        print(f"  Filtered out: {stats['filtered_out']}")
        
        # Show configuration impact
        print("\nConfiguration:")
        print(f"  Max concurrent requests: {config.max_concurrent_requests}")
        print(f"  Request delay: {config.request_delay_ms}ms")
        print(f"  Cache enabled: {config.enable_instrument_caching}")
        
    except Exception as e:
        print(f"Error: {e}")
    
    print()


async def main():
    """Run all provider examples."""
    print("Delta Exchange Instrument Provider Examples")
    print("=" * 50)
    print()
    
    # Note: These examples require valid API credentials
    print("Note: These examples require valid Delta Exchange API credentials.")
    print("Set DELTA_EXCHANGE_API_KEY and DELTA_EXCHANGE_API_SECRET environment variables.")
    print()
    
    try:
        await basic_provider_example()
        await filtered_provider_example()
        await options_provider_example()
        await cached_provider_example()
        await specific_instruments_example()
        await performance_monitoring_example()
        
    except Exception as e:
        print(f"Example execution failed: {e}")
        print("This is likely due to missing API credentials or network issues.")
    
    print("All examples completed!")
    print()
    print("Next steps:")
    print("1. Set up your Delta Exchange API credentials")
    print("2. Choose appropriate configuration for your use case")
    print("3. Implement error handling and retry logic")
    print("4. Monitor performance and adjust settings as needed")


if __name__ == "__main__":
    asyncio.run(main())
