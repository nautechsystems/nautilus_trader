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
Kraken Futures order placement demo.

Demonstrates order placement for various order types on Kraken Futures testnet.

Environment Variables:
    KRAKEN_TESTNET_API_KEY: Your Kraken demo API key
    KRAKEN_TESTNET_API_SECRET: Your Kraken demo API secret

Usage:
    export KRAKEN_TESTNET_API_KEY="your_key"
    export KRAKEN_TESTNET_API_SECRET="your_secret"
    python kraken_futures_demo.py

"""

import asyncio
import os
from datetime import datetime

from nautilus_trader.core.nautilus_pyo3 import AccountId
from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import KrakenFuturesHttpClient
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import OrderType
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import TimeInForce
from nautilus_trader.core.nautilus_pyo3 import VenueOrderId


async def run_order_tests(client: KrakenFuturesHttpClient, account_id: AccountId) -> None:
    """
    Test order placement with various order types.
    """
    instrument_id = InstrumentId.from_str("PI_XBTUSD.KRAKEN")
    placed_order_ids: list[VenueOrderId] = []
    results = {"passed": 0, "failed": 0}

    instruments = await client.request_instruments()
    for inst in instruments:
        client.cache_instrument(inst)

    mark_price = float(await client.request_mark_price(instrument_id))
    print(f"Reference price: {mark_price}")

    # Test 1: LIMIT order (post-only)
    print("\n[1] LIMIT order (post-only, 50% below market)...")
    try:
        limit_price = mark_price * 0.50
        report = await client.submit_order(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=ClientOrderId(f"limit-{int(datetime.now().timestamp())}"),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            quantity=Quantity.from_int(1),
            time_in_force=TimeInForce.GTC,
            price=Price.from_str(f"{limit_price:.2f}"),
            post_only=True,
        )
        print(f"    Order: {report.venue_order_id}, Status: {report.order_status}")
        placed_order_ids.append(report.venue_order_id)
        results["passed"] += 1
    except Exception as e:
        print(f"    FAILED: {e}")
        results["failed"] += 1

    # Test 2: Cancel order
    print("\n[2] Cancel LIMIT order...")
    try:
        if placed_order_ids:
            report = await client.cancel_order(
                account_id=account_id,
                instrument_id=instrument_id,
                venue_order_id=placed_order_ids.pop(),
            )
            print(f"    Status: {report.order_status}")
            results["passed"] += 1
        else:
            print("    SKIPPED: No order to cancel")
    except Exception as e:
        print(f"    FAILED: {e}")
        results["failed"] += 1

    # Test 3: STOP_MARKET order
    print("\n[3] STOP_MARKET order (trigger at 60% of market)...")
    try:
        stop_price = mark_price * 0.60
        report = await client.submit_order(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=ClientOrderId(f"stop-{int(datetime.now().timestamp())}"),
            order_side=OrderSide.SELL,
            order_type=OrderType.STOP_MARKET,
            quantity=Quantity.from_int(1),
            time_in_force=TimeInForce.GTC,
            trigger_price=Price.from_str(f"{stop_price:.2f}"),
        )
        print(f"    Order: {report.venue_order_id}, Status: {report.order_status}")
        placed_order_ids.append(report.venue_order_id)
        results["passed"] += 1
    except Exception as e:
        print(f"    FAILED: {e}")
        results["failed"] += 1

    # Test 4: STOP_LIMIT order
    print("\n[4] STOP_LIMIT order (trigger at 65%, limit at 64%)...")
    try:
        stop_trigger = mark_price * 0.65
        stop_limit = mark_price * 0.64
        report = await client.submit_order(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=ClientOrderId(f"stop-limit-{int(datetime.now().timestamp())}"),
            order_side=OrderSide.SELL,
            order_type=OrderType.STOP_LIMIT,
            quantity=Quantity.from_int(1),
            time_in_force=TimeInForce.GTC,
            price=Price.from_str(f"{stop_limit:.2f}"),
            trigger_price=Price.from_str(f"{stop_trigger:.2f}"),
        )
        print(f"    Order: {report.venue_order_id}, Status: {report.order_status}")
        placed_order_ids.append(report.venue_order_id)
        results["passed"] += 1
    except Exception as e:
        print(f"    FAILED: {e}")
        results["failed"] += 1

    # Test 5: MARKET order
    print("\n[5] MARKET order...")
    try:
        report = await client.submit_order(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=ClientOrderId(f"market-{int(datetime.now().timestamp())}"),
            order_side=OrderSide.BUY,
            order_type=OrderType.MARKET,
            quantity=Quantity.from_int(1),
            time_in_force=TimeInForce.IOC,
        )
        print(f"    Status: {report.order_status}, Filled: {report.filled_qty}")
        results["passed"] += 1
    except Exception as e:
        print(f"    FAILED: {e}")
        results["failed"] += 1

    # Cleanup: Cancel any remaining orders
    if placed_order_ids:
        print(f"\n[Cleanup] Cancelling {len(placed_order_ids)} orders...")
        try:
            cancelled = await client.cancel_all_orders(instrument_id=instrument_id)
            print(f"    Cancelled: {cancelled}")
        except Exception as e:
            print(f"    Cleanup failed: {e}")

    print(f"\nResults: {results['passed']} passed, {results['failed']} failed")


async def main():
    api_key = os.environ.get("KRAKEN_TESTNET_API_KEY")
    api_secret = os.environ.get("KRAKEN_TESTNET_API_SECRET")

    if not api_key or not api_secret:
        print("Set KRAKEN_TESTNET_API_KEY and KRAKEN_TESTNET_API_SECRET")
        return

    client = KrakenFuturesHttpClient(
        api_key=api_key,
        api_secret=api_secret,
        testnet=True,
    )

    account_id = AccountId("KRAKEN-001")
    await run_order_tests(client, account_id)


if __name__ == "__main__":
    asyncio.run(main())
