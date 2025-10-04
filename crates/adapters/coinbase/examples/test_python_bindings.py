#!/usr/bin/env python3
"""
Test script for Coinbase Advanced Trade API Python bindings.

This demonstrates how to use the Rust-based Coinbase adapter from Python.
"""

import asyncio
import json
import os
from nautilus_trader.core.nautilus_pyo3 import CoinbaseHttpClient, CoinbaseWebSocketClient


async def test_http_client():
    """Test HTTP client functionality."""
    print("=" * 80)
    print("Testing Coinbase HTTP Client")
    print("=" * 80)
    
    # Get API credentials from environment
    api_key = os.getenv("COINBASE_API_KEY")
    api_secret = os.getenv("COINBASE_API_SECRET")
    
    if not api_key or not api_secret:
        print("❌ Error: COINBASE_API_KEY and COINBASE_API_SECRET must be set")
        return
    
    # Create HTTP client
    client = CoinbaseHttpClient(api_key, api_secret)
    print(f"✅ Created HTTP client: {client}")
    
    # Test 1: List products
    print("\n📊 Test 1: List Products")
    products_json = await client.list_products()
    products = json.loads(products_json)
    print(f"✅ Found {len(products.get('products', []))} products")
    print(f"   First 3: {[p['product_id'] for p in products.get('products', [])[:3]]}")
    
    # Test 2: Get specific product
    print("\n📊 Test 2: Get Product (BTC-USD)")
    product_json = await client.get_product("BTC-USD")
    product = json.loads(product_json)
    print(f"✅ BTC-USD: {product.get('product_id')}")
    print(f"   Price: ${product.get('price', 'N/A')}")
    print(f"   Volume (24h): {product.get('volume_24h', 'N/A')}")
    
    # Test 3: List accounts
    print("\n💰 Test 3: List Accounts")
    accounts_json = await client.list_accounts()
    accounts = json.loads(accounts_json)
    print(f"✅ Found {len(accounts.get('accounts', []))} accounts")
    for acc in accounts.get('accounts', [])[:5]:
        balance = float(acc.get('available_balance', {}).get('value', 0))
        if balance > 0:
            print(f"   {acc.get('currency')}: {balance}")
    
    # Test 4: Get candles
    print("\n🕯️  Test 4: Get Candles (BTC-USD, 1 hour)")
    candles_json = await client.get_candles("BTC-USD", 3600, None, None)
    candles = json.loads(candles_json)
    print(f"✅ Retrieved {len(candles.get('candles', []))} candles")
    if candles.get('candles'):
        first_candle = candles['candles'][0]
        print(f"   Latest: O={first_candle.get('open')} H={first_candle.get('high')} "
              f"L={first_candle.get('low')} C={first_candle.get('close')}")
    
    # Test 5: Get market trades
    print("\n💱 Test 5: Get Market Trades (BTC-USD)")
    trades_json = await client.get_market_trades("BTC-USD", 5)
    trades = json.loads(trades_json)
    print(f"✅ Retrieved {len(trades.get('trades', []))} trades")
    
    # Test 6: Get product book
    print("\n📖 Test 6: Get Product Book (BTC-USD)")
    book_json = await client.get_product_book("BTC-USD", 10)
    book = json.loads(book_json)
    pricebook = book.get('pricebook', {})
    print(f"✅ Order book for {pricebook.get('product_id')}")
    print(f"   Bids: {len(pricebook.get('bids', []))}")
    print(f"   Asks: {len(pricebook.get('asks', []))}")
    if pricebook.get('bids') and pricebook.get('asks'):
        best_bid = pricebook['bids'][0]
        best_ask = pricebook['asks'][0]
        print(f"   Best Bid: ${best_bid.get('price')} ({best_bid.get('size')})")
        print(f"   Best Ask: ${best_ask.get('price')} ({best_ask.get('size')})")
    
    # Test 7: Get best bid/ask
    print("\n💹 Test 7: Get Best Bid/Ask (BTC-USD, ETH-USD)")
    best_json = await client.get_best_bid_ask(["BTC-USD", "ETH-USD"])
    best = json.loads(best_json)
    print(f"✅ Retrieved best bid/ask for {len(best.get('pricebooks', []))} products")
    for pb in best.get('pricebooks', []):
        print(f"   {pb.get('product_id')}: Bid=${pb.get('bids', [{}])[0].get('price', 'N/A')} "
              f"Ask=${pb.get('asks', [{}])[0].get('price', 'N/A')}")
    
    print("\n✅ HTTP Client tests completed successfully!")


