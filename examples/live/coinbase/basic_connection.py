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
Basic Coinbase connection test.

This example demonstrates how to:
1. Connect to the Coinbase Advanced Trade API
2. Fetch available trading products
3. Get account balances
4. Retrieve current prices

Requirements:
- COINBASE_API_KEY environment variable
- COINBASE_API_SECRET environment variable

To run:
    export COINBASE_API_KEY="organizations/your-org-id/apiKeys/your-key-id"
    export COINBASE_API_SECRET="-----BEGIN EC PRIVATE KEY-----\n...\n-----END EC PRIVATE KEY-----"
    python basic_connection.py
"""

import asyncio
import os
from decimal import Decimal

from nautilus_trader.adapters.coinbase.factories import get_coinbase_http_client


async def main():
    """Test basic connection to Coinbase API."""
    print("=" * 80)
    print("Coinbase Advanced Trade API - Basic Connection Test")
    print("=" * 80)
    
    # Get API credentials from environment
    api_key = os.getenv("COINBASE_API_KEY")
    api_secret = os.getenv("COINBASE_API_SECRET")
    
    if not api_key or not api_secret:
        print("\n❌ Error: API credentials not found!")
        print("\nPlease set the following environment variables:")
        print("  COINBASE_API_KEY")
        print("  COINBASE_API_SECRET")
        print("\nExample:")
        print('  export COINBASE_API_KEY="organizations/your-org-id/apiKeys/your-key-id"')
        print('  export COINBASE_API_SECRET="-----BEGIN EC PRIVATE KEY-----\\n...\\n-----END EC PRIVATE KEY-----"')
        return
    
    try:
        # Create HTTP client
        print("\n1. Creating Coinbase HTTP client...")
        client = get_coinbase_http_client(
            api_key=api_key,
            api_secret=api_secret,
        )
        print("   ✓ HTTP client created successfully")
        
        # Fetch available products
        print("\n2. Fetching available trading products...")
        products = await client.list_products()
        print(f"   ✓ Found {len(products.get('products', []))} trading products")
        
        # Show first 5 products as examples
        print("\n   Example products:")
        for product in products.get('products', [])[:5]:
            product_id = product.get('product_id', 'N/A')
            base = product.get('base_currency_id', 'N/A')
            quote = product.get('quote_currency_id', 'N/A')
            status = product.get('status', 'N/A')
            print(f"     • {product_id:15} ({base}/{quote}) - Status: {status}")
        
        # Fetch account balances
        print("\n3. Fetching account balances...")
        accounts = await client.list_accounts()
        print(f"   ✓ Found {len(accounts.get('accounts', []))} accounts")
        
        # Show accounts with non-zero balances
        print("\n   Accounts with balances:")
        has_balance = False
        for account in accounts.get('accounts', []):
            currency = account.get('currency', 'N/A')
            available = Decimal(account.get('available_balance', {}).get('value', '0'))
            if available > 0:
                has_balance = True
                print(f"     • {currency:10} {available:>15}")
        
        if not has_balance:
            print("     (No accounts with balances)")
        
        # Get current price for BTC-USD
        print("\n4. Fetching current BTC-USD price...")
        try:
            product = await client.get_product("BTC-USD")
            price = product.get('price', 'N/A')
            print(f"   ✓ BTC-USD: ${price}")
        except Exception as e:
            print(f"   ⚠ Could not fetch BTC-USD price: {e}")
        
        print("\n" + "=" * 80)
        print("✅ Connection test completed successfully!")
        print("=" * 80)
        
    except Exception as e:
        print(f"\n❌ Error: {e}")
        print("\nTroubleshooting:")
        print("  1. Verify your API key and secret are correct")
        print("  2. Ensure your API key has 'View' permissions")
        print("  3. Check that your API key is not expired")
        print("  4. Verify you're using Coinbase Advanced Trade API credentials")
        raise


if __name__ == "__main__":
    asyncio.run(main())

