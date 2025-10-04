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
Simple example trading strategy for Coinbase.

This example demonstrates a basic momentum strategy that:
1. Monitors BTC-USD price
2. Buys when price increases by 2% in 5 minutes
3. Sells when price decreases by 1% from entry

This is for EDUCATIONAL PURPOSES ONLY. Do not use in production without proper testing.

Requirements:
- COINBASE_API_KEY environment variable
- COINBASE_API_SECRET environment variable

To run:
    export COINBASE_API_KEY="organizations/your-org-id/apiKeys/your-key-id"
    export COINBASE_API_SECRET="-----BEGIN EC PRIVATE KEY-----\n...\n-----END EC PRIVATE KEY-----"
    python simple_strategy.py
"""

import asyncio
import os
from collections import deque
from datetime import datetime
from decimal import Decimal

from nautilus_trader.adapters.coinbase.factories import get_coinbase_http_client


# Strategy configuration
PRODUCT_ID = "BTC-USD"
BUY_THRESHOLD = Decimal("0.02")  # Buy when price up 2%
SELL_THRESHOLD = Decimal("0.01")  # Sell when price down 1% from entry
TRADE_AMOUNT_USD = Decimal("100")  # Trade size in USD
CHECK_INTERVAL = 60  # Check every 60 seconds
LOOKBACK_PERIODS = 5  # Look back 5 periods (5 minutes)


class SimpleMomentumStrategy:
    """
    Simple momentum trading strategy.
    
    This strategy buys when price momentum is positive and sells when it reverses.
    """
    
    def __init__(self, client, product_id: str, dry_run: bool = True):
        """
        Initialize the strategy.
        
        Parameters
        ----------
        client : CoinbaseHttpClient
            The Coinbase HTTP client
        product_id : str
            The product to trade (e.g., "BTC-USD")
        dry_run : bool, default True
            If True, only simulate trades without executing
        
        """
        self.client = client
        self.product_id = product_id
        self.dry_run = dry_run
        
        self.price_history = deque(maxlen=LOOKBACK_PERIODS)
        self.position = None  # None, 'LONG'
        self.entry_price = None
        
    async def get_current_price(self) -> Decimal:
        """Get current price for the product."""
        product = await self.client.get_product(self.product_id)
        return Decimal(product.get('price', '0'))
    
    def calculate_momentum(self) -> Decimal | None:
        """Calculate price momentum over lookback period."""
        if len(self.price_history) < 2:
            return None
        
        oldest_price = self.price_history[0]
        current_price = self.price_history[-1]
        
        return (current_price - oldest_price) / oldest_price
    
    async def execute_buy(self, price: Decimal):
        """Execute a buy order."""
        quantity = TRADE_AMOUNT_USD / price
        
        if self.dry_run:
            print(f"  [DRY RUN] BUY {quantity:.8f} {self.product_id} @ ${price}")
        else:
            # In real trading, you would execute the order here
            # order = await self.client.create_market_order(...)
            print(f"  [LIVE] BUY {quantity:.8f} {self.product_id} @ ${price}")
        
        self.position = 'LONG'
        self.entry_price = price
    
    async def execute_sell(self, price: Decimal):
        """Execute a sell order."""
        quantity = TRADE_AMOUNT_USD / self.entry_price
        pnl = (price - self.entry_price) / self.entry_price * 100
        
        if self.dry_run:
            print(f"  [DRY RUN] SELL {quantity:.8f} {self.product_id} @ ${price} | P&L: {pnl:+.2f}%")
        else:
            # In real trading, you would execute the order here
            # order = await self.client.create_market_order(...)
            print(f"  [LIVE] SELL {quantity:.8f} {self.product_id} @ ${price} | P&L: {pnl:+.2f}%")
        
        self.position = None
        self.entry_price = None
    
    async def check_signals(self):
        """Check for buy/sell signals."""
        current_price = await self.get_current_price()
        self.price_history.append(current_price)
        
        timestamp = datetime.now().strftime("%H:%M:%S")
        print(f"[{timestamp}] {self.product_id}: ${current_price:,.2f}", end="")
        
        # Calculate momentum
        momentum = self.calculate_momentum()
        if momentum is not None:
            print(f" | Momentum: {momentum*100:+.2f}%", end="")
        
        # Check for buy signal
        if self.position is None and momentum is not None:
            if momentum >= BUY_THRESHOLD:
                print(f" | 🟢 BUY SIGNAL")
                await self.execute_buy(current_price)
                return
        
        # Check for sell signal
        if self.position == 'LONG':
            pnl = (current_price - self.entry_price) / self.entry_price
            print(f" | Position P&L: {pnl*100:+.2f}%", end="")
            
            if pnl <= -SELL_THRESHOLD:
                print(f" | 🔴 SELL SIGNAL")
                await self.execute_sell(current_price)
                return
        
        print()  # New line
    
    async def run(self):
        """Run the strategy."""
        print("=" * 80)
        print(f"Simple Momentum Strategy - {self.product_id}")
        print("=" * 80)
        print(f"Buy Threshold: +{BUY_THRESHOLD*100}%")
        print(f"Sell Threshold: -{SELL_THRESHOLD*100}%")
        print(f"Trade Size: ${TRADE_AMOUNT_USD}")
        print(f"Mode: {'DRY RUN (simulation)' if self.dry_run else 'LIVE TRADING'}")
        print("\nPress Ctrl+C to stop...\n")
        
        try:
            while True:
                await self.check_signals()
                await asyncio.sleep(CHECK_INTERVAL)
        
        except KeyboardInterrupt:
            print("\n\nShutting down...")
            
            # Close any open positions
            if self.position == 'LONG':
                current_price = await self.get_current_price()
                print("\nClosing open position...")
                await self.execute_sell(current_price)


async def main():
    """Run the simple momentum strategy."""
    # Get API credentials from environment
    api_key = os.getenv("COINBASE_API_KEY")
    api_secret = os.getenv("COINBASE_API_SECRET")
    
    if not api_key or not api_secret:
        print("❌ Error: API credentials not found!")
        print("\nPlease set COINBASE_API_KEY and COINBASE_API_SECRET environment variables.")
        return
    
    # Create HTTP client
    client = get_coinbase_http_client(
        api_key=api_key,
        api_secret=api_secret,
    )
    
    # Create and run strategy
    strategy = SimpleMomentumStrategy(
        client=client,
        product_id=PRODUCT_ID,
        dry_run=True,  # Set to False for live trading (NOT RECOMMENDED without testing)
    )
    
    await strategy.run()


if __name__ == "__main__":
    asyncio.run(main())

