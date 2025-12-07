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
Kraken Spot REST API order type testing.

Tests all supported order types on Kraken Spot mainnet via REST API.

Order Types Tested:
    1. Market order (buy)
    2. Market order (sell)
    3. Limit order (buy, post-only)
    4. Limit order (sell)
    5. Stop-Loss order (StopMarket)
    6. Stop-Loss-Limit order (StopLimit)
    7. Take-Profit order (MarketIfTouched)
    8. Take-Profit-Limit order (LimitIfTouched)

Environment Variables:
    KRAKEN_SPOT_API_KEY: Your Kraken Spot API key
    KRAKEN_SPOT_API_SECRET: Your Kraken Spot API secret

"""

import asyncio
import os
import uuid

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
DEFAULT_PRICE = 5.0

# Price multipliers
LIMIT_BUY_MULTIPLIER = 0.95    # 5% below market
LIMIT_SELL_MULTIPLIER = 1.05   # 5% above market
STOP_LOSS_MULTIPLIER = 0.90    # 10% below market
TAKE_PROFIT_MULTIPLIER = 1.10  # 10% above market
SECONDARY_PRICE_MULTIPLIER = 0.99  # 1% below trigger for limit orders

# Timing
SHORT_WAIT = 2  # seconds (increased for order propagation)
LONG_WAIT = 3   # seconds


def generate_order_id(prefix: str) -> ClientOrderId:
    """
    Generate a unique client order ID in UUID format (required by Kraken).
    """
    # Kraken requires cl_ord_id to be in UUID format
    return ClientOrderId(str(uuid.uuid4()))


async def submit_market_order(
    client: KrakenSpotHttpClient,
    account_id: AccountId,
    instrument_id: InstrumentId,
    side: OrderSide,
    prefix: str,
) -> VenueOrderId | None:
    """
    Submit a market order and print result.
    """
    client_order_id = generate_order_id(prefix)
    try:
        venue_order_id = await client.submit_order(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=side,
            order_type=OrderType.MARKET,
            quantity=Quantity.from_str(QTY),
            time_in_force=TimeInForce.GTC,  # Market orders don't support IOC flag on Kraken
        )
        print(f"    SUCCESS: {venue_order_id}")
        return venue_order_id
    except Exception as e:
        print(f"    FAILED: {e}")
        return None


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
    client_order_id = generate_order_id(prefix)
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


async def submit_stop_market_order(
    client: KrakenSpotHttpClient,
    account_id: AccountId,
    instrument_id: InstrumentId,
    side: OrderSide,
    trigger_price: float,
    prefix: str,
) -> VenueOrderId | None:
    """
    Submit a stop-loss (stop-market) order.
    """
    client_order_id = generate_order_id(prefix)
    try:
        venue_order_id = await client.submit_order(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=side,
            order_type=OrderType.STOP_MARKET,
            quantity=Quantity.from_str(QTY),
            time_in_force=TimeInForce.GTC,
            trigger_price=Price.from_str(f"{trigger_price:.4f}"),
        )
        print(f"    SUCCESS: {venue_order_id}")
        return venue_order_id
    except Exception as e:
        print(f"    FAILED: {e}")
        return None


async def submit_stop_limit_order(
    client: KrakenSpotHttpClient,
    account_id: AccountId,
    instrument_id: InstrumentId,
    side: OrderSide,
    trigger_price: float,
    limit_price: float,
    prefix: str,
) -> VenueOrderId | None:
    """
    Submit a stop-loss-limit order.
    """
    client_order_id = generate_order_id(prefix)
    try:
        venue_order_id = await client.submit_order(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=side,
            order_type=OrderType.STOP_LIMIT,
            quantity=Quantity.from_str(QTY),
            time_in_force=TimeInForce.GTC,
            trigger_price=Price.from_str(f"{trigger_price:.4f}"),
            price=Price.from_str(f"{limit_price:.4f}"),
        )
        print(f"    SUCCESS: {venue_order_id}")
        return venue_order_id
    except Exception as e:
        print(f"    FAILED: {e}")
        return None


async def submit_take_profit_order(
    client: KrakenSpotHttpClient,
    account_id: AccountId,
    instrument_id: InstrumentId,
    side: OrderSide,
    trigger_price: float,
    prefix: str,
) -> VenueOrderId | None:
    """
    Submit a take-profit (market-if-touched) order.
    """
    client_order_id = generate_order_id(prefix)
    try:
        venue_order_id = await client.submit_order(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=side,
            order_type=OrderType.MARKET_IF_TOUCHED,
            quantity=Quantity.from_str(QTY),
            time_in_force=TimeInForce.GTC,
            trigger_price=Price.from_str(f"{trigger_price:.4f}"),
        )
        print(f"    SUCCESS: {venue_order_id}")
        return venue_order_id
    except Exception as e:
        print(f"    FAILED: {e}")
        return None


async def submit_take_profit_limit_order(
    client: KrakenSpotHttpClient,
    account_id: AccountId,
    instrument_id: InstrumentId,
    side: OrderSide,
    trigger_price: float,
    limit_price: float,
    prefix: str,
) -> VenueOrderId | None:
    """
    Submit a take-profit-limit (limit-if-touched) order.
    """
    client_order_id = generate_order_id(prefix)
    try:
        venue_order_id = await client.submit_order(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=side,
            order_type=OrderType.LIMIT_IF_TOUCHED,
            quantity=Quantity.from_str(QTY),
            time_in_force=TimeInForce.GTC,
            trigger_price=Price.from_str(f"{trigger_price:.4f}"),
            price=Price.from_str(f"{limit_price:.4f}"),
        )
        print(f"    SUCCESS: {venue_order_id}")
        return venue_order_id
    except Exception as e:
        print(f"    FAILED: {e}")
        return None


async def run_order_tests(client: KrakenSpotHttpClient, account_id: AccountId) -> None:
    """
    Test order placement with all supported order types via REST API.
    """
    instrument_id = InstrumentId.from_str(INSTRUMENT_ID_STR)

    print("\n[SETUP] Loading instruments...")
    instruments = await client.request_instruments()
    for inst in instruments:
        client.cache_instrument(inst)
    print(f"    Loaded {len(instruments)} instruments")

    reference_price = DEFAULT_PRICE
    print(f"    Reference price: ${reference_price:.4f}")

    # Calculate prices for different order types
    limit_buy_price = reference_price * LIMIT_BUY_MULTIPLIER
    limit_sell_price = reference_price * LIMIT_SELL_MULTIPLIER
    stop_loss_trigger = reference_price * STOP_LOSS_MULTIPLIER
    stop_limit_price = stop_loss_trigger * SECONDARY_PRICE_MULTIPLIER
    take_profit_trigger = reference_price * TAKE_PROFIT_MULTIPLIER
    take_profit_limit_price = take_profit_trigger * SECONDARY_PRICE_MULTIPLIER

    print("\n[SETUP] Canceling all open orders...")
    cancelled = await client.cancel_all_orders()
    print(f"    Cancelled: {cancelled}")
    await asyncio.sleep(SHORT_WAIT)

    # =========================================================================
    # TEST 1: Market Order (BUY)
    # =========================================================================
    print(f"\n[TEST 1] MARKET BUY Order\n    Symbol: {SYMBOL}, Qty: {QTY}")
    await submit_market_order(client, account_id, instrument_id, OrderSide.BUY, "market-buy")
    await asyncio.sleep(LONG_WAIT)

    # =========================================================================
    # TEST 2: Market Order (SELL)
    # =========================================================================
    print(f"\n[TEST 2] MARKET SELL Order\n    Symbol: {SYMBOL}, Qty: {QTY}")
    await submit_market_order(client, account_id, instrument_id, OrderSide.SELL, "market-sell")
    await asyncio.sleep(LONG_WAIT)

    # =========================================================================
    # TEST 3: Limit Order (BUY, post-only)
    # =========================================================================
    print("\n[TEST 3] LIMIT BUY Order (post-only)")
    print(f"    Symbol: {SYMBOL}, Qty: {QTY}, Price: ${limit_buy_price:.4f}")
    await submit_limit_order(
        client, account_id, instrument_id, OrderSide.BUY, limit_buy_price, "limit-buy", post_only=True,
    )
    await asyncio.sleep(SHORT_WAIT)

    # =========================================================================
    # TEST 4: Limit Order (SELL)
    # =========================================================================
    print("\n[TEST 4] LIMIT SELL Order")
    print("    Buying position first...")
    await submit_market_order(client, account_id, instrument_id, OrderSide.BUY, "pre-buy-4")
    await asyncio.sleep(LONG_WAIT)
    print(f"    Symbol: {SYMBOL}, Qty: {QTY}, Price: ${limit_sell_price:.4f}")
    await submit_limit_order(
        client, account_id, instrument_id, OrderSide.SELL, limit_sell_price, "limit-sell",
    )
    await asyncio.sleep(SHORT_WAIT)

    # =========================================================================
    # TEST 5: Stop-Loss Order (StopMarket)
    # =========================================================================
    print("\n[TEST 5] STOP-LOSS Order (StopMarket)")
    print("    Buying position first...")
    await submit_market_order(client, account_id, instrument_id, OrderSide.BUY, "pre-buy-5")
    await asyncio.sleep(LONG_WAIT)
    print(f"    Symbol: {SYMBOL}, Qty: {QTY}, Trigger: ${stop_loss_trigger:.4f}")
    await submit_stop_market_order(
        client, account_id, instrument_id, OrderSide.SELL, stop_loss_trigger, "stop-loss",
    )
    await asyncio.sleep(SHORT_WAIT)

    # =========================================================================
    # TEST 6: Stop-Loss-Limit Order (StopLimit)
    # =========================================================================
    print("\n[TEST 6] STOP-LOSS-LIMIT Order (StopLimit)")
    print("    Buying position first...")
    await submit_market_order(client, account_id, instrument_id, OrderSide.BUY, "pre-buy-6")
    await asyncio.sleep(LONG_WAIT)
    print(f"    Symbol: {SYMBOL}, Qty: {QTY}")
    print(f"    Trigger: ${stop_loss_trigger:.4f}, Limit: ${stop_limit_price:.4f}")
    await submit_stop_limit_order(
        client, account_id, instrument_id, OrderSide.SELL, stop_loss_trigger, stop_limit_price, "stop-limit",
    )
    await asyncio.sleep(SHORT_WAIT)

    # =========================================================================
    # TEST 7: Take-Profit Order (MarketIfTouched)
    # =========================================================================
    print("\n[TEST 7] TAKE-PROFIT Order (MarketIfTouched)")
    print("    Buying position first...")
    await submit_market_order(client, account_id, instrument_id, OrderSide.BUY, "pre-buy-7")
    await asyncio.sleep(LONG_WAIT)
    print(f"    Symbol: {SYMBOL}, Qty: {QTY}, Trigger: ${take_profit_trigger:.4f}")
    await submit_take_profit_order(
        client, account_id, instrument_id, OrderSide.SELL, take_profit_trigger, "take-profit",
    )
    await asyncio.sleep(SHORT_WAIT)

    # =========================================================================
    # TEST 8: Take-Profit-Limit Order (LimitIfTouched)
    # =========================================================================
    print("\n[TEST 8] TAKE-PROFIT-LIMIT Order (LimitIfTouched)")
    print("    Buying position first...")
    await submit_market_order(client, account_id, instrument_id, OrderSide.BUY, "pre-buy-8")
    await asyncio.sleep(LONG_WAIT)
    print(f"    Symbol: {SYMBOL}, Qty: {QTY}")
    print(f"    Trigger: ${take_profit_trigger:.4f}, Limit: ${take_profit_limit_price:.4f}")
    await submit_take_profit_limit_order(
        client, account_id, instrument_id, OrderSide.SELL, take_profit_trigger, take_profit_limit_price, "tp-limit",
    )
    await asyncio.sleep(SHORT_WAIT)

    # Final cleanup
    print("\n[CLEANUP] Canceling any remaining orders...")
    cancelled = await client.cancel_all_orders()
    print(f"    Cancelled: {cancelled}")

    print("\n" + "=" * 60)
    print("REST API Order Type Testing Complete!")
    print("=" * 60)
    print("Tested order types:")
    print("  1. Market (Buy/Sell)")
    print("  2. Limit (Buy with post-only, Sell)")
    print("  3. Stop-Loss / StopMarket")
    print("  4. Stop-Loss-Limit / StopLimit")
    print("  5. Take-Profit / MarketIfTouched")
    print("  6. Take-Profit-Limit / LimitIfTouched")
    print("=" * 60)


async def main() -> None:
    api_key = os.environ.get("KRAKEN_SPOT_API_KEY")
    api_secret = os.environ.get("KRAKEN_SPOT_API_SECRET")

    if not api_key or not api_secret:
        print("Error: Set KRAKEN_SPOT_API_KEY and KRAKEN_SPOT_API_SECRET environment variables")
        print()
        print("Usage:")
        print('  export KRAKEN_SPOT_API_KEY="<YOUR_KRAKEN_SPOT_API_KEY>"')
        print('  export KRAKEN_SPOT_API_SECRET="<YOUR_KRAKEN_SPOT_API_SECRET>"')
        print("  python kraken_spot_rest.py")
        return

    print("Kraken Spot REST API - All Order Types Testing")
    print("=" * 60)

    client = KrakenSpotHttpClient(
        api_key=api_key,
        api_secret=api_secret,
    )

    account_id = AccountId(ACCOUNT_ID_STR)
    await run_order_tests(client, account_id)


if __name__ == "__main__":
    asyncio.run(main())
