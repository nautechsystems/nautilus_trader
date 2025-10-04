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
Stream live market data from Coinbase.

This example demonstrates how to:
1. Connect to Coinbase WebSocket API
2. Subscribe to real-time market data
3. Handle ticker updates
4. Process order book updates

Requirements:
- COINBASE_API_KEY environment variable
- COINBASE_API_SECRET environment variable

To run:
    export COINBASE_API_KEY="organizations/your-org-id/apiKeys/your-key-id"
    export COINBASE_API_SECRET="-----BEGIN EC PRIVATE KEY-----\n...\n-----END EC PRIVATE KEY-----"
    python stream_market_data.py
"""

import asyncio
import os
from datetime import datetime

from nautilus_trader.adapters.coinbase.factories import get_coinbase_websocket_client


# Products to subscribe to
PRODUCTS = ["BTC-USD", "ETH-USD", "SOL-USD"]


async def handle_message(message: dict):
    """
    Handle incoming WebSocket messages.
    
    Parameters
    ----------
    message : dict
        The WebSocket message
    
    """
    channel = message.get("channel", "unknown")
    timestamp = datetime.now().strftime("%H:%M:%S.%f")[:-3]
    
    if channel == "ticker":
        # Handle ticker updates
        events = message.get("events", [])
        for event in events:
            tickers = event.get("tickers", [])
            for ticker in tickers:
                product_id = ticker.get("product_id", "N/A")
                price = ticker.get("price", "N/A")
                volume_24h = ticker.get("volume_24_h", "N/A")
                
                print(f"[{timestamp}] TICKER {product_id:10} Price: ${price:>10} | 24h Volume: {volume_24h}")
    
    elif channel == "level2":
        # Handle order book updates
        events = message.get("events", [])
        for event in events:
            product_id = event.get("product_id", "N/A")
            updates = event.get("updates", [])
            
            if updates:
                print(f"[{timestamp}] BOOK   {product_id:10} {len(updates)} updates")
    
    elif channel == "heartbeats":
        # Handle heartbeat messages
        print(f"[{timestamp}] HEARTBEAT")
    
    elif channel == "subscriptions":
        # Handle subscription confirmations
        print(f"[{timestamp}] SUBSCRIPTION confirmed")
    
    else:
        # Handle other message types
        print(f"[{timestamp}] {channel.upper()}: {message}")


async def main():
    """Stream live market data from Coinbase."""
    print("=" * 80)
    print("Coinbase Advanced Trade API - Market Data Streaming")
    print("=" * 80)
    
    # Get API credentials from environment
    api_key = os.getenv("COINBASE_API_KEY")
    api_secret = os.getenv("COINBASE_API_SECRET")
    
    if not api_key or not api_secret:
        print("\n❌ Error: API credentials not found!")
        print("\nPlease set the following environment variables:")
        print("  COINBASE_API_KEY")
        print("  COINBASE_API_SECRET")
        return
    
    print(f"\nSubscribing to: {', '.join(PRODUCTS)}")
    print("Channels: ticker, level2, heartbeats")
    print("\nPress Ctrl+C to stop...\n")
    
    try:
        # Create WebSocket client
        client = get_coinbase_websocket_client(
            api_key=api_key,
            api_secret=api_secret,
            message_handler=handle_message,
        )
        
        # Connect to WebSocket
        await client.connect()
        
        # Subscribe to ticker channel
        await client.subscribe_ticker(PRODUCTS)
        
        # Subscribe to level2 (order book) channel
        await client.subscribe_level2(PRODUCTS)
        
        # Subscribe to heartbeats
        await client.subscribe_heartbeats()
        
        # Keep connection alive
        while True:
            await asyncio.sleep(1)
    
    except KeyboardInterrupt:
        print("\n\nShutting down...")
    
    except Exception as e:
        print(f"\n❌ Error: {e}")
        raise
    
    finally:
        # Cleanup
        if 'client' in locals():
            await client.disconnect()
        print("Disconnected.")


if __name__ == "__main__":
    asyncio.run(main())

