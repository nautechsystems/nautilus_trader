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

import pickle

from nautilus_trader.common.enums import LogColor
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.trading.strategy import Strategy


class DataContainer:
    """
    Simple container for demonstrating Cache storage of custom objects.
    """

    def __init__(self, name: str):
        self.name = name
        self.xxx = 100
        self.yyy = ["a", "b", "c"]
        self.zzz = {"key1": 1, "key2": 2}

    def __str__(self):
        return f"Container({self.name}, xxx={self.xxx})"


class CacheDemoStrategy(Strategy):

    def __init__(self, bar_type: BarType):
        super().__init__()
        self.bar_type = bar_type
        self.instrument_id = bar_type.instrument_id

        # Trading state
        self.bar_count = 0
        self.order_placed = False  # Flag to ensure, only one order is placed
        self.show_cache_info_at_bar: int | None = None  # Bar number where to output Cache info

    def on_start(self):
        self.subscribe_bars(self.bar_type)

        # =============================================
        # CUSTOM OBJECT STORAGE
        # Cache allows storing and retrieving any arbitrary object using a string key.
        # This is useful for persisting custom data or sharing data between components.
        # =============================================

        self.log.info("=== Custom Object Storage Demo ===", color=LogColor.YELLOW)

        # Store simple object (dictionary)
        # Note: This example uses abstract data to demonstrate Cache storage functionality.
        # The actual content is not important - we're just showing how to store/retrieve data.
        simple_data = {
            "aaa": 123,
            "bbb": "xyz",
            "ccc": [1, 2, 3],
        }

        # Convert dictionary to bytes before storing
        simple_data_key, simple_data_bytes = "simple_data", pickle.dumps(simple_data)
        self.cache.add(simple_data_key, simple_data_bytes)
        self.log.info(f"Stored simple data: {simple_data}")

        # Store complex object (custom class instance)
        # Note: This example shows how to store custom objects.
        # The class structure is kept minimal to focus on the Cache functionality.

        # Create and store complex object
        complex_data = DataContainer("example")
        complex_data_key, complex_data_bytes = "complex_data", pickle.dumps(complex_data)
        self.cache.add(complex_data_key, complex_data_bytes)
        self.log.info(f"Stored complex data: {complex_data}")

        # Retrieve simple object (dictionary)
        # Step 1: Load bytes from cache
        simple_data_bytes = self.cache.get(simple_data_key)
        # Step 2: Deserialize bytes to objects
        simple_retrieved = pickle.loads(simple_data_bytes)  # noqa: S301 (safe pickle usage)
        self.log.info(f"Retrieved simple data: {simple_retrieved}")

        # Retrieve complex object (custom class instance)
        complex_data_bytes = self.cache.get(complex_data_key)
        complex_retrieved = pickle.loads(complex_data_bytes)  # noqa: S301 (safe pickle usage)
        self.log.info(f"Retrieved complex data: {complex_retrieved}")

        # =============================================
        # INSTRUMENT ACCESS
        # Cache provides access to instrument definitions and their properties.
        # Useful for getting instrument details, specifications, and filtering by venue.
        # =============================================

        self.log.info("=== Instrument Operations Demo ===", color=LogColor.YELLOW)

        # Get specific instrument by ID - returns full instrument definition
        # including specifications, tick size, lot size, margins, etc.
        instrument = self.cache.instrument(self.instrument_id)
        self.log.info(f"Single instrument: {instrument}")

        # Get all instruments for a specific venue (exchange)
        # Useful for market scanning or multi-instrument strategies
        venue = self.instrument_id.venue  # Extract venue from instrument ID
        instruments = self.cache.instruments(venue=venue)
        self.log.info("All instruments for venue:")
        for instrument in instruments:
            self.log.info(f"Instrument: {instrument}")

        # =============================================
        # ACCOUNT ACCESS
        # Cache maintains trading account information including balances and states.
        # Provides methods to access account details by venue or ID, useful for:
        # - Checking account balances and margins
        # - Monitoring account state and permissions
        # - Managing multiple accounts across venues
        # =============================================

        self.log.info("=== Account Operations Demo ===", color=LogColor.YELLOW)

        # Get all trading accounts in the system
        # Useful for strategies managing multiple accounts
        accounts = self.cache.accounts()
        self.log.info(f"All trading accounts: {accounts}")

        # Get account for specific venue (exchange)
        # Returns account with balances, margins, and permissions
        account = self.cache.account_for_venue(venue)
        self.log.info(f"Trading account for {venue}: {account}")

    def on_bar(self, bar: Bar):
        """
        Handle new bar events.
        """
        # Count bars
        self.bar_count += 1

        # Place order exactly at bar 100
        if self.bar_count == 100 and not self.order_placed:
            # Prepare values for order
            instrument = self.cache.instrument(self.instrument_id)
            last_price = bar.close
            tick_size = instrument.price_increment
            profit_price = instrument.make_price(
                last_price + (10 * tick_size),
            )  # 10 ticks profit target
            stoploss_price = instrument.make_price(
                last_price - (10 * tick_size),
            )  # 10 ticks stop loss

            # Create BUY MARKET order with PT and SL
            bracket_order_list = self.order_factory.bracket(
                instrument_id=self.instrument_id,
                order_side=OrderSide.BUY,
                quantity=instrument.make_qty(1),  # Trade size: 1 contract
                time_in_force=TimeInForce.GTC,
                tp_price=profit_price,
                sl_trigger_price=stoploss_price,
            )

            # Submit order and remember it
            self.submit_order_list(bracket_order_list)
            self.order_placed = True
            self.log.info(f"Submitted bracket order: {bracket_order_list}", color=LogColor.GREEN)
            return

        # Wait with the Cache demonstration, until 1 bar after position opened
        if self.bar_count != self.show_cache_info_at_bar:
            return

        self.demonstrate_cache_methods()

    def on_position_opened(self, event: PositionOpened):
        """
        Handle position opened event.
        """
        # Log position details
        self.log.info(f"Position opened: {event}", color=LogColor.GREEN)

        # Set target bar number for Cache demonstration
        self.show_cache_info_at_bar = (
            self.bar_count + 1
        )  # Show Cache details one bar after position opens

    def on_stop(self):
        self.log.info("Strategy stopped.", color=LogColor.BLUE)

    def demonstrate_cache_methods(self):
        """
        Demonstrate various Cache methods for market data, orders, and trading
        operations.
        """
        self.log.info(
            f"\nExecuting Cache demonstrations at bar {self.bar_count}",
            color=LogColor.CYAN,
        )

        # =============================================
        # MARKET DATA ACCESS
        # Cache maintains historical market data with methods for:
        # - Accessing latest and historical bars/ticks/quotes
        # - Checking data availability and counts
        # - Managing different data types (bars, ticks, quotes, order books)
        # Similar methods exist for Ticks (trade_tick/quote_tick) and OrderBook
        # =============================================

        self.log.info("=== Market Data Operations Demo ===", color=LogColor.YELLOW)

        # Get and show bars from cache
        bars = self.cache.bars(self.bar_type)
        self.log.info("Bars in cache:")
        self.log.info(f"Total bars: {len(bars)}")

        # Show latest bars
        last_bar = self.cache.bar(self.bar_type)
        previous_bar = self.cache.bar(self.bar_type, index=1)
        self.log.info(f"Last bar:  {last_bar}")
        self.log.info(f"Previous bar: {previous_bar}")

        # Show data availability
        has_bars = self.cache.has_bars(self.bar_type)
        bar_count = self.cache.bar_count(self.bar_type)
        self.log.info(f"Bars available: {has_bars}, Count: {bar_count}")

        # =============================================
        # ORDER MANAGEMENT
        # Cache provides comprehensive order tracking with:
        # - Filtering by venue/instrument/strategy/state
        # - Order state monitoring (open/closed/emulated)
        # - Order statistics and counts
        # - Parent/child order relationships
        # =============================================

        self.log.info("=== Order Management Demo ===", color=LogColor.YELLOW)

        # Query orders with different filters to monitor trading activity
        # Get complete list of orders for detailed inspection
        all_orders = self.cache.orders()  # Complete order history
        open_orders = self.cache.orders_open()  # Pending execution orders
        closed_orders = self.cache.orders_closed()  # Completed orders

        # Log detailed order information
        self.log.info("Order details from Cache:")

        # Show all orders
        self.log.info("All orders:", color=LogColor.BLUE)
        for order in all_orders:
            self.log.info(f"{order}")

        # Show currently open orders
        self.log.info("Open orders:", color=LogColor.BLUE)
        for order in open_orders:
            self.log.info(f"{order}")

        # Show completed orders
        self.log.info("Closed orders:", color=LogColor.BLUE)
        for order in closed_orders:
            self.log.info(f"{order}")

        # Get order counts directly (more efficient than len() when only count is needed)
        total = self.cache.orders_total_count()
        open_count = self.cache.orders_open_count()
        closed_count = self.cache.orders_closed_count()
        self.log.info(f"Order counts - Total: {total}, Open: {open_count}, Closed: {closed_count}")

        # =============================================
        # POSITION MANAGEMENT
        # Cache tracks all positions with functionality for:
        # - Position filtering by venue/instrument/strategy
        # - State monitoring (open/closed)
        # - Position metrics and statistics
        # - Historical position analysis
        # =============================================

        self.log.info("=== Position Management Demo ===", color=LogColor.YELLOW)

        # Query positions with different filters
        # Get complete list of positions for detailed inspection
        all_positions = self.cache.positions()  # All positions
        open_positions = self.cache.positions_open()  # Active positions
        closed_positions = self.cache.positions_closed()  # Completed positions

        # Show all positions
        self.log.info("All positions:", color=LogColor.BLUE)
        for pos in all_positions:
            self.log.info(f"{pos}")

        # Show currently open positions
        self.log.info("Open positions:", color=LogColor.BLUE)
        for pos in open_positions:
            self.log.info(f"{pos}")

        # Show completed positions
        self.log.info("\nClosed positions:", color=LogColor.BLUE)
        for pos in closed_positions:
            self.log.info(f"{pos}")

        # Get position counts and show statistics
        total = self.cache.positions_total_count()
        open_count = self.cache.positions_open_count()
        closed_count = self.cache.positions_closed_count()
        self.log.info(
            f"Position counts - Total: {total}, Open: {open_count}, Closed: {closed_count}",
        )

        # =============================================
        # SYSTEM STATE ACCESS
        # Cache provides access to various system identifiers and collections.
        # Useful for querying system state and finding specific objects.
        # Includes methods for accessing different types of IDs.
        # =============================================

        self.log.info("=== System State Access Demo ===", color=LogColor.YELLOW)

        # Show active strategies
        strategy_ids = self.cache.strategy_ids()
        self.log.info(f"Active Strategies: {strategy_ids}")

        # Show actor IDs
        actor_ids = self.cache.actor_ids()
        self.log.info(f"Active Actors: {actor_ids}")
