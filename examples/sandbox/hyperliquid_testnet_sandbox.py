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
Sandbox script for testing Hyperliquid testnet order placement.

Usage:
    export HYPERLIQUID_TESTNET_PK=0x...
    python examples/sandbox/hyperliquid_testnet_sandbox.py

"""

import asyncio
import logging
import os
import sys
import uuid

from nautilus_trader.core.nautilus_pyo3 import UUID4
from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
from nautilus_trader.core.nautilus_pyo3 import HyperliquidHttpClient  # type: ignore
from nautilus_trader.core.nautilus_pyo3 import LimitOrder
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import StrategyId
from nautilus_trader.core.nautilus_pyo3 import TimeInForce
from nautilus_trader.core.nautilus_pyo3 import TraderId


async def main():
    """
    Test Hyperliquid testnet order placement.
    """
    logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")

    private_key = os.getenv("HYPERLIQUID_TESTNET_PK")
    if not private_key:
        logging.error("HYPERLIQUID_TESTNET_PK not set")
        sys.exit(1)

    logging.info(f"Private key from environment: {private_key[:10]}...{private_key[-8:]}")

    try:
        logging.info("Creating HTTP client with credentials...")
        http_client = HyperliquidHttpClient(
            private_key=private_key,
            vault_address=None,
            is_testnet=True,
            timeout_secs=None,
        )
        logging.info("✓ Client created")

        # Verify the derived address
        address = http_client.get_user_address()
        logging.info(f"Derived wallet address: {address}")
        logging.info("Expected address: 0x09bab0ad3c86DE1ad847a57c0C0A7A0F3f44be8a")

        logging.info("Loading instruments...")
        instruments = await http_client.load_instrument_definitions(
            include_perp=True,
            include_spot=False,
        )
        instrument = next((i for i in instruments if "BTC-USD-PERP" in str(i.id.symbol)), None)
        if not instrument:
            raise ValueError("BTC-USD-PERP not found")

        http_client.add_instrument(instrument)
        http_client.set_account_id(f"HYPERLIQUID-{address}")
        logging.info(f"✓ Instrument: {instrument.id}")

        # Get current BTC price
        logging.info("Fetching BTC order book...")
        import json

        book_json = await http_client.get_l2_book("BTC")
        book = json.loads(book_json)
        best_bid = float(book["levels"][0][0]["px"])

        # Place order at best bid minus $100
        limit_price = int(best_bid - 100)  # $100 below best bid for safety

        # Use 0.001 BTC (same as Rust binary)
        quantity = 0.001

        logging.info(f"Best bid: ${best_bid}, Order price: ${limit_price}")
        logging.info(f"Quantity: {quantity} BTC (~${quantity * limit_price:.2f} notional)")

        cloid_hex = "0x" + uuid.UUID(UUID4().value).hex
        order = LimitOrder(
            trader_id=TraderId("TESTER-001"),
            strategy_id=StrategyId("SANDBOX-001"),
            instrument_id=instrument.id,
            client_order_id=ClientOrderId(cloid_hex),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("0.00100"),  # Exactly 5 decimals
            price=Price.from_str(str(limit_price)),
            time_in_force=TimeInForce.GTC,
            post_only=False,  # Try without post_only
            reduce_only=False,
            quote_quantity=False,
            init_id=UUID4(),
            ts_init=0,
        )

        logging.info("Submitting order...")
        report = await http_client.submit_order(order)
        logging.info("=" * 60)
        logging.info("✓ ORDER SUBMITTED SUCCESSFULLY!")
        logging.info(f"  Client Order ID: {report.client_order_id}")
        logging.info(f"  Venue Order ID:  {report.venue_order_id}")
        logging.info(f"  Order Status:    {report.order_status}")
        logging.info(f"  Filled Qty:      {report.filled_qty}")
        logging.info("=" * 60)
    except Exception as e:
        logging.error(f"Error: {e}", exc_info=True)
        sys.exit(1)


if __name__ == "__main__":
    asyncio.run(main())
