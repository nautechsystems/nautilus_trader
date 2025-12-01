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
Test script to verify Rust gRPC bindings work from Python.

This demonstrates the Rust-first approach for dYdX v4:
- DydxGrpcClient: Rust gRPC client exposed via PyO3
- DydxWallet: Rust wallet/signing exposed via PyO3
- DydxOrderSubmitter: Rust order submission exposed via PyO3

Usage:
    Set environment variables:
    - DYDX_MNEMONIC: 24-word mnemonic phrase

    python examples/live/dydx/dydx_v4_grpc_bindings.py

"""

import asyncio
import os
import random
import sys


async def test_rust_grpc_bindings() -> None:  # noqa: C901
    """
    Test the Rust gRPC bindings.
    """
    try:
        from nautilus_trader.core.nautilus_pyo3 import DydxGrpcClient  # type: ignore[attr-defined]
        from nautilus_trader.core.nautilus_pyo3 import DydxHttpClient  # type: ignore[attr-defined]
        from nautilus_trader.core.nautilus_pyo3 import DydxOrderSubmitter  # type: ignore[attr-defined]
        from nautilus_trader.core.nautilus_pyo3 import DydxWallet  # type: ignore[attr-defined]
    except ImportError as e:
        print(f"Failed to import Rust bindings: {e}")
        print("Make sure to run 'make build' first to compile the Rust extensions.")
        sys.exit(1)

    # Configuration - mainnet only for now
    is_testnet = False
    mnemonic = os.environ.get("DYDX_MNEMONIC", "")

    if not mnemonic:
        print("DYDX_MNEMONIC environment variable not set")
        print("Using a dummy test - wallet creation only")

        # Test wallet creation with a test mnemonic (DO NOT USE FOR REAL FUNDS)
        test_mnemonic = (
            "abandon abandon abandon abandon abandon abandon abandon abandon "
            "abandon abandon abandon abandon abandon abandon abandon abandon "
            "abandon abandon abandon abandon abandon abandon abandon art"
        )

        try:
            wallet = DydxWallet.from_mnemonic(test_mnemonic)
            address = wallet.address()
            print("[OK] Wallet created successfully")
            print(f"  Address: {address}")
        except Exception as e:
            print(f"[FAILED] Wallet creation failed: {e}")
            return

        print("\n[OK] Basic Rust binding tests passed!")
        print("Set DYDX_MNEMONIC to test full gRPC connectivity.")
        return

    # Get gRPC URLs from Rust bindings
    from nautilus_trader.adapters.dydx_v4 import get_grpc_urls

    grpc_urls = get_grpc_urls(is_testnet)

    print(f"Testing Rust gRPC bindings on {'testnet' if is_testnet else 'mainnet'}...")
    print(f"gRPC URLs: {grpc_urls}")

    # Test 1: Create wallet from mnemonic
    print("\n1. Testing DydxWallet...")
    try:
        wallet = DydxWallet.from_mnemonic(mnemonic)
        address = wallet.address()
        print(f"   [OK] Wallet created, address: {address}")
    except Exception as e:
        print(f"   [FAILED] Wallet creation failed: {e}")
        return

    # Test 2: Connect to gRPC
    print("\n2. Testing DydxGrpcClient connection...")
    try:
        grpc_client = await DydxGrpcClient.connect_with_fallback(grpc_urls)
        print("   [OK] gRPC client connected")
    except Exception as e:
        print(f"   [FAILED] gRPC connection failed: {e}")
        return

    # Test 3: Get latest block height
    print("\n3. Testing latest_block_height...")
    try:
        block_height = await grpc_client.latest_block_height()
        print(f"   [OK] Latest block height: {block_height}")
    except Exception as e:
        print(f"   [FAILED] Failed to get block height: {e}")
        return

    # Test 4: Get account info
    print("\n4. Testing get_account...")
    try:
        account_info = await grpc_client.get_account(address)
        print(f"   [OK] Account info: account_number={account_info[0]}, sequence={account_info[1]}")
    except Exception as e:
        print(f"   [FAILED] Failed to get account: {e}")
        # This may fail if account doesn't exist on chain - that's OK

    # Test 5: Get account balances
    print("\n5. Testing get_account_balances...")
    try:
        balances = await grpc_client.get_account_balances(address)
        print(f"   [OK] Account balances: {balances}")
    except Exception as e:
        print(f"   [FAILED] Failed to get balances: {e}")

    # Test 6: Get subaccount
    print("\n6. Testing get_subaccount...")
    try:
        subaccount = await grpc_client.get_subaccount(address, 0)
        print(f"   [OK] Subaccount info: {subaccount}")
    except Exception as e:
        print(f"   [FAILED] Failed to get subaccount: {e}")

    # Test 7: Create HTTP client and fetch instruments
    print("\n7. Testing DydxHttpClient and instrument fetching...")
    try:
        http_client = DydxHttpClient(is_testnet=False)
        await http_client.fetch_and_cache_instruments()
        print(f"   [OK] Fetched and cached {http_client.instrument_count()} instruments")
        symbols = http_client.instrument_symbols()
        print(f"   Available: {symbols[:5]}...")
    except Exception as e:
        print(f"   [FAILED] Failed to fetch instruments: {e}")
        return

    # Test 8: Create OrderSubmitter and place a limit order on SOL-USD-PERP
    print("\n8. Testing DydxOrderSubmitter - placing SOL limit order...")
    try:
        submitter = DydxOrderSubmitter(
            grpc_client=grpc_client,
            http_client=http_client,
            wallet_address=address,
            subaccount_number=0,
            chain_id="dydx-mainnet-1",
        )
        print("   [OK] OrderSubmitter created")

        # Generate a unique client order ID
        client_order_id = random.randint(100000, 999999)  # noqa: S311

        # Place a BUY limit order for SOL-USD-PERP far below market price
        # This is a post-only order that won't fill immediately
        instrument_id = "SOL-USD-PERP.DYDX"
        side = 1  # BUY (OrderSide enum: 0=NO_SIDE, 1=BUY, 2=SELL)
        price = "100.00"  # Far below current SOL price (~$230)
        quantity = "0.1"  # Minimum quantity
        time_in_force = 1  # GTC (TimeInForce enum: 1=GTC)
        post_only = True
        reduce_only = False
        expire_time = None  # No expiry for GTC

        print(f"   Placing order: {side=} {quantity} {instrument_id} @ {price}")
        print(f"   client_order_id: {client_order_id}")
        print(f"   block_height: {block_height}")

        await submitter.submit_limit_order(
            wallet=wallet,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            side=side,
            price=price,
            quantity=quantity,
            time_in_force=time_in_force,
            post_only=post_only,
            reduce_only=reduce_only,
            block_height=block_height,
            expire_time=expire_time,
        )
        print("   [OK] Limit order submitted successfully!")

        # Wait a bit for confirmation
        await asyncio.sleep(2)

        # Cancel the order - need fresh block height
        print("\n9. Testing order cancellation...")
        fresh_block_height = await grpc_client.latest_block_height()
        print(f"   Fresh block height: {fresh_block_height}")
        await submitter.cancel_order(
            wallet=wallet,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            block_height=fresh_block_height,
        )
        print("   [OK] Order canceled successfully!")

    except Exception as e:
        print(f"   [FAILED] Order submission failed: {e}")
        import traceback

        traceback.print_exc()

    print("\n" + "=" * 50)
    print("[OK] All Rust gRPC binding tests completed!")
    print("=" * 50)


if __name__ == "__main__":
    asyncio.run(test_rust_grpc_bindings())
