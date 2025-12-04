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
Kraken Spot WebSocket API order placement.

Places orders via WebSocket add_order method on mainnet.
Uses REST only for instrument loading and cancel_all_orders cleanup.

Order Types Tested:
    1. Market order (buy and sell)
    2. Limit order (with post-only)
    3. IOC (Immediate-Or-Cancel) limit order

Environment Variables:
    KRAKEN_SPOT_API_KEY: Your Kraken Spot API key
    KRAKEN_SPOT_API_SECRET: Your Kraken Spot API secret

Usage:
    export KRAKEN_SPOT_API_KEY="your_key"
    export KRAKEN_SPOT_API_SECRET="your_secret"
    python kraken_spot_ws.py

"""

import asyncio
import os
from datetime import datetime

from nautilus_trader.core.nautilus_pyo3 import AccountId
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import KrakenEnvironment
from nautilus_trader.core.nautilus_pyo3 import KrakenSpotHttpClient
from nautilus_trader.core.nautilus_pyo3 import KrakenSpotWebSocketClient


# -----------------------------------------------------------------------------
# Configuration Constants
# -----------------------------------------------------------------------------
SYMBOL = "ATOM/USDC"
INSTRUMENT_ID_STR = "ATOM/USDC.KRAKEN"
QTY = 0.5
ACCOUNT_ID_STR = "KRAKEN-001"

# Price multipliers
LIMIT_BUY_MULTIPLIER = 0.95   # 5% below market
LIMIT_SELL_MULTIPLIER = 1.05  # 5% above market

# Timing
SHORT_WAIT = 1   # seconds
LONG_WAIT = 2    # seconds
WS_TIMEOUT = 10.0  # seconds

# Reference price for ATOM/USDC
REFERENCE_PRICE = 7.50


def gen_cl_ord_id(prefix: str) -> str:
    """
    Generate a client order ID (max 18 chars for Kraken).
    """
    ts = int(datetime.now().timestamp()) % 100000
    return f"{prefix}{ts}"


def round_price(price: float, decimals: int = 4) -> float:
    """
    Round price to specified decimal places.
    """
    return round(price, decimals)


def handle_ws_message(msg: object) -> None:
    """
    Handle WebSocket messages (execution reports, etc.).
    """
    print(f"    [WS] Received: {type(msg).__name__}")


async def run_order_tests(
    rest_client: KrakenSpotHttpClient,
    ws_client: KrakenSpotWebSocketClient,
    account_id: AccountId,
) -> None:
    """
    Test order placement via WebSocket add_order method.
    """
    instrument_id = InstrumentId.from_str(INSTRUMENT_ID_STR)

    # Load instruments via REST and cache in both clients
    print("\n[SETUP] Loading instruments...")
    instruments = await rest_client.request_instruments()
    for inst in instruments:
        rest_client.cache_instrument(inst)
        ws_client.cache_instrument(inst)
    print(f"    Loaded {len(instruments)} instruments")

    # Connect WebSocket
    print("\n[SETUP] Connecting WebSocket...")
    await ws_client.connect(instruments, handle_ws_message)
    await ws_client.wait_until_active(timeout_secs=WS_TIMEOUT)
    print(f"    Connected to {ws_client.url}")

    # Authenticate for private channels
    print("    Authenticating...")
    await ws_client.authenticate()
    ws_client.set_account_id(account_id)

    # Subscribe to executions
    print("    Subscribing to executions...")
    await ws_client.subscribe_executions(snap_orders=True, snap_trades=True)
    print("    WebSocket ready for order placement")

    print(f"    Reference price: ${REFERENCE_PRICE:.4f}")

    # Cancel all open orders first via REST
    print("\n[SETUP] Canceling all open orders via REST...")
    cancelled = await rest_client.cancel_all_orders()
    print(f"    Cancelled: {cancelled}")

    await asyncio.sleep(SHORT_WAIT)

    # =========================================================================
    # TEST 1: MARKET order (BUY) via WebSocket
    # =========================================================================
    print("\n[TEST 1] MARKET BUY Order (via WebSocket add_order)")
    print(f"    Symbol: {SYMBOL}, Qty: {QTY}")

    cl_ord_id = gen_cl_ord_id("ws-mkt-b-")
    ws_client.cache_client_order(cl_ord_id, instrument_id)

    try:
        await ws_client.add_order(
            order_type="market",
            side="buy",
            order_qty=QTY,
            symbol=SYMBOL,
            cl_ord_id=cl_ord_id,
        )
        print(f"    SUCCESS: cl_ord_id={cl_ord_id}")
    except Exception as e:
        print(f"    FAILED: {e}")

    await asyncio.sleep(LONG_WAIT)

    # =========================================================================
    # TEST 2: MARKET order (SELL) via WebSocket - close position
    # =========================================================================
    print("\n[TEST 2] MARKET SELL Order (via WebSocket add_order)")
    print(f"    Symbol: {SYMBOL}, Qty: {QTY}")

    cl_ord_id = gen_cl_ord_id("ws-mkt-s-")
    ws_client.cache_client_order(cl_ord_id, instrument_id)

    try:
        await ws_client.add_order(
            order_type="market",
            side="sell",
            order_qty=QTY,
            symbol=SYMBOL,
            cl_ord_id=cl_ord_id,
        )
        print(f"    SUCCESS: cl_ord_id={cl_ord_id}")
    except Exception as e:
        print(f"    FAILED: {e}")

    await asyncio.sleep(LONG_WAIT)

    # =========================================================================
    # TEST 3: LIMIT order (BUY) with post-only via WebSocket
    # =========================================================================
    limit_buy_price = round_price(REFERENCE_PRICE * LIMIT_BUY_MULTIPLIER)
    print("\n[TEST 3] LIMIT BUY Order (post-only, via WebSocket)")
    print(f"    Symbol: {SYMBOL}, Qty: {QTY}, Price: ${limit_buy_price:.4f}")

    cl_ord_id = gen_cl_ord_id("ws-lmt-b-")
    ws_client.cache_client_order(cl_ord_id, instrument_id)

    try:
        await ws_client.add_order(
            order_type="limit",
            side="buy",
            order_qty=QTY,
            symbol=SYMBOL,
            limit_price=limit_buy_price,
            cl_ord_id=cl_ord_id,
            time_in_force="gtc",
            post_only=True,
        )
        print(f"    SUCCESS: cl_ord_id={cl_ord_id}")
    except Exception as e:
        print(f"    FAILED: {e}")

    await asyncio.sleep(SHORT_WAIT)

    # Cancel via REST
    print("    Canceling limit order via REST...")
    cancelled = await rest_client.cancel_all_orders()
    print(f"    Cancelled: {cancelled}")

    await asyncio.sleep(SHORT_WAIT)

    # =========================================================================
    # TEST 4: LIMIT order (SELL) via WebSocket
    # =========================================================================
    print("\n[TEST 4] LIMIT SELL Order (via WebSocket)")
    print("    Buying position first...")

    cl_ord_id = gen_cl_ord_id("ws-pre-")
    ws_client.cache_client_order(cl_ord_id, instrument_id)

    try:
        await ws_client.add_order(
            order_type="market",
            side="buy",
            order_qty=QTY,
            symbol=SYMBOL,
            cl_ord_id=cl_ord_id,
        )
    except Exception as e:
        print(f"    Pre-buy failed: {e}")

    await asyncio.sleep(LONG_WAIT)

    limit_sell_price = round_price(REFERENCE_PRICE * LIMIT_SELL_MULTIPLIER)
    print(f"    Symbol: {SYMBOL}, Qty: {QTY}, Price: ${limit_sell_price:.4f}")

    cl_ord_id = gen_cl_ord_id("ws-lmt-s-")
    ws_client.cache_client_order(cl_ord_id, instrument_id)

    try:
        await ws_client.add_order(
            order_type="limit",
            side="sell",
            order_qty=QTY,
            symbol=SYMBOL,
            limit_price=limit_sell_price,
            cl_ord_id=cl_ord_id,
            time_in_force="gtc",
        )
        print(f"    SUCCESS: cl_ord_id={cl_ord_id}")
    except Exception as e:
        print(f"    FAILED: {e}")

    await asyncio.sleep(SHORT_WAIT)

    # Cancel and close position
    print("    Canceling limit sell order via REST...")
    await rest_client.cancel_all_orders()

    # Close at market
    print("    Closing position at market...")
    cl_ord_id = gen_cl_ord_id("ws-cls-")
    ws_client.cache_client_order(cl_ord_id, instrument_id)

    try:
        await ws_client.add_order(
            order_type="market",
            side="sell",
            order_qty=QTY,
            symbol=SYMBOL,
            cl_ord_id=cl_ord_id,
        )
    except Exception as e:
        print(f"    Close failed: {e}")

    await asyncio.sleep(LONG_WAIT)

    # =========================================================================
    # TEST 5: IOC LIMIT order via WebSocket
    # =========================================================================
    ioc_price = round_price(REFERENCE_PRICE * LIMIT_BUY_MULTIPLIER)
    print("\n[TEST 5] IOC LIMIT BUY Order (via WebSocket)")
    print(f"    Symbol: {SYMBOL}, Qty: {QTY}, Price: ${ioc_price:.4f}")
    print("    Note: Should cancel immediately if not filled")

    cl_ord_id = gen_cl_ord_id("ws-ioc-")
    ws_client.cache_client_order(cl_ord_id, instrument_id)

    try:
        await ws_client.add_order(
            order_type="limit",
            side="buy",
            order_qty=QTY,
            symbol=SYMBOL,
            limit_price=ioc_price,
            cl_ord_id=cl_ord_id,
            time_in_force="ioc",
        )
        print(f"    SUCCESS: cl_ord_id={cl_ord_id}")
    except Exception as e:
        print(f"    FAILED (expected if not filled): {e}")

    await asyncio.sleep(SHORT_WAIT)

    # =========================================================================
    # Cleanup
    # =========================================================================
    print("\n[CLEANUP] Canceling any remaining orders...")
    cancelled = await rest_client.cancel_all_orders()
    print(f"    Cancelled: {cancelled}")

    print("\n[CLEANUP] Disconnecting WebSocket...")
    await ws_client.disconnect()
    print("    Disconnected")

    print("\n" + "=" * 60)
    print("WebSocket add_order Testing Complete!")
    print("Tested: Market (Buy/Sell), Limit (Buy/Sell, post-only), IOC")
    print("All orders placed via WebSocket add_order method")
    print("=" * 60)


async def main() -> None:
    api_key = os.environ.get("KRAKEN_SPOT_API_KEY")
    api_secret = os.environ.get("KRAKEN_SPOT_API_SECRET")

    if not api_key or not api_secret:
        print("Error: Set KRAKEN_SPOT_API_KEY and KRAKEN_SPOT_API_SECRET environment variables")
        return

    print("Kraken Spot WebSocket API - Order Type Testing")
    print("=" * 60)
    print("Orders placed via WebSocket add_order method")
    print("REST used only for instrument loading and cancel_all")
    print("=" * 60)

    # Create REST client for instrument loading and cancel_all
    rest_client = KrakenSpotHttpClient(
        api_key=api_key,
        api_secret=api_secret,
    )

    # Create WebSocket client for order placement
    ws_client = KrakenSpotWebSocketClient(
        environment=KrakenEnvironment.MAINNET,
        api_key=api_key,
        api_secret=api_secret,
    )

    account_id = AccountId(ACCOUNT_ID_STR)
    await run_order_tests(rest_client, ws_client, account_id)


if __name__ == "__main__":
    asyncio.run(main())
