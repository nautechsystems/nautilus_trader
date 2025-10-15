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
Hyperliquid Testnet Order Placer - Python equivalent of place_order.rs

This example demonstrates how to place orders on Hyperliquid testnet using Nautilus Trader,
with the same command-line interface as hyperliquid-trading-bot/src/bin/place_order.rs.

╔══════════════════════════════════════════════════════════════════════════════╗
║                           SETUP REQUIREMENTS                                 ║
╚══════════════════════════════════════════════════════════════════════════════╝

1. Install/Build Nautilus Trader:
   
   Option A - From source (if you're in the nautilus_trader repo):
       cd nautilus_trader
       make install
   
   Option B - From PyPI:
       pip install nautilus_trader

2. Set environment variable:
   export HYPERLIQUID_TESTNET_PK='your_testnet_private_key'

3. Get testnet funds:
   Visit https://app.hyperliquid-testnet.xyz/

4. Run the script:
   python examples/sandbox/hyperliquid_testnet_order_placer.py --asset BTC --side buy --size 0.001

╔══════════════════════════════════════════════════════════════════════════════╗
║                            USAGE EXAMPLES                                    ║
╚══════════════════════════════════════════════════════════════════════════════╝

Basic buy order:
    python hyperliquid_testnet_order_placer.py --asset BTC --side buy --size 0.001

Sell with custom spread:
    python hyperliquid_testnet_order_placer.py --asset ETH --side sell --size 0.01 --spread 0.5

Using notional value ($ amount):
    python hyperliquid_testnet_order_placer.py --asset ETH --side buy --notional 50

Post-only order (maker-only):
    python hyperliquid_testnet_order_placer.py --asset BTC --side buy --size 0.001 --post-only

Immediate-or-Cancel:
    python hyperliquid_testnet_order_placer.py --asset BTC --side buy --size 0.001 --tif IOC

╔══════════════════════════════════════════════════════════════════════════════╗
║                         COMMAND LINE OPTIONS                                 ║
╚══════════════════════════════════════════════════════════════════════════════╝

Required:
    --asset       Asset to trade (BTC, ETH, SOL, etc.)
    --side        Order side (buy, sell, long, short)
    --size        Order size in asset units (or use --notional)

Optional:
    --notional    USD value instead of --size (e.g., 50 = $50 worth)
    --spread      Spread from mid price % (default: 1.0)
    --leverage    Leverage multiplier (default: 10)
    --tif         Time in force: GTC, IOC, FOK (default: GTC)
    --post-only   Maker-only orders (no taker fees)
    --log-level   Logging level: INFO, DEBUG (default: INFO)

╔══════════════════════════════════════════════════════════════════════════════╗
║                    RUST VS PYTHON COMPARISON                                 ║
╚══════════════════════════════════════════════════════════════════════════════╝

RUST (place_order.rs):
    cargo run --bin place_order -- --asset BTC --side buy --size 0.001

PYTHON (this script):
    python hyperliquid_testnet_order_placer.py --asset BTC --side buy --size 0.001

Both produce the same result, different implementation approaches!

RUST APPROACH:
    - Direct SDK usage with hyperliquid-rust-sdk
    - Synchronous execution flow
    - Immediate response handling
    - Minimal abstractions

PYTHON APPROACH:
    - Framework-based with Nautilus Trader
    - Event-driven with callbacks
    - Async order lifecycle management
    - Built-in risk management

╔══════════════════════════════════════════════════════════════════════════════╗
║                         HOW IT WORKS                                         ║
╚══════════════════════════════════════════════════════════════════════════════╝

1. Strategy starts → on_start()
2. Subscribe to quotes → get live market data
3. Receive quote → on_quote_tick(tick)
4. Calculate order price:
   - Buy: mid_price * (1 - spread%)
   - Sell: mid_price * (1 + spread%)
5. Create limit order
6. Submit to exchange
7. Handle callbacks:
   - on_order_accepted() → Order on book
   - on_order_filled() → Order executed
   - on_order_rejected() → Order failed

╔══════════════════════════════════════════════════════════════════════════════╗
║                         TROUBLESHOOTING                                      ║
╚══════════════════════════════════════════════════════════════════════════════╝

❌ "HYPERLIQUID_TESTNET_PK not set"
   → export HYPERLIQUID_TESTNET_PK='your_private_key'

❌ "Instrument not found"
   → Check asset name (BTC, ETH, SOL)
   → Ensure load_all=True in config

❌ "Insufficient balance"
   → Visit https://app.hyperliquid-testnet.xyz/ for testnet funds

❌ "Order rejected"
   → Check order size (may be too small)
   → Adjust spread (may be too aggressive)
   → Verify leverage settings

Enable debug logging:
    python hyperliquid_testnet_order_placer.py --asset BTC --side buy --size 0.001 --log-level DEBUG

╔══════════════════════════════════════════════════════════════════════════════╗
║                          VIEW YOUR ORDERS                                    ║
╚══════════════════════════════════════════════════════════════════════════════╝

Testnet Interface: https://app.hyperliquid-testnet.xyz/portfolio

⚠️  WARNING: This is for TESTNET ONLY. Do not use with real funds!

"""

import argparse
import asyncio
import os
import sys
from decimal import Decimal

from nautilus_trader.adapters.hyperliquid import HYPERLIQUID
from nautilus_trader.adapters.hyperliquid import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidExecClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidLiveDataClientFactory
from nautilus_trader.adapters.hyperliquid import HyperliquidLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.trading.strategy import Strategy


class OrderPlacer(Strategy):
    """
    A simple strategy that places a single order and then stops.
    
    This mimics the behavior of the Rust place_order binary.
    """

    def __init__(
        self,
        instrument_id: InstrumentId,
        side: OrderSide,
        size: Decimal | None,
        notional: Decimal | None,
        spread: float,
        leverage: int,
        post_only: bool,
        time_in_force: TimeInForce,
    ):
        super().__init__()
        self.instrument_id = instrument_id
        self.side = side
        self.size = size
        self.notional = notional
        self.spread = spread
        self.leverage = leverage
        self.post_only = post_only
        self.time_in_force = time_in_force
        self._order_placed = False

    def on_start(self) -> None:
        """Called when strategy starts."""
        self.log.info(f"OrderPlacer started for {self.instrument_id}")
        
        # Subscribe to quote ticks to get market data
        self.subscribe_quote_ticks(self.instrument_id)
        
    def on_quote_tick(self, tick) -> None:
        """Called when a quote tick is received."""
        if self._order_placed:
            return
            
        self.log.info(f"Market Data: Bid=${tick.bid_price}, Ask=${tick.ask_price}")
        
        # Get instrument
        instrument = self.cache.instrument(self.instrument_id)
        if instrument is None:
            self.log.error(f"Instrument {self.instrument_id} not found")
            self.stop()
            return
            
        # Calculate mid price
        mid_price = (tick.bid_price + tick.ask_price) / 2
        self.log.info(f"Mid Price: ${mid_price}")
        
        # Calculate order size
        if self.notional is not None:
            # Calculate size from notional value
            raw_size = self.notional / float(mid_price)
            order_size = instrument.make_qty(Decimal(str(raw_size)))
            self.log.info(f"Calculated size from ${self.notional} notional: {order_size}")
        else:
            order_size = instrument.make_qty(self.size)
            
        # Calculate order price with spread
        if self.side == OrderSide.BUY:
            # Buy below market
            spread_multiplier = 1.0 - (self.spread / 100.0)
        else:
            # Sell above market
            spread_multiplier = 1.0 + (self.spread / 100.0)
            
        order_price = instrument.make_price(mid_price * Decimal(str(spread_multiplier)))
        notional_value = float(order_size) * float(order_price)
        
        self.log.info("Order Details:")
        self.log.info(f"   Side: {self.side.name}")
        self.log.info(f"   Size: {order_size} {instrument.base_currency}")
        self.log.info(f"   Price: ${order_price} ({self.spread}% {'below' if self.side == OrderSide.BUY else 'above'} market)")
        self.log.info(f"   Notional: ${notional_value:.2f}")
        self.log.info(f"   Time-in-Force: {self.time_in_force.name}")
        self.log.info(f"   Post-Only: {self.post_only}")
        
        # Create and submit limit order
        order = self.order_factory.limit(
            instrument_id=self.instrument_id,
            order_side=self.side,
            quantity=order_size,
            price=order_price,
            time_in_force=self.time_in_force,
            post_only=self.post_only,
        )
        
        self.log.info(f"Placing order: {order.client_order_id}")
        self.submit_order(order)
        self._order_placed = True
        
        # Stop after placing order
        self.log.info("Order submitted! Stopping strategy...")
        
    def on_order_accepted(self, event) -> None:
        """Called when order is accepted by the exchange."""
        self.log.info(f"ORDER ACCEPTED: {event.client_order_id}")
        self.log.info(f"   Venue Order ID: {event.venue_order_id}")
        
    def on_order_rejected(self, event) -> None:
        """Called when order is rejected by the exchange."""
        self.log.error(f"ORDER REJECTED: {event.client_order_id}")
        self.log.error(f"   Reason: {event.reason}")
        
    def on_order_filled(self, event) -> None:
        """Called when order is filled."""
        self.log.info(f"ORDER FILLED: {event.client_order_id}")
        self.log.info(f"   Filled Qty: {event.last_qty}")
        self.log.info(f"   Fill Price: ${event.last_px}")
        
    def on_stop(self) -> None:
        """Called when strategy stops."""
        self.log.info("OrderPlacer stopped")


def parse_args():
    """Parse command line arguments."""
    parser = argparse.ArgumentParser(
        description="Place orders on Hyperliquid testnet",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__
    )
    
    parser.add_argument(
        "--asset",
        required=True,
        help="Asset to trade (e.g., BTC, ETH, SOL)"
    )
    
    parser.add_argument(
        "--side",
        required=True,
        choices=["buy", "sell", "long", "short", "b", "s"],
        help="Order side: buy/long or sell/short"
    )
    
    parser.add_argument(
        "--size",
        type=float,
        help="Order size in asset units (e.g., 0.01 ETH). Required unless --notional is used"
    )
    
    parser.add_argument(
        "--notional",
        type=float,
        help="Target notional value in USD (e.g., 50 means ~$50 worth of the asset)"
    )
    
    parser.add_argument(
        "--spread",
        type=float,
        default=1.0,
        help="Spread from mid price as percentage (default: 1.0%%)"
    )
    
    parser.add_argument(
        "--leverage",
        type=int,
        default=10,
        help="Leverage multiplier (default: 10x)"
    )
    
    parser.add_argument(
        "--tif",
        choices=["GTC", "IOC", "FOK", "GTD"],
        default="GTC",
        help="Time in force (default: GTC - Good-til-Cancel)"
    )
    
    parser.add_argument(
        "--post-only",
        action="store_true",
        help="Use post-only orders (maker-only, no taker fees)"
    )
    
    parser.add_argument(
        "--log-level",
        choices=["DEBUG", "INFO", "WARNING", "ERROR"],
        default="INFO",
        help="Logging level (default: INFO)"
    )
    
    args = parser.parse_args()
    
    # Validate arguments
    if args.size is None and args.notional is None:
        parser.error("Either --size or --notional must be specified")
        
    if args.size is not None and args.notional is not None:
        parser.error("Cannot specify both --size and --notional")
        
    return args


async def main():
    """Main entry point."""
    args = parse_args()
    
    # Check for testnet private key
    testnet_pk = os.getenv("HYPERLIQUID_TESTNET_PK")
    if not testnet_pk:
        print("❌ Error: HYPERLIQUID_TESTNET_PK environment variable not set")
        print("Please set your testnet private key:")
        print("  export HYPERLIQUID_TESTNET_PK='your_private_key_here'")
        sys.exit(1)
    
    # Parse order side
    side_lower = args.side.lower()
    if side_lower in ["buy", "long", "b"]:
        order_side = OrderSide.BUY
        side_str = "BUY"
    else:
        order_side = OrderSide.SELL
        side_str = "SELL"
    
    # Parse time in force
    tif_map = {
        "GTC": TimeInForce.GTC,
        "IOC": TimeInForce.IOC,
        "FOK": TimeInForce.FOK,
        "GTD": TimeInForce.GTD,
    }
    time_in_force = tif_map[args.tif]
    
    # Build instrument ID
    # Hyperliquid uses format: BTC-USD-PERP, ETH-USD-PERP, etc.
    symbol = f"{args.asset.upper()}-USD-PERP"
    instrument_id = InstrumentId.from_str(f"{symbol}.{HYPERLIQUID}")
    
    print(f"\nPlace {side_str} Order on Hyperliquid Testnet\n")
    print(f"Configuration:")
    print(f"   Asset: {args.asset.upper()}")
    print(f"   Symbol: {symbol}")
    print(f"   Side: {side_str}")
    if args.size:
        print(f"   Size: {args.size}")
    if args.notional:
        print(f"   Notional: ${args.notional}")
    print(f"   Spread: {args.spread}%")
    print(f"   Leverage: {args.leverage}x")
    print(f"   Time-in-Force: {time_in_force.name}")
    print(f"   Post-Only: {args.post_only}")
    print()
    
    # Configure the trading node
    config_node = TradingNodeConfig(
        trader_id=TraderId("ORDER-PLACER-001"),
        logging=LoggingConfig(
            log_level=args.log_level,
            use_pyo3=True,
        ),
        exec_engine=LiveExecEngineConfig(
            reconciliation=False,  # Disable for simple order placement
        ),
        data_clients={
            HYPERLIQUID: HyperliquidDataClientConfig(
                instrument_provider=InstrumentProviderConfig(load_all=True),
                testnet=True,
            ),
        },
        exec_clients={
            HYPERLIQUID: HyperliquidExecClientConfig(
                private_key=None,  # Will use HYPERLIQUID_TESTNET_PK env var
                instrument_provider=InstrumentProviderConfig(load_all=True),
                testnet=True,
            ),
        },
        timeout_connection=30.0,
        timeout_reconciliation=10.0,
        timeout_portfolio=10.0,
        timeout_disconnection=10.0,
        timeout_post_stop=2.0,
    )
    
    # Create strategy
    strategy = OrderPlacer(
        instrument_id=instrument_id,
        side=order_side,
        size=Decimal(str(args.size)) if args.size else None,
        notional=Decimal(str(args.notional)) if args.notional else None,
        spread=args.spread,
        leverage=args.leverage,
        post_only=args.post_only,
        time_in_force=time_in_force,
    )
    
    # Create and configure node
    node = TradingNode(config=config_node)
    node.trader.add_strategy(strategy)
    node.add_data_client_factory(HYPERLIQUID, HyperliquidLiveDataClientFactory)
    node.add_exec_client_factory(HYPERLIQUID, HyperliquidLiveExecClientFactory)
    node.build()
    
    try:
        # Start the node
        print("Connecting to Hyperliquid testnet...")
        
        # Start the node and run for 10 seconds to allow order placement
        await node.kernel.start_async()
        await asyncio.sleep(10)
        
        # Stop the node
        print("\nStopping...")
        await node.stop_async()
        
        print("\nDone! View your order at: https://app.hyperliquid-testnet.xyz/portfolio")
        
    except KeyboardInterrupt:
        print("\nInterrupted by user")
        await node.stop_async()
    except Exception as e:
        print(f"\nError: {e}")
        import traceback
        traceback.print_exc()
        await node.stop_async()


if __name__ == "__main__":
    # *** THIS IS A TEST SCRIPT WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
    # *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***
    asyncio.run(main())
