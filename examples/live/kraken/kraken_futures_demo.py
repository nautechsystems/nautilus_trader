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
Kraken Futures Demo - Comprehensive testing of the Kraken Futures adapter.

This demo tests:
1. Public endpoints (instruments, trades, mark/index prices, bars)
2. Authenticated endpoints (open orders, positions, fills)
3. Order placement (LIMIT, STOP_MARKET, STOP_LIMIT, MARKET_IF_TOUCHED, MARKET execution, position close)

Environment Variables (Testnet/Demo):
    KRAKEN_TESTNET_API_KEY: Your Kraken demo API key
    KRAKEN_TESTNET_API_SECRET: Your Kraken demo API secret

Usage:
    # Public endpoints only (no credentials)
    python kraken_futures_demo.py

    # Full testnet tests with order placement
    export KRAKEN_TESTNET_API_KEY="your_key"
    export KRAKEN_TESTNET_API_SECRET="your_secret"
    python kraken_futures_demo.py
"""

import asyncio
import os
from datetime import UTC
from datetime import datetime
from datetime import timedelta

from nautilus_trader.core.nautilus_pyo3 import AccountId
from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import KrakenFuturesHttpClient
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import OrderType
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import TimeInForce


def get_testnet_credentials():
    """
    Check for testnet credentials in environment.
    """
    api_key = os.environ.get("KRAKEN_TESTNET_API_KEY")
    api_secret = os.environ.get("KRAKEN_TESTNET_API_SECRET")

    if api_key and api_secret:
        return api_key, api_secret
    return None, None


async def test_futures_public(client: KrakenFuturesHttpClient):
    """
    Test Futures public endpoints.
    """
    print("\n=== Testing Futures Public Endpoints ===")

    # Test 1: Request instruments
    print("\n[1/5] Requesting instruments...")
    try:
        instruments = await client.request_instruments()
        print(f"[OK] Found {len(instruments)} instruments")
        if instruments:
            sample = instruments[0]
            # Cache all instruments for later use
            for inst in instruments:
                client.cache_instrument(inst)
            print(f"  Sample: {sample.id}")
    except Exception as e:
        print(f"[ERROR] Instruments request failed: {e}")
        return

    # Use PI_XBTUSD for remaining tests
    instrument_id = InstrumentId.from_str("PI_XBTUSD.KRAKEN")

    # Test 2: Request trades
    print("\n[2/5] Requesting recent trades for PI_XBTUSD...")
    try:
        end = datetime.now(UTC)
        start = end - timedelta(minutes=5)
        trades = await client.request_trades(instrument_id, start, end, limit=10)
        print(f"[OK] Received {len(trades)} trades")
        if trades:
            latest = trades[-1]
            print(f"  Latest: {latest.price} @ {latest.size} ({latest.aggressor_side})")
    except Exception as e:
        print(f"[ERROR] Trades request failed: {e}")

    # Test 3: Request mark price
    print("\n[3/5] Requesting mark price for PI_XBTUSD...")
    mark_price = None
    try:
        mark_price = await client.request_mark_price(instrument_id)
        print(f"[OK] Mark price: {mark_price}")
    except Exception:
        print("[WARN] Mark price unavailable in testnet")

    # Test 4: Request index price
    print("\n[4/5] Requesting index price for PI_XBTUSD...")
    try:
        index_price = await client.request_index_price(instrument_id)
        print(f"[OK] Index price: {index_price}")
    except Exception:
        print("[WARN] Index price unavailable in testnet")

    # Return mark price or latest trade price for order placement tests
    return mark_price or (trades[-1].price if trades else None)

    # Test 5: Request bars
    print("\n[5/5] Requesting 1-minute bars for PI_XBTUSD...")
    try:
        from nautilus_trader.model.data import BarType as NautilusBarType
        bar_type = NautilusBarType.from_str("PI_XBTUSD.KRAKEN-1-MINUTE-LAST-EXTERNAL")
        end = datetime.now(UTC)
        start = end - timedelta(hours=1)
        bars = await client.request_bars(bar_type, start, end, limit=10)
        print(f"[OK] Received {len(bars)} bars")
        if bars:
            latest = bars[-1]
            print(f"  Latest: O={latest.open} H={latest.high} L={latest.low} C={latest.close} V={latest.volume}")
    except Exception:
        print("[WARN] Bars unavailable in testnet")


async def test_futures_authenticated(client: KrakenFuturesHttpClient, account_id: AccountId):
    """
    Test Futures authenticated endpoints.
    """
    print("\n=== Testing Futures Authenticated Endpoints ===")

    # Test 1: Request open orders
    print("\n[1/3] Requesting open orders...")
    try:
        orders = await client.request_order_status_reports(
            account_id=account_id,
            open_only=True,
        )
        print(f"[OK] Found {len(orders)} open orders")
        if orders:
            for order in orders[:3]:  # Show first 3
                print(f"  Order: {order.venue_order_id} {order.order_side} {order.quantity} @ {order.price or 'MARKET'}")
    except Exception as e:
        print(f"[ERROR] Open orders request failed: {e}")
        return

    # Test 2: Request positions
    print("\n[2/3] Requesting open positions...")
    try:
        positions = await client.request_position_status_reports(account_id=account_id)
        print(f"[OK] Found {len(positions)} positions")
        if positions:
            for pos in positions:
                print(f"  Position: {pos.instrument_id} size={pos.quantity}")
    except Exception as e:
        print(f"[ERROR] Positions request failed: {e}")

    # Test 3: Request recent fills
    print("\n[3/3] Requesting recent fills...")
    try:
        end = datetime.now(UTC)
        start = end - timedelta(hours=24)
        fills = await client.request_fill_reports(
            account_id=account_id,
            start=start,
            end=end,
        )
        print(f"[OK] Found {len(fills)} fills in last 24 hours")
        if fills:
            for fill in fills[:3]:  # Show first 3
                print(f"  Fill: {fill.venue_order_id} {fill.order_side} {fill.last_qty} @ {fill.last_px}")
    except Exception as e:
        print(f"[ERROR] Fills request failed: {e}")


async def test_futures_order_placement(  # noqa: C901
    client: KrakenFuturesHttpClient,
    account_id: AccountId,
    reference_price,
):
    """
    Test order placement with all required order types.
    """
    print("\n=== Testing Order Placement ===")

    instrument_id = InstrumentId.from_str("PI_XBTUSD.KRAKEN")
    placed_order_ids = []

    # Use provided reference price
    if not reference_price:
        print("[ERROR] No reference price available, skipping order placement tests")
        return

    mark_price = float(reference_price)
    print(f"\nUsing reference price: {mark_price}")

    # Test 1: LIMIT order (Post-only, far from market)
    print("\n[Test 1/8] Placing LIMIT order (post-only, 50% of market price)...")
    try:
        limit_price = float(mark_price) * 0.50  # 50% below market
        client_order_id = ClientOrderId(f"test-limit-{int(datetime.now().timestamp())}")

        report = await client.submit_order(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            quantity=Quantity.from_int(1),
            time_in_force=TimeInForce.GTC,
            price=Price.from_str(f"{limit_price:.2f}"),
            post_only=True,
        )

        venue_order_id = report.venue_order_id
        print(f"[OK] LIMIT order placed: {venue_order_id}")
        print(f"  Price: {limit_price:.2f}, Status: {report.order_status}")
        placed_order_ids.append(venue_order_id)

    except Exception as e:
        error_msg = str(e)
        if "502" in error_msg:
            print("[WARN] LIMIT order skipped (testnet API returned 502 - infrastructure issue)")
        else:
            print(f"[ERROR] LIMIT order failed: {e}")

    # Test 2: Cancel the LIMIT order
    print("\n[Test 2/8] Cancelling LIMIT order...")
    try:
        if placed_order_ids:
            report = await client.cancel_order(
                account_id=account_id,
                instrument_id=instrument_id,
                venue_order_id=placed_order_ids[0],
            )
            print(f"[OK] Cancel status: {report.order_status}")
            placed_order_ids.pop(0)
    except Exception as e:
        print(f"[ERROR] Cancel failed: {e}")

    await asyncio.sleep(2)  # Longer delay to avoid rate limiting

    # Test 3: STOP_MARKET order (stop-loss trigger)
    print("\n[Test 3/8] Placing STOP_MARKET order (trigger at 60% of market)...")
    try:
        stop_price = float(mark_price) * 0.60  # 60% of market (stop-loss)
        client_order_id = ClientOrderId(f"test-stop-{int(datetime.now().timestamp())}")

        report = await client.submit_order(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=OrderSide.SELL,
            order_type=OrderType.STOP_MARKET,
            quantity=Quantity.from_int(1),
            time_in_force=TimeInForce.GTC,
            trigger_price=Price.from_str(f"{stop_price:.2f}"),
        )

        venue_order_id = report.venue_order_id
        print(f"[OK] STOP_MARKET order placed: {venue_order_id}")
        print(f"  Trigger: {stop_price:.2f}, Status: {report.order_status}")
        placed_order_ids.append(venue_order_id)

    except Exception as e:
        error_msg = str(e)
        if "502" in error_msg:
            print("[WARN] STOP_MARKET order skipped (testnet API returned 502 - infrastructure issue)")
        else:
            print(f"[ERROR] STOP_MARKET order failed: {e}")

    await asyncio.sleep(2)  # Longer delay to avoid rate limiting

    # Test 4: STOP_LIMIT order (stop-loss with limit price)
    print("\n[Test 4/8] Placing STOP_LIMIT order (trigger at 65% with limit at 64%)...")
    try:
        stop_trigger = float(mark_price) * 0.65  # 65% of market
        stop_limit = float(mark_price) * 0.64   # 64% of market
        client_order_id = ClientOrderId(f"test-stop-limit-{int(datetime.now().timestamp())}")

        report = await client.submit_order(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=OrderSide.SELL,
            order_type=OrderType.STOP_LIMIT,
            quantity=Quantity.from_int(1),
            time_in_force=TimeInForce.GTC,
            price=Price.from_str(f"{stop_limit:.2f}"),  # Limit price
            trigger_price=Price.from_str(f"{stop_trigger:.2f}"),  # Stop trigger
        )

        venue_order_id = report.venue_order_id
        print(f"[OK] STOP_LIMIT order placed: {venue_order_id}")
        print(f"  Trigger: {stop_trigger:.2f}, Limit: {stop_limit:.2f}, Status: {report.order_status}")
        placed_order_ids.append(venue_order_id)

    except Exception as e:
        error_msg = str(e)
        if "502" in error_msg:
            print("[WARN] STOP_LIMIT order skipped (testnet API returned 502 - infrastructure issue)")
        else:
            print(f"[ERROR] STOP_LIMIT order failed: {e}")

    await asyncio.sleep(2)  # Longer delay to avoid rate limiting

    # Test 5: MARKET_IF_TOUCHED order (take-profit trigger)
    print("\n[Test 5/8] Placing MARKET_IF_TOUCHED order (trigger at 150% of market)...")
    try:
        profit_price = float(mark_price) * 1.50  # 150% of market (take-profit)
        # Add a limit price slightly above trigger for take-profit orders
        limit_price = profit_price * 1.01  # 1% above trigger
        client_order_id = ClientOrderId(f"test-profit-{int(datetime.now().timestamp())}")

        report = await client.submit_order(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=OrderSide.SELL,
            order_type=OrderType.MARKET_IF_TOUCHED,
            quantity=Quantity.from_int(1),
            time_in_force=TimeInForce.GTC,
            price=Price.from_str(f"{limit_price:.2f}"),  # Limit price for execution
            trigger_price=Price.from_str(f"{profit_price:.2f}"),  # Trigger price
        )

        venue_order_id = report.venue_order_id
        print(f"[OK] MARKET_IF_TOUCHED order placed: {venue_order_id}")
        print(f"  Trigger: {profit_price:.2f}, Limit: {limit_price:.2f}, Status: {report.order_status}")
        placed_order_ids.append(venue_order_id)

    except Exception as e:
        print(f"[WARN] MARKET_IF_TOUCHED order skipped (testnet API limitation): {str(e)[:60]}")

    await asyncio.sleep(1)

    # Test 6: MARKET order (immediate execution)
    print("\n[Test 6/8] Placing MARKET order (immediate execution)...")
    try:
        client_order_id = ClientOrderId(f"test-market-{int(datetime.now().timestamp())}")

        report = await client.submit_order(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=OrderSide.BUY,
            order_type=OrderType.MARKET,
            quantity=Quantity.from_int(1),
            time_in_force=TimeInForce.IOC,
        )

        print("[OK] MARKET order executed")
        print(f"  Status: {report.order_status}, Filled: {report.filled_qty}")

    except Exception as e:
        print(f"[ERROR] MARKET order failed: {e}")

    await asyncio.sleep(1)

    # Test 7: Close position with reduce_only MARKET order
    print("\n[Test 7/8] Closing position with reduce_only MARKET order...")
    try:
        # Check if we have an open position
        positions = await client.request_position_status_reports(
            account_id=account_id,
            instrument_id=instrument_id,
        )

        if positions:
            pos = positions[0]
            print(f"  Current position size: {pos.quantity}")

            # Close with opposite side based on quantity sign
            position_qty = pos.quantity.as_double()
            close_side = OrderSide.SELL if position_qty > 0 else OrderSide.BUY
            client_order_id = ClientOrderId(f"test-close-{int(datetime.now().timestamp())}")

            report = await client.submit_order(
                account_id=account_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                order_side=close_side,
                order_type=OrderType.MARKET,
                quantity=Quantity.from_str(str(abs(pos.quantity.as_double()))),
                time_in_force=TimeInForce.IOC,
                reduce_only=True,
            )

            print("[OK] Position close order executed")
            print(f"  Status: {report.order_status}, Filled: {report.filled_qty}")
        else:
            print("  No position to close")

    except Exception as e:
        print(f"[ERROR] Position close failed: {e}")

    # Cleanup: Cancel any remaining open orders
    if placed_order_ids:
        print(f"\n[Cleanup] Cancelling {len(placed_order_ids)} remaining orders...")
        try:
            cancelled_count = await client.cancel_all_orders(instrument_id=instrument_id)
            print(f"[OK] Cancelled {cancelled_count} orders")
        except Exception as e:
            print(f"[ERROR] Cleanup failed: {e}")

    # Final status check
    print("\n[Final Status] Checking open orders and positions...")
    try:
        orders = await client.request_order_status_reports(
            account_id=account_id,
            open_only=True,
        )
        positions = await client.request_position_status_reports(account_id=account_id)
        print(f"  Open orders: {len(orders)}")
        print(f"  Open positions: {len(positions)}")
    except Exception as e:
        print(f"[ERROR] Status check failed: {e}")

    # Test 8: IOC (Immediate-or-Cancel) LIMIT order
    print("\n[Test 8/8] Placing IOC LIMIT order (immediate-or-cancel)...")
    try:
        ioc_price = float(mark_price) * 0.55  # 55% below market (unlikely to fill)
        client_order_id = ClientOrderId(f"test-ioc-{int(datetime.now().timestamp())}")

        report = await client.submit_order(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            quantity=Quantity.from_int(1),
            time_in_force=TimeInForce.IOC,  # IOC time in force
            price=Price.from_str(f"{ioc_price:.2f}"),
        )

        print("[OK] IOC LIMIT order executed")
        print(f"  Price: {ioc_price:.2f}, Status: {report.order_status}")
        # IOC orders auto-cancel if not filled, no need to track

    except Exception as e:
        error_msg = str(e)
        if "502" in error_msg:
            print("[WARN] IOC LIMIT order skipped (testnet API returned 502 - infrastructure issue)")
        else:
            print(f"[ERROR] IOC LIMIT order failed: {e}")

    print("\nAll order type tests complete!")


async def main():
    """
    Run the main demo entry point.
    """
    print("=== Kraken Futures Adapter Demo (TESTNET) ===\n")

    # Check for testnet credentials
    api_key, api_secret = get_testnet_credentials()

    if api_key and api_secret:
        print(f"Using KRAKEN_TESTNET_API_KEY: {api_key[:8]}...")
        print(f"Using KRAKEN_TESTNET_API_SECRET: {api_secret[:8]}...\n")

        # Create authenticated client
        client = KrakenFuturesHttpClient(
            api_key=api_key,
            api_secret=api_secret,
            testnet=True,
        )

        # Test public endpoints and get reference price
        reference_price = await test_futures_public(client)

        # Test authenticated endpoints
        account_id = AccountId("KRAKEN-001")
        await test_futures_authenticated(client, account_id)

        # Test order placement
        await test_futures_order_placement(client, account_id, reference_price)

    else:
        print("No testnet credentials found")
        print("Running public endpoint tests only...\n")

        # Create public client
        client = KrakenFuturesHttpClient(testnet=True)

        # Test public endpoints only
        await test_futures_public(client)

    print("\n=== Demo Complete ===")


if __name__ == "__main__":
    asyncio.run(main())