async def test_websocket_client():
    """Test WebSocket client functionality."""
    print("\n" + "=" * 80)
    print("Testing Coinbase WebSocket Client")
    print("=" * 80)
    
    # Get API credentials from environment
    api_key = os.getenv("COINBASE_API_KEY")
    api_secret = os.getenv("COINBASE_API_SECRET")
    
    if not api_key or not api_secret:
        print("❌ Error: COINBASE_API_KEY and COINBASE_API_SECRET must be set")
        return
    
    # Create WebSocket client for market data
    client = CoinbaseWebSocketClient(api_key, api_secret, is_user_data=False)
    print(f"✅ Created WebSocket client: {client}")
    
    # Connect
    print("\n🔌 Connecting to WebSocket...")
    await client.connect()
    is_connected = await client.is_connected()
    print(f"✅ Connected: {is_connected}")
    
    # Subscribe to channels
    print("\n📡 Subscribing to channels...")
    await client.subscribe_heartbeats()
    print("✅ Subscribed to heartbeats")
    
    await client.subscribe(["BTC-USD", "ETH-USD"], "ticker")
    print("✅ Subscribed to ticker (BTC-USD, ETH-USD)")
    
    await client.subscribe(["BTC-USD"], "level2")
    print("✅ Subscribed to level2 (BTC-USD)")
    
    # Receive messages for 10 seconds
    print("\n📨 Receiving messages for 10 seconds...")
    message_count = 0
    ticker_count = 0
    heartbeat_count = 0
    level2_count = 0
    
    start_time = asyncio.get_event_loop().time()
    while asyncio.get_event_loop().time() - start_time < 10:
        try:
            message = await asyncio.wait_for(client.receive_message(), timeout=1.0)
            if message:
                message_count += 1
                data = json.loads(message)
                channel = data.get('channel', 'unknown')
                
                if channel == 'heartbeats':
                    heartbeat_count += 1
                    if heartbeat_count <= 3:
                        events = data.get('events', [])
                        if events:
                            print(f"💓 Heartbeat #{heartbeat_count}: counter={events[0].get('heartbeat_counter')}")
                
                elif channel == 'ticker':
                    ticker_count += 1
                    if ticker_count <= 5:
                        events = data.get('events', [])
                        if events:
                            event = events[0]
                            print(f"📊 Ticker: {event.get('product_id')} = ${event.get('price')} "
                                  f"(24h: {event.get('price_percent_chg_24h', 'N/A')}%)")
                
                elif channel == 'l2_data':
                    level2_count += 1
                    if level2_count == 1:
                        events = data.get('events', [])
                        if events:
                            event = events[0]
                            updates = event.get('updates', [])
                            print(f"📖 Level2: {event.get('product_id')} - {len(updates)} updates")
        
        except asyncio.TimeoutError:
            continue
    
    print(f"\n📊 Summary:")
    print(f"   Total messages: {message_count}")
    print(f"   Heartbeats: {heartbeat_count}")
    print(f"   Tickers: {ticker_count}")
    print(f"   Level2: {level2_count}")
    
    # Disconnect
    print("\n🔌 Disconnecting...")
    await client.disconnect()
    print("✅ Disconnected")
    
    print("\n✅ WebSocket Client tests completed successfully!")


async def main():
    """Run all tests."""
    try:
        await test_http_client()
        await test_websocket_client()
        
        print("\n" + "=" * 80)
        print("🎉 ALL TESTS PASSED!")
        print("=" * 80)
    
    except Exception as e:
        print(f"\n❌ Error: {e}")
        import traceback
        traceback.print_exc()


if __name__ == "__main__":
    asyncio.run(main())

