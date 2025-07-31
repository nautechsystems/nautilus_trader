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

import os
import time
from decimal import Decimal
from typing import Optional

from nautilus_trader.adapters.bybit import BYBIT
from nautilus_trader.adapters.bybit import BybitDataClientConfig
from nautilus_trader.adapters.bybit import BybitProductType
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.book import OrderBook
from nautilus_trader.trading.strategy import Strategy
from nautilus_trader.trading.config import StrategyConfig
from nautilus_trader.adapters.bybit.common.constants import BYBIT_VENUE


class BybitOptionsOrderBookStrategyConfig(StrategyConfig, frozen=True):
    """
    Configuration for the Bybit Options Order Book Test Strategy.
    """
    instrument_id: InstrumentId
    spot_instrument_id: InstrumentId  # Add spot instrument ID
    depth: int = 25  # Options support 25 or 100 levels


class BybitOptionsOrderBookStrategy(Strategy):
    """
    A test strategy that subscribes to Bybit options order book data and spot data, and prints both.
    """
    
    def __init__(self, config: BybitOptionsOrderBookStrategyConfig) -> None:
        super().__init__(config)
        self.book: Optional[OrderBook] = None
        self.quote_count = 0
        self.delta_count = 0
        self.spot_quote_count = 0
        self.last_print_time = 0
        self.last_spot_price: Optional[Decimal] = None
        self.last_options_bid: Optional[Decimal] = None
        self.last_options_ask: Optional[Decimal] = None
        
    def on_start(self) -> None:
        """
        Actions to be performed on strategy start.
        """
        self.log.info("Starting Bybit Options Order Book Test Strategy")
        
        # Print available options first
        self._print_available_options()
        
        # Get the options instrument
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.config.instrument_id}")
            self.log.info("Please check the available instruments above and use a valid instrument ID")
            self.stop()
            return
            
        self.log.info(f"Found options instrument: {self.instrument}")
        
        # Get the spot instrument
        self.spot_instrument = self.cache.instrument(self.config.spot_instrument_id)
        if self.spot_instrument is None:
            self.log.error(f"Could not find spot instrument for {self.config.spot_instrument_id}")
            self.stop()
            return
            
        self.log.info(f"Found spot instrument: {self.spot_instrument}")
        
        # Initialize order book
        self.book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=BookType.L2_MBP,
        )
        
        # Subscribe to options order book deltas
        self.subscribe_order_book_deltas(
            instrument_id=self.config.instrument_id,
            book_type=BookType.L2_MBP,
            depth=self.config.depth,
        )
        
        # Subscribe to options quote ticks
        self.subscribe_quote_ticks(instrument_id=self.config.instrument_id)
        
        # Subscribe to spot quote ticks
        self.subscribe_quote_ticks(instrument_id=self.config.spot_instrument_id)
        
        self.log.info(f"Subscribed to options order book deltas and quote ticks for {self.config.instrument_id}")
        self.log.info(f"Subscribed to spot quote ticks for {self.config.spot_instrument_id}")
        
    def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
        """
        Handle incoming order book deltas.
        """
        if self.book is None:
            self.log.error("No order book initialized")
            return
            
        # Apply deltas to maintain the order book
        self.book.apply_deltas(deltas)
        self.delta_count += 1
        
        # Update latest options bid/ask from order book
        best_bid = self.book.best_bid_price()
        best_ask = self.book.best_ask_price()
        if best_bid:
            self.last_options_bid = best_bid
        if best_ask:
            self.last_options_ask = best_ask
        
        # Print combined info every 5 seconds
        current_time = time.time()
        if current_time - self.last_print_time >= 5.0:
            self._print_combined_info()
            self.last_print_time = current_time
            
    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Handle incoming quote ticks (both options and spot).
        """
        if tick.instrument_id == self.config.instrument_id:
            # Options quote tick
            self.quote_count += 1
            self.last_options_bid = tick.bid_price
            self.last_options_ask = tick.ask_price
                
        elif tick.instrument_id == self.config.spot_instrument_id:
            # Spot quote tick
            self.spot_quote_count += 1
            self.last_spot_price = (tick.bid_price + tick.ask_price) / 2  # Mid price
            
    def _print_combined_info(self) -> None:
        """
        Print combined options and spot information in a clean format.
        """
        # Options info
        options_info = ""
        if self.last_options_bid and self.last_options_ask:
            options_spread = self.last_options_ask.as_double() - self.last_options_bid.as_double()
            options_spread_pct = (options_spread / self.last_options_bid.as_double()) * 100
            options_info = f"Options: Bid={self.last_options_bid} Ask={self.last_options_ask} Spread={options_spread:.0f} ({options_spread_pct:.1f}%)"
        else:
            options_info = "Options: No bid/ask data"
        
        # Spot info
        spot_info = ""
        if self.last_spot_price:
            spot_info = f" | Spot: {self.last_spot_price}"
        else:
            spot_info = " | Spot: No data"
        
        # Order book depth info
        book_info = ""
        if self.book:
            bids = self.book.bids()[:3]  # Top 3 bid levels
            asks = self.book.asks()[:3]  # Top 3 ask levels
            
            if bids and asks:
                # Fix: Call the size() method to get actual values
                bid_str = ", ".join([f"{level.price}({level.size()})" for level in bids])
                ask_str = ", ".join([f"{level.price}({level.size()})" for level in asks])
                book_info = f" | Book: Bids[{bid_str}] Asks[{ask_str}]"
        
        # Combined log message
        self.log.info(
            f"Update #{self.delta_count} | "
            f"{options_info}{spot_info}{book_info}"
        )
            
    def on_stop(self) -> None:
        """
        Actions to be performed on strategy stop.
        """
        self.log.info(
            f"Strategy stopped. "
            f"Processed {self.delta_count} order book deltas, "
            f"{self.quote_count} options quote ticks, and "
            f"{self.spot_quote_count} spot quote ticks."
        )

    def _print_available_options(self) -> None:
        """
        Print available options from the cache.
        """
        # Get all instruments and filter for options
        all_instruments = self.cache.instruments()
        self.log.info(f"Total instruments loaded: {len(all_instruments)}")
        
        # Filter for BTC options specifically
        btc_option_instruments = [
            instrument for instrument in all_instruments 
            if (instrument.id.venue == BYBIT_VENUE and 
                "-OPTION" in str(instrument.id) and
                "BTC" in str(instrument.id))
        ]
        
        self.log.info(f"Found {len(btc_option_instruments)} BTC options instruments")
        
        if len(btc_option_instruments) == 0:
            self.log.warning("No BTC options found in cache")
            return
        
        # Sort BTC options by underlying, expiry, strike, and type
        def sort_key(instrument):
            symbol = str(instrument.id.symbol)
            parts = symbol.split('-')
            if len(parts) >= 4:
                underlying = parts[0]
                expiry = parts[1]
                strike = float(parts[2]) if parts[2].isdigit() else 0
                option_type = parts[3]  # C or P
                return (underlying, expiry, strike, option_type)
            return (symbol, 0, 0, '')
        
        sorted_options = sorted(btc_option_instruments, key=sort_key)
        
        # Group by expiry date
        expiry_groups = {}
        for instrument in sorted_options:
            symbol = str(instrument.id.symbol)
            parts = symbol.split('-')
            if len(parts) >= 4:
                expiry = parts[1]  # e.g., "02AUG25"
                if expiry not in expiry_groups:
                    expiry_groups[expiry] = []
                expiry_groups[expiry].append(instrument)
        
        # Log expiry groups
        self.log.info(f"BTC options grouped by expiry ({len(expiry_groups)} expiry dates):")
        for expiry in sorted(expiry_groups.keys()):
            count = len(expiry_groups[expiry])
            self.log.info(f"  {expiry}: {count} options")
        
        # Show first 100 elements of all options
        self.log.info("First 100 BTC options instruments:")
        for i, instrument in enumerate(sorted_options[:100]):
            self.log.info(f"  {i+1:3d}. {instrument.id}")
        
        # Check if the target instrument exists
        
                        #BTC-1AUG25-117000-C-USDT-OPTION.BYBIT

        target_symbol = "BTC-15AUG25-110000-C-USDT-OPTION.BYBIT"
        target_found = False
        similar_instruments = []
        
        for instrument in sorted_options:
            instrument_str = str(instrument.id)
            if instrument_str == target_symbol:
                target_found = True
                break
            # Check for similar instruments (same expiry and strike)
            if "02AUG25" in instrument_str and "110000" in instrument_str:
                similar_instruments.append(instrument_str)
        
        if target_found:
            self.log.info(f"✓ Target instrument found: {target_symbol}")
        else:
            self.log.error(f"✗ Target instrument NOT found: {target_symbol}")
            
            # Show similar instruments
            if similar_instruments:
                self.log.info(f"Similar instruments with same expiry (02AUG25) and strike (110000):")
                for i, instrument in enumerate(similar_instruments[:10]):
                    self.log.info(f"  {i+1}. {instrument}")
                if len(similar_instruments) > 10:
                    self.log.info(f"  ... and {len(similar_instruments) - 10} more")
            else:
                self.log.info("No instruments found with expiry 02AUG25 and strike 110000")
                
                # Check what expiries are available for 02AUG25
                aug25_instruments = [inst for inst in sorted_options if "02AUG25" in str(inst.id)]
                if aug25_instruments:
                    self.log.info(f"Instruments with expiry 02AUG25 ({len(aug25_instruments)} found):")
                    for i, instrument in enumerate(aug25_instruments[:20]):
                        self.log.info(f"  {i+1}. {instrument.id}")
                    if len(aug25_instruments) > 20:
                        self.log.info(f"  ... and {len(aug25_instruments) - 20} more")
                else:
                    self.log.info("No instruments found with expiry 02AUG25")
                    
                    # Show available expiries
                    available_expiries = sorted(expiry_groups.keys())
                    self.log.info(f"Available expiries: {available_expiries}")


def main():
    """
    Main function to run the Bybit options order book test.
    """
    # Configuration for Bybit options and spot
    product_types = [BybitProductType.OPTION, BybitProductType.SPOT]
    
    # You can change this to any available option symbol
    # Format: {UNDERLYING}-{EXPIRY}-{STRIKE}-{CALL/PUT}-OPTION.BYBIT
    option_symbol = "BTC-15AUG25-110000-C-USDT-OPTION.BYBIT"  # Changed from 02AUG25 to 15AUG25
    
    # Create spot symbol for the underlying asset
    # Extract the underlying from the option symbol (BTC from BTC-15AUG25-110000-C-USDT-OPTION)
    underlying = option_symbol.split('-')[0]  # BTC
    spot_symbol = f"{underlying}USDT-SPOT.BYBIT"
    
    # Define specific instrument IDs to load (only BTC-related)
    load_instrument_ids = [
        InstrumentId.from_str(spot_symbol),  # BTCUSDT-SPOT.BYBIT
        InstrumentId.from_str(option_symbol),  # BTC-29JUL25-110000-C-USDT-OPTION.BYBIT
    ]
    
    # Configure the trading node
    config_node = TradingNodeConfig(
        trader_id=TraderId("OPTIONS-TESTER-001"),
        logging=LoggingConfig(
            log_level="INFO",
            use_pyo3=True,
            log_colors=True,
        ),
        data_clients={
            BYBIT: BybitDataClientConfig(
                api_key=os.getenv("BYBIT_API_KEY"),
                api_secret=os.getenv("BYBIT_API_SECRET"),
                instrument_provider=InstrumentProviderConfig(
                    load_all=True,  # Load all instruments
                    filters={
                        "base_coin": "BTC",  # Filter for BTC base coin only
                    },
                ),
                product_types=product_types,  # Load both options and spot
                testnet=False,  # Use testnet for testing
            ),
        },
        timeout_connection=30.0,
        timeout_reconciliation=10.0,
        timeout_portfolio=10.0,
        timeout_disconnection=10.0,
        timeout_post_stop=5.0,
    )
    
    # Instantiate the node
    node = TradingNode(config=config_node)
    
    # Configure the strategy
    strategy_config = BybitOptionsOrderBookStrategyConfig(
        instrument_id=InstrumentId.from_str(option_symbol),
        spot_instrument_id=InstrumentId.from_str(spot_symbol),  # Add spot instrument
        depth=25,  # Use 25 levels for options
    )
    
    # Instantiate the strategy
    strategy = BybitOptionsOrderBookStrategy(config=strategy_config)
    
    # Add the strategy to the node
    node.trader.add_strategy(strategy)
    
    # Register the data client factory
    from nautilus_trader.adapters.bybit import BybitLiveDataClientFactory
    node.add_data_client_factory(BYBIT, BybitLiveDataClientFactory)
    
    # Build the node
    node.build()
    
    
    # Run the node
    if __name__ == "__main__":
        try:
            print(f"Starting Bybit Options Order Book Test for {option_symbol}")
            print(f"Also monitoring spot price for {spot_symbol}")
            print("Press Ctrl+C to stop...")
            node.run()
        except KeyboardInterrupt:
            print("\nStopping...")
        finally:
            node.dispose()


if __name__ == "__main__":
    main() 