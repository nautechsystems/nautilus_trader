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
    depth: int = 25  # Options support 25 or 100 levels


class BybitOptionsOrderBookStrategy(Strategy):
    """
    A test strategy that subscribes to Bybit options order book data and prints it.
    """
    
    def __init__(self, config: BybitOptionsOrderBookStrategyConfig) -> None:
        super().__init__(config)
        self.book: Optional[OrderBook] = None
        self.quote_count = 0
        self.delta_count = 0
        self.last_print_time = 0
        
    def on_start(self) -> None:
        """
        Actions to be performed on strategy start.
        """
        self.log.info("Starting Bybit Options Order Book Test Strategy")
        
        # Print available options first
        self._print_available_options()
        
        # Get the instrument
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.config.instrument_id}")
            self.stop()
            return
            
        self.log.info(f"Found instrument: {self.instrument}")
        
        # Initialize order book
        self.book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=BookType.L2_MBP,
        )
        
        # Subscribe to order book deltas
        self.subscribe_order_book_deltas(
            instrument_id=self.config.instrument_id,
            book_type=BookType.L2_MBP,
            depth=self.config.depth,
        )
        
        # Also subscribe to quote ticks for comparison
        self.subscribe_quote_ticks(instrument_id=self.config.instrument_id)
        
        self.log.info(f"Subscribed to order book deltas and quote ticks for {self.config.instrument_id}")
        
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
        
        # Print order book info every 5 seconds
        current_time = time.time()
        if current_time - self.last_print_time >= 5.0:
            self._print_order_book_info()
            self.last_print_time = current_time
            
    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Handle incoming quote ticks.
        """
        self.quote_count += 1
        
        # Print quote info every 10 seconds
        current_time = time.time()
        if current_time - self.last_print_time >= 10.0:
            self.log.info(
                f"Quote #{self.quote_count}: "
                f"Bid={tick.bid_price} (size: {tick.bid_size}), "
                f"Ask={tick.ask_price} (size: {tick.ask_size}), "
                f"Spread={tick.ask_price.as_double() - tick.bid_price.as_double():.6f}"
            )
            
    def _print_order_book_info(self) -> None:
        """
        Print current order book information.
        """
        if self.book is None:
            return
            
        best_bid = self.book.best_bid_price()
        best_ask = self.book.best_ask_price()
        best_bid_size = self.book.best_bid_size()
        best_ask_size = self.book.best_ask_size()
        
        if best_bid and best_ask:
            spread = best_ask.as_f64() - best_bid.as_f64()
            spread_pct = (spread / best_bid.as_f64()) * 100
            
            self.log.info(
                f"Order Book #{self.delta_count}: "
                f"Best Bid={best_bid} ({best_bid_size}), "
                f"Best Ask={best_ask} ({best_ask_size}), "
                f"Spread={spread:.6f} ({spread_pct:.2f}%)"
            )
            
            # Get top 5 levels
            bids = self.book.bids_as_map(depth=5)
            asks = self.book.asks_as_map(depth=5)
            
            self.log.info(f"Top 5 Bids: {dict(bids)}")
            self.log.info(f"Top 5 Asks: {dict(asks)}")
            
            # Check if we have a reasonable spread
            if spread_pct > 50:  # More than 50% spread
                self.log.warning(f"Large spread detected: {spread_pct:.2f}%")
        else:
            self.log.warning("No bid/ask prices available in order book")
            
    def on_stop(self) -> None:
        """
        Actions to be performed on strategy stop.
        """
        self.log.info(
            f"Strategy stopped. "
            f"Processed {self.delta_count} order book deltas and {self.quote_count} quote ticks."
        )

    def _print_available_options(self) -> None:
        """
        Print available options from the cache.
        """
        # Get all instruments and filter for options
        all_instruments = self.cache.instruments()
        option_instruments = [
            instrument for instrument in all_instruments 
            if instrument.id.venue == BYBIT_VENUE and "-OPTION" in str(instrument.id)
        ]
        
        if len(option_instruments) == 0:
            self.log.warning("No options found in cache")
            return
        
        # Sort options by underlying, expiry, strike, and type
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
        
        sorted_options = sorted(option_instruments, key=sort_key)


def main():
    """
    Main function to run the Bybit options order book test.
    """
    # Configuration for Bybit options
    product_type = BybitProductType.OPTION
    
    # You can change this to any available option symbol
    # Format: {UNDERLYING}-{EXPIRY}-{STRIKE}-{CALL/PUT}-OPTION.BYBIT
    # Example: ETH-3JAN23-1250-P-OPTION.BYBIT
    # option_symbol = "ETH-3JAN23-1250-P-OPTION.BYBIT"
    option_symbol = "BTC-29JUL25-110000-C-USDT-OPTION.BYBIT"
                    #"BTC-26JUN26-280000-P-USDT"
    
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
                instrument_provider=InstrumentProviderConfig(load_all=True),
                product_types=[product_type],
                testnet=True,  # Use testnet for testing
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
    
    # Print available options before running
    print("\n" + "="*80)
    print("AVAILABLE BYBIT OPTIONS (showing first 20)")
    print("="*80)
    
    # Get all instruments and filter for options
    all_instruments = node.cache.instruments()
    option_instruments = [
        instrument for instrument in all_instruments 
        if instrument.id.venue == BYBIT_VENUE and "-OPTION" in str(instrument.id)
    ]
    
    # Sort options by underlying, expiry, strike, and type
    def sort_key(instrument):
        symbol = str(instrument.id.symbol)
        # Parse the symbol to extract components for sorting
        parts = symbol.split('-')
        if len(parts) >= 4:
            underlying = parts[0]
            expiry = parts[1]
            strike = float(parts[2]) if parts[2].isdigit() else 0
            option_type = parts[3]  # C or P
            return (underlying, expiry, strike, option_type)
        return (symbol, 0, 0, '')
    
    sorted_options = sorted(option_instruments, key=sort_key)
    
    # Print header
    print(f"{'Symbol':<25} {'Underlying':<8} {'Expiry':<10} {'Strike':<10} {'Type':<4} {'Status':<8}")
    print("-" * 80)
    
    # Print first 20 options
    for i, instrument in enumerate(sorted_options[:20]):
        symbol = str(instrument.id.symbol)
        parts = symbol.split('-')
        
        if len(parts) >= 4:
            underlying = parts[0]
            expiry = parts[1]
            strike = parts[2]
            option_type = parts[3]
            status = "Active"  # You could get this from the instrument if available
        else:
            underlying = symbol[:8]
            expiry = "N/A"
            strike = "N/A"
            option_type = "N/A"
            status = "N/A"
        
        print(f"{symbol:<25} {underlying:<8} {expiry:<10} {strike:<10} {option_type:<4} {status:<8}")
    
    print("-" * 80)
    print(f"Total options loaded: {len(sorted_options)}")
    print("="*80)
    print()
    
    # Run the node
    if __name__ == "__main__":
        try:
            print(f"Starting Bybit Options Order Book Test for {option_symbol}")
            print("Press Ctrl+C to stop...")
            node.run()
        except KeyboardInterrupt:
            print("\nStopping...")
        finally:
            node.dispose()


if __name__ == "__main__":
    main() 