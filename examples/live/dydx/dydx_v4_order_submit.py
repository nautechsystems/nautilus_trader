#!/usr/bin/env python3
"""
Simple test of dYdX v4 order submission via Rust bindings.

This demonstrates the low-level order submission without the full NautilusTrader stack.

"""

import asyncio
import os

from nautilus_trader.core.nautilus_pyo3 import dydx  # type: ignore[attr-defined]


async def main():
    """
    Test direct order submission via Rust bindings.
    """
    # Check credentials
    wallet_address = os.getenv("DYDX_WALLET_ADDRESS")
    mnemonic = os.getenv("DYDX_MNEMONIC")

    if not wallet_address or not mnemonic:
        print("ERROR: Missing credentials!")
        print("Please set DYDX_WALLET_ADDRESS and DYDX_MNEMONIC environment variables")
        return

    print(f"Using wallet: {wallet_address}")
    print("=" * 60)

    # 1. Create wallet
    print("\n1. Creating wallet from mnemonic...")
    try:
        wallet = dydx.DydxWallet.from_mnemonic(mnemonic)
        print("[OK] Wallet created")
    except Exception as e:
        print(f"[FAIL] Failed to create wallet: {e}")
        print(f"   Mnemonic has {len(mnemonic.split())} words (needs 24)")
        return

    # 2. Create HTTP client
    print("\n2. Creating HTTP client...")
    http_client = dydx.DydxHttpClient(
        base_url=None,  # Uses default mainnet URL
        is_testnet=False,
    )
    print("[OK] HTTP client created (mainnet)")

    # 3. Load instruments
    print("\n3. Loading instruments...")
    # fetch_and_cache_instruments() caches both instruments AND market params
    await http_client.fetch_and_cache_instruments()

    # Retrieve cached instruments
    instruments = await http_client.request_instruments(
        maker_fee=None,  # Will use default fees from API
        taker_fee=None,
    )
    print(f"[OK] Loaded and cached {len(instruments)} instruments")

    # Find ETH-USD-PERP (exact match to avoid WSTETH-USD etc)
    eth_instrument = None
    for inst in instruments:
        inst_id = str(inst.id)
        if inst_id.startswith("ETH-USD-PERP"):
            eth_instrument = inst
            break

    if not eth_instrument:
        print("ERROR: ETH-USD instrument not found")
        return

    print(f"[OK] Found instrument: {eth_instrument.id}")

    # 4. Create gRPC client
    print("\n4. Connecting to gRPC...")
    grpc_urls = [
        "https://dydx-grpc.publicnode.com:443",
        "https://dydx-mainnet-grpc.allthatnode.com:443",
    ]
    try:
        grpc_client = await dydx.DydxGrpcClient.connect_with_fallback(grpc_urls)
        print("[OK] gRPC client connected")
    except Exception as e:
        print(f"[FAIL] Failed to connect to gRPC: {e}")
        return

    # 5. Create order submitter
    print("\n5. Creating order submitter...")
    order_submitter = dydx.DydxOrderSubmitter(
        grpc_client=grpc_client,
        http_client=http_client,
        wallet_address=wallet_address,
        subaccount_number=0,
        chain_id="dydx-mainnet-1",
    )
    print("[OK] Order submitter created (mainnet)")

    # 5b. Get current block height for order validity
    print("\n5b. Fetching current block height...")
    current_block = await grpc_client.latest_block_height()
    # Pass current block - Rust code adds SHORT_TERM_ORDER_MAXIMUM_LIFETIME (20) internally
    print(f"[OK] Current block: {current_block}")

    # 6. Submit a limit order
    print("\n6. Submitting test limit order...")
    print(f"   Instrument: {eth_instrument.id}")
    print("   Side: BUY (1)")
    print("   Price: $2000.00")
    print("   Quantity: 0.01")
    print("   Client Order ID: 12345")

    try:
        await order_submitter.submit_limit_order(
            wallet=wallet,
            instrument_id=str(eth_instrument.id),
            client_order_id=12345,
            side=1,  # BUY = 1
            price="2000.00",
            quantity="0.01",
            time_in_force=1,  # GTC = 1
            post_only=True,
            reduce_only=False,
            block_height=current_block,
            expire_time=None,
        )
        print("[OK] Order submitted successfully!")

    except Exception as e:
        print(f"[FAIL] Order submission failed: {e}")
        import traceback

        traceback.print_exc()
        return

    # 7. Cancel the order
    print("\n7. Canceling the order...")
    print("   Client Order ID: 12345")

    try:
        await order_submitter.cancel_order(
            wallet=wallet,
            instrument_id=str(eth_instrument.id),
            client_order_id=12345,
            block_height=current_block,
        )
        print("[OK] Order canceled successfully!")
        print("\nCheck dYdX mainnet UI to verify:")
        print("https://dydx.trade/portfolio/orders")

    except Exception as e:
        print(f"[FAIL] Order cancellation failed: {e}")
        import traceback

        traceback.print_exc()

    print("\n" + "=" * 60)
    print("Test complete!")


if __name__ == "__main__":
    asyncio.run(main())
