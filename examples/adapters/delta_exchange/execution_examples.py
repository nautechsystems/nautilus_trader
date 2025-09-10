#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
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
Delta Exchange Execution Client Examples.

This module demonstrates how to use the DeltaExchangeExecutionClient for various
trading operations including order management, position tracking, risk management,
and real-time execution updates.
"""

import asyncio
import logging
from decimal import Decimal

from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeExecClientConfig
from nautilus_trader.adapters.delta_exchange.constants import DELTA_EXCHANGE
from nautilus_trader.adapters.delta_exchange.execution import DeltaExchangeExecutionClient
from nautilus_trader.adapters.delta_exchange.providers import DeltaExchangeInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock, MessageBus
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.enums import OrderSide, OrderType, TimeInForce
from nautilus_trader.model.identifiers import ClientOrderId, InstrumentId, Symbol, TraderId, StrategyId
from nautilus_trader.model.objects import Price, Quantity
from nautilus_trader.model.orders import LimitOrder, MarketOrder, StopMarketOrder, OrderList
from nautilus_trader.test_kit.mocks import MockMessageBus


# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s - %(name)s - %(levelname)s - %(message)s"
)


async def basic_execution_client_example():
    """
    Demonstrate basic Delta Exchange execution client usage.
    
    This example shows how to:
    1. Create and configure an execution client
    2. Connect to Delta Exchange WebSocket
    3. Submit orders and manage positions
    4. Handle real-time execution updates
    """
    print("=== Basic Execution Client Example ===")
    
    # Create configuration for testnet
    config = DeltaExchangeExecClientConfig.testnet(
        api_key="your_testnet_api_key",
        api_secret="your_testnet_api_secret",
        account_id="test_account",
        max_retries=3,
        retry_delay_secs=1.0,
        position_limits={"BTCUSDT": Decimal("10.0")},
        daily_loss_limit=Decimal("1000.0"),
        max_position_value=Decimal("50000.0"),
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
    
    # Create execution client
    exec_client = DeltaExchangeExecutionClient(
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
        await exec_client._connect()
        print("Connected to Delta Exchange execution WebSocket")
        
        # Get a test instrument
        instruments = instrument_provider.list_all()
        if instruments:
            test_instrument = instruments[0]
            print(f"Using test instrument: {test_instrument.id}")
            
            # Create a limit order
            limit_order = LimitOrder(
                trader_id=TraderId("TRADER-001"),
                strategy_id=StrategyId("STRATEGY-001"),
                instrument_id=test_instrument.id,
                client_order_id=ClientOrderId("LIMIT-001"),
                order_side=OrderSide.BUY,
                quantity=Quantity.from_str("0.1"),
                price=Price.from_str("45000.00"),
                time_in_force=TimeInForce.GTC,
                init_id=UUID4(),
                ts_init=clock.timestamp_ns(),
            )
            
            # Submit the order
            await exec_client._submit_order(limit_order)
            print(f"Submitted limit order: {limit_order.client_order_id}")
            
            # Wait for order updates
            await asyncio.sleep(5)
            
            # Create a market order
            market_order = MarketOrder(
                trader_id=TraderId("TRADER-001"),
                strategy_id=StrategyId("STRATEGY-001"),
                instrument_id=test_instrument.id,
                client_order_id=ClientOrderId("MARKET-001"),
                order_side=OrderSide.SELL,
                quantity=Quantity.from_str("0.05"),
                time_in_force=TimeInForce.IOC,
                init_id=UUID4(),
                ts_init=clock.timestamp_ns(),
            )
            
            # Submit the market order
            await exec_client._submit_order(market_order)
            print(f"Submitted market order: {market_order.client_order_id}")
            
            # Wait for execution
            await asyncio.sleep(5)
            
            # Cancel the limit order if still open
            if limit_order.venue_order_id:
                await exec_client._cancel_order(limit_order)
                print(f"Cancelled limit order: {limit_order.client_order_id}")
            
        # Print statistics
        stats = exec_client.stats
        print("\nExecution Client Statistics:")
        for key, value in stats.items():
            print(f"  {key}: {value:,}")
            
    except KeyboardInterrupt:
        print("\nShutting down...")
    finally:
        await exec_client._disconnect()
        print("Disconnected from Delta Exchange")


async def advanced_order_management_example():
    """
    Demonstrate advanced order management features.
    
    This example shows how to:
    1. Submit order lists (batch orders)
    2. Use different order types
    3. Implement risk management
    4. Handle order modifications
    """
    print("\n=== Advanced Order Management Example ===")
    
    # Create configuration with advanced risk settings
    config = DeltaExchangeExecClientConfig(
        api_key="your_api_key",
        api_secret="your_api_secret",
        testnet=False,  # Production
        account_id="prod_account",
        max_retries=5,
        retry_delay_secs=2.0,
        position_limits={
            "BTCUSDT": Decimal("5.0"),
            "ETHUSDT": Decimal("50.0"),
            "SOLUSDT": Decimal("1000.0"),
        },
        order_size_limits={
            "BTCUSDT": (Decimal("0.001"), Decimal("1.0")),
            "ETHUSDT": (Decimal("0.01"), Decimal("10.0")),
        },
        daily_loss_limit=Decimal("5000.0"),
        max_position_value=Decimal("100000.0"),
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
    
    # Create execution client
    exec_client = DeltaExchangeExecutionClient(
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
        await exec_client._connect()
        print("Connected to Delta Exchange (Production)")
        
        # Get instruments
        btc_instrument = None
        eth_instrument = None
        
        for instrument in instrument_provider.list_all():
            if instrument.id.symbol.value == "BTCUSDT":
                btc_instrument = instrument
            elif instrument.id.symbol.value == "ETHUSDT":
                eth_instrument = instrument
        
        if not btc_instrument or not eth_instrument:
            print("Required instruments not found")
            return
        
        # Create multiple orders for batch submission
        orders = []
        
        # BTC limit orders
        btc_buy_order = LimitOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("STRATEGY-001"),
            instrument_id=btc_instrument.id,
            client_order_id=ClientOrderId("BTC-BUY-001"),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("0.1"),
            price=Price.from_str("45000.00"),
            time_in_force=TimeInForce.GTC,
            init_id=UUID4(),
            ts_init=clock.timestamp_ns(),
        )
        orders.append(btc_buy_order)
        
        btc_sell_order = LimitOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("STRATEGY-001"),
            instrument_id=btc_instrument.id,
            client_order_id=ClientOrderId("BTC-SELL-001"),
            order_side=OrderSide.SELL,
            quantity=Quantity.from_str("0.1"),
            price=Price.from_str("55000.00"),
            time_in_force=TimeInForce.GTC,
            init_id=UUID4(),
            ts_init=clock.timestamp_ns(),
        )
        orders.append(btc_sell_order)
        
        # ETH limit orders
        eth_buy_order = LimitOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("STRATEGY-001"),
            instrument_id=eth_instrument.id,
            client_order_id=ClientOrderId("ETH-BUY-001"),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("1.0"),
            price=Price.from_str("3000.00"),
            time_in_force=TimeInForce.GTC,
            init_id=UUID4(),
            ts_init=clock.timestamp_ns(),
        )
        orders.append(eth_buy_order)
        
        # Create order list
        order_list = OrderList(
            order_list_id=TestStubs.order_list_id(),
            orders=orders,
        )
        
        # Submit order list
        await exec_client._submit_order_list(order_list)
        print(f"Submitted order list with {len(orders)} orders")
        
        # Wait for order updates
        await asyncio.sleep(10)
        
        # Create stop-loss order
        stop_order = StopMarketOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("STRATEGY-001"),
            instrument_id=btc_instrument.id,
            client_order_id=ClientOrderId("BTC-STOP-001"),
            order_side=OrderSide.SELL,
            quantity=Quantity.from_str("0.05"),
            trigger_price=Price.from_str("44000.00"),
            time_in_force=TimeInForce.GTC,
            init_id=UUID4(),
            ts_init=clock.timestamp_ns(),
        )
        
        # Submit stop order
        await exec_client._submit_order(stop_order)
        print(f"Submitted stop order: {stop_order.client_order_id}")
        
        # Wait for more updates
        await asyncio.sleep(10)
        
        # Modify an order (if it has venue order ID)
        if btc_buy_order.venue_order_id:
            new_price = Price.from_str("44500.00")
            await exec_client._modify_order(
                btc_buy_order,
                price=new_price,
            )
            print(f"Modified order {btc_buy_order.client_order_id} price to {new_price}")
        
        # Cancel all orders for BTC
        await exec_client._cancel_all_orders(btc_instrument.id)
        print(f"Cancelled all orders for {btc_instrument.id}")
        
        # Monitor for a while
        print("Monitoring execution updates... (press Ctrl+C to stop)")
        for i in range(30):  # 30 seconds
            await asyncio.sleep(1)
            
            # Check health every 10 seconds
            if i % 10 == 0:
                health = await exec_client._health_check()
                print(f"Health check: {'OK' if health else 'FAILED'}")
                
                # Log statistics every 15 seconds
                if i % 15 == 0:
                    exec_client._log_statistics()
        
    except KeyboardInterrupt:
        print("\nShutting down...")
    finally:
        await exec_client._disconnect()
        print("Disconnected from Delta Exchange")


async def risk_management_example():
    """
    Demonstrate risk management features.
    
    This example shows how to:
    1. Configure position and order limits
    2. Handle risk check failures
    3. Monitor account state
    4. Implement emergency procedures
    """
    print("\n=== Risk Management Example ===")
    
    # Create configuration with strict risk limits
    config = DeltaExchangeExecClientConfig.testnet(
        api_key="your_testnet_api_key",
        api_secret="your_testnet_api_secret",
        account_id="risk_test",
        position_limits={"BTCUSDT": Decimal("0.1")},  # Very small limit
        order_size_limits={"BTCUSDT": (Decimal("0.001"), Decimal("0.05"))},
        daily_loss_limit=Decimal("100.0"),  # Small loss limit
        max_position_value=Decimal("5000.0"),  # Small position value limit
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
    
    # Create execution client
    exec_client = DeltaExchangeExecutionClient(
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
        await exec_client._connect()
        print("Connected for risk management testing")
        
        # Get BTC instrument
        btc_instrument = None
        for instrument in instrument_provider.list_all():
            if instrument.id.symbol.value == "BTCUSDT":
                btc_instrument = instrument
                break
        
        if not btc_instrument:
            print("BTC instrument not found")
            return
        
        print(f"Testing risk limits for {btc_instrument.id}")
        
        # Test 1: Order size too large
        print("\nTest 1: Order size exceeds maximum limit")
        large_order = LimitOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("RISK-TEST"),
            instrument_id=btc_instrument.id,
            client_order_id=ClientOrderId("LARGE-ORDER"),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("1.0"),  # Exceeds 0.05 limit
            price=Price.from_str("50000.00"),
            time_in_force=TimeInForce.GTC,
            init_id=UUID4(),
            ts_init=clock.timestamp_ns(),
        )
        
        await exec_client._submit_order(large_order)
        print("Large order should be rejected by risk check")
        
        # Test 2: Position value too high
        print("\nTest 2: Position value exceeds maximum limit")
        expensive_order = LimitOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("RISK-TEST"),
            instrument_id=btc_instrument.id,
            client_order_id=ClientOrderId("EXPENSIVE-ORDER"),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("0.01"),
            price=Price.from_str("600000.00"),  # 0.01 * 600000 = 6000 > 5000 limit
            time_in_force=TimeInForce.GTC,
            init_id=UUID4(),
            ts_init=clock.timestamp_ns(),
        )
        
        await exec_client._submit_order(expensive_order)
        print("Expensive order should be rejected by risk check")
        
        # Test 3: Valid order within limits
        print("\nTest 3: Valid order within all limits")
        valid_order = LimitOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("RISK-TEST"),
            instrument_id=btc_instrument.id,
            client_order_id=ClientOrderId("VALID-ORDER"),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("0.01"),
            price=Price.from_str("45000.00"),
            time_in_force=TimeInForce.GTC,
            init_id=UUID4(),
            ts_init=clock.timestamp_ns(),
        )
        
        await exec_client._submit_order(valid_order)
        print("Valid order should be accepted")
        
        # Wait for order processing
        await asyncio.sleep(5)
        
        # Check statistics
        stats = exec_client.stats
        print("\nRisk Management Test Results:")
        print(f"Orders submitted: {stats['orders_submitted']}")
        print(f"Orders rejected: {stats['orders_rejected']}")
        print(f"API calls: {stats['api_calls']}")
        print(f"Errors: {stats['errors']}")
        
    except Exception as e:
        print(f"Risk management test error: {e}")
    finally:
        await exec_client._disconnect()
        print("Disconnected from Delta Exchange")


if __name__ == "__main__":
    """Run all examples."""
    print("Delta Exchange Execution Client Examples")
    print("=" * 50)
    
    # Note: Replace API credentials with your actual credentials
    print("Note: Please replace API credentials with your actual credentials")
    print("      from Delta Exchange before running these examples.")
    print()
    
    # Run examples
    asyncio.run(basic_execution_client_example())
    asyncio.run(advanced_order_management_example())
    asyncio.run(risk_management_example())
    
    print("\nAll examples completed!")
