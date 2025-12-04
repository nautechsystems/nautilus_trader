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
Kraken Spot REST API order placement.

Places orders for various order types on Kraken Spot mainnet via REST API.

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
    python kraken_spot_rest.py

"""

import asyncio
import os
from datetime import datetime

from nautilus_trader.core.nautilus_pyo3 import AccountId
from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import KrakenSpotHttpClient
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import OrderType
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import TimeInForce
from nautilus_trader.core.nautilus_pyo3 import VenueOrderId


# -----------------------------------------------------------------------------
# Configuration Constants
# -----------------------------------------------------------------------------
SYMBOL = "ATOM/USDC"
INSTRUMENT_ID_STR = "ATOM/USDC.KRAKEN"
QTY = "0.5"
ACCOUNT_ID_STR = "KRAKEN-001"

# Price multipliers
LIMIT_BUY_MULTIPLIER = 0.95   # 5% below market
LIMIT_SELL_MULTIPLIER = 1.05  # 5% above market

# Timing
SHORT_WAIT = 1  # seconds
LONG_WAIT = 2   # seconds


async def submit_market_order(
    client: KrakenSpotHttpClient,
    account_id: AccountId,
    instrument_id: InstrumentId,
    side: OrderSide,
    prefix: str,
) -> None:
    """
    Submit a market order and print result.
    """
    client_order_id = ClientOrderId(f"{prefix}-{int(datetime.now().timestamp())}")
    try:
        venue_order_id = await client.submit_order(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=side,
            order_type=OrderType.MARKET,
            quantity=Quantity.from_str(QTY),
            time_in_force=TimeInForce.IOC,
        )
        print(f"    SUCCESS: {venue_order_id}")
    except Exception as e:
        print(f"    FAILED: {e}")


async def submit_limit_order(
    client: KrakenSpotHttpClient,
    account_id: AccountId,
    instrument_id: InstrumentId,
    side: OrderSide,
    price: float,
    prefix: str,
    time_in_force: TimeInForce = TimeInForce.GTC,
    post_only: bool = False,
) -> VenueOrderId | None:
    """
    Submit a limit order and return venue order ID on success.
    """
    client_order_id = ClientOrderId(f"{prefix}-{int(datetime.now().timestamp())}")
    try:
        venue_order_id = await client.submit_order(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=side,
            order_type=OrderType.LIMIT,
            quantity=Quantity.from_str(QTY),
            time_in_force=time_in_force,
            price=Price.from_str(f"{price:.4f}"),
            post_only=post_only,
        )
        print(f"    SUCCESS: {venue_order_id}")
        return venue_order_id
    except Exception as e:
        print(f"    FAILED: {e}")
        return None


async def run_order_tests(client: KrakenSpotHttpClient, account_id: AccountId) -> None:
    """
    Test order placement with various order types via REST API.
    """
    instrument_id = InstrumentId.from_str(INSTRUMENT_ID_STR)

    print("\n[SETUP] Loading instruments...")
    instruments = await client.request_instruments()
    for inst in instruments:
        client.cache_instrument(inst)
    print(f"    Loaded {len(instruments)} instruments")

    reference_price = 7.50
    print(f"    Reference price: ${reference_price:.4f}")

    print("\n[SETUP] Canceling all open orders...")
    cancelled = await client.cancel_all_orders()
    print(f"    Cancelled: {cancelled}")
    await asyncio.sleep(SHORT_WAIT)

    # TEST 1: MARKET BUY
    print(f"\n[TEST 1] MARKET BUY Order\n    Symbol: {SYMBOL}, Qty: {QTY}")
    await submit_market_order(client, account_id, instrument_id, OrderSide.BUY, "market-buy")
    await asyncio.sleep(LONG_WAIT)

    # TEST 2: MARKET SELL
    print(f"\n[TEST 2] MARKET SELL Order\n    Symbol: {SYMBOL}, Qty: {QTY}")
    await submit_market_order(client, account_id, instrument_id, OrderSide.SELL, "market-sell")
    await asyncio.sleep(LONG_WAIT)

    # TEST 3: LIMIT BUY (post-only)
    limit_buy_price = reference_price * LIMIT_BUY_MULTIPLIER
    print(f"\n[TEST 3] LIMIT BUY Order (post-only)\n    Symbol: {SYMBOL}, Qty: {QTY}, Price: ${limit_buy_price:.4f}")
    venue_id = await submit_limit_order(client, account_id, instrument_id, OrderSide.BUY, limit_buy_price, "limit-buy", post_only=True)
    await asyncio.sleep(SHORT_WAIT)
    if venue_id:
        print("    Canceling limit order...")
        try:
            report = await client.cancel_order(account_id=account_id, instrument_id=instrument_id, venue_order_id=venue_id)
            print(f"    Cancelled: {report.order_status}")
        except Exception as e:
            print(f"    Cancel failed: {e}")
    await asyncio.sleep(SHORT_WAIT)

    # TEST 4: LIMIT SELL
    print("\n[TEST 4] LIMIT SELL Order\n    Buying position first...")
    await submit_market_order(client, account_id, instrument_id, OrderSide.BUY, "pre-buy")
    await asyncio.sleep(LONG_WAIT)
    limit_sell_price = reference_price * LIMIT_SELL_MULTIPLIER
    print(f"    Symbol: {SYMBOL}, Qty: {QTY}, Price: ${limit_sell_price:.4f}")
    venue_id = await submit_limit_order(client, account_id, instrument_id, OrderSide.SELL, limit_sell_price, "limit-sell")
    await asyncio.sleep(SHORT_WAIT)
    if venue_id:
        print("    Canceling limit sell order...")
        try:
            await client.cancel_order(account_id=account_id, instrument_id=instrument_id, venue_order_id=venue_id)
        except Exception as e:
            print(f"    Cancel failed: {e}")
    print("    Closing position at market...")
    await submit_market_order(client, account_id, instrument_id, OrderSide.SELL, "close")
    await asyncio.sleep(LONG_WAIT)

    # TEST 5: IOC LIMIT BUY
    ioc_price = reference_price * LIMIT_BUY_MULTIPLIER
    print(f"\n[TEST 5] IOC LIMIT BUY Order\n    Symbol: {SYMBOL}, Qty: {QTY}, Price: ${ioc_price:.4f}")
    print("    Note: Should cancel immediately if not filled")
    await submit_limit_order(client, account_id, instrument_id, OrderSide.BUY, ioc_price, "ioc", time_in_force=TimeInForce.IOC)
    await asyncio.sleep(SHORT_WAIT)

    # Cleanup
    print("\n[CLEANUP] Canceling any remaining orders...")
    cancelled = await client.cancel_all_orders()
    print(f"    Cancelled: {cancelled}")

    print("\n" + "=" * 50)
    print("REST API Order Type Testing Complete!")
    print("Tested: Market (Buy/Sell), Limit (Buy/Sell, post-only), IOC")
    print("=" * 50)


async def main() -> None:
    api_key = os.environ.get("KRAKEN_SPOT_API_KEY")
    api_secret = os.environ.get("KRAKEN_SPOT_API_SECRET")

    if not api_key or not api_secret:
        print("Error: Set KRAKEN_SPOT_API_KEY and KRAKEN_SPOT_API_SECRET environment variables")
        return

    print("Kraken Spot REST API - Order Type Testing")
    print("=" * 50)

    client = KrakenSpotHttpClient(
        api_key=api_key,
        api_secret=api_secret,
    )

    account_id = AccountId(ACCOUNT_ID_STR)
    await run_order_tests(client, account_id)


if __name__ == "__main__":
    asyncio.run(main())
