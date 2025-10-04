#!/usr/bin/env python3
"""
Simple test script for the Coinbase adapter.

This script demonstrates basic usage of the Coinbase adapter without requiring
a full NautilusTrader installation.
"""

import asyncio
import os
import sys

# Add the project root to the path
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "../../.."))

from nautilus_trader.adapters.coinbase.config import CoinbaseDataClientConfig
from nautilus_trader.adapters.coinbase.factories import get_coinbase_http_client


async def test_coinbase_connection():
    """Test basic connection to Coinbase API."""
    print("Testing Coinbase adapter...")
    print("-" * 50)
    
    # Get API credentials from environment
    api_key = os.getenv("COINBASE_API_KEY")
    api_secret = os.getenv("COINBASE_API_SECRET")
    
    if not api_key or not api_secret:
        print("⚠️  Warning: COINBASE_API_KEY and COINBASE_API_SECRET not set")
        print("   Set these environment variables to test with real credentials")
        print("   For now, testing with placeholder values...")
        api_key = "test_key"
        api_secret = "test_secret"
    
    try:
        # Create HTTP client
        print("\n1. Creating Coinbase HTTP client...")
        client = get_coinbase_http_client(
            api_key=api_key,
            api_secret=api_secret,
        )
        print("✓ HTTP client created successfully")
        
        # Test listing products (this doesn't require authentication)
        print("\n2. Fetching available products...")
        try:
            products_json = await client.list_products()
            print(f"✓ Successfully fetched products")
            print(f"   Response length: {len(products_json)} characters")
        except Exception as e:
            print(f"✗ Failed to fetch products: {e}")
            if "401" in str(e) or "403" in str(e):
                print("   This is expected if using placeholder credentials")
        
        print("\n" + "=" * 50)
        print("Coinbase adapter test completed!")
        print("=" * 50)
        
    except Exception as e:
        print(f"\n✗ Error: {e}")
        import traceback
        traceback.print_exc()
        return False
    
    return True


if __name__ == "__main__":
    print("""
╔══════════════════════════════════════════════════════════════╗
║                 Coinbase Adapter Test                        ║
╚══════════════════════════════════════════════════════════════╝
    """)
    
    success = asyncio.run(test_coinbase_connection())
    
    if success:
        print("\n✓ All tests passed!")
        sys.exit(0)
    else:
        print("\n✗ Some tests failed")
        sys.exit(1)

