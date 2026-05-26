#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter — Sandbox smoke test (HTTP)
# -------------------------------------------------------------------------------------------------

"""
Manual smoke test for LMEX REST endpoints via the real adapter stack.

Usage
-----
    export LMEX_API_KEY=<your_sandbox_key>
    export LMEX_API_SECRET=<your_sandbox_secret>
    python tests/sandbox/sandbox_http.py

The script exercises every HTTP method on the sandbox environment
(``https://test-api.lmex.io/spot``) and prints results.

**BTC-EUR is used as the test pair** (USD balance is zero in the sandbox;
EUR balance is sufficient for small limit orders).

Set ``PLACE_ORDERS=1`` to enable order submission and cancellation tests.
"""

from __future__ import annotations

import asyncio
import os

from nautilus_trader.adapters.lmex.constants import LMEX_BASE_URL_SANDBOX
from nautilus_trader.adapters.lmex.http.account import LmexAccountHttpAPI
from nautilus_trader.adapters.lmex.http.client import LmexHttpClient
from nautilus_trader.adapters.lmex.http.market import LmexMarketHttpAPI
from nautilus_trader.common.component import LiveClock

API_KEY = os.environ.get("LMEX_API_KEY", "")
API_SECRET = os.environ.get("LMEX_API_SECRET", "")
PLACE_ORDERS = os.environ.get("PLACE_ORDERS", "0") == "1"

# Sandbox test pair — EUR balance is available in test account
TEST_SYMBOL = "BTC-EUR"


async def run() -> None:
    clock = LiveClock()
    http = LmexHttpClient(
        clock=clock,
        api_key=API_KEY,
        api_secret=API_SECRET,
        base_url=LMEX_BASE_URL_SANDBOX,
    )
    market = LmexMarketHttpAPI(http)
    account = LmexAccountHttpAPI(http)

    # ------------------------------------------------------------------
    # Public endpoints
    # ------------------------------------------------------------------
    print("\n=== server time ===")
    t = await market.get_server_time()
    print(f"  iso={t.iso!r}  epoch={t.epoch}")

    print(f"\n=== market_summary ({TEST_SYMBOL}) ===")
    summary = await market.get_market_summary(symbol=TEST_SYMBOL)
    s = summary[0]
    print(f"  symbol={s.symbol}  last={s.last}  active={s.active}")
    print(f"  minPriceIncrement={s.minPriceIncrement}  minSizeIncrement={s.minSizeIncrement}")

    print(f"\n=== orderbook ({TEST_SYMBOL}, depth=5) ===")
    ob = await market.get_orderbook(TEST_SYMBOL, depth=5)
    print(f"  bids[0]: price={ob.buyQuote[0].price}  size={ob.buyQuote[0].size}")
    print(f"  asks[0]: price={ob.sellQuote[0].price}  size={ob.sellQuote[0].size}")

    print(f"\n=== recent trades ({TEST_SYMBOL}, count=3) ===")
    trades = await market.get_trades(TEST_SYMBOL, count=3)
    for tr in trades:
        print(f"  {tr.side:4s}  price={tr.price:.2f}  size={tr.size}  ts={tr.timestamp}")

    if not API_KEY:
        print("\n[SKIP] No API key — skipping authenticated endpoints")
        return

    # ------------------------------------------------------------------
    # Authenticated endpoints
    # ------------------------------------------------------------------
    print("\n=== wallet (non-zero balances) ===")
    wallet = await account.get_wallet_balance()
    non_zero = [e for e in wallet if e.total > 0]
    for e in non_zero:
        print(f"  {e.currency:8s}  total={e.total}  available={e.available}")

    print(f"\n=== open orders ({TEST_SYMBOL}) ===")
    open_orders = await account.get_open_orders(symbol=TEST_SYMBOL)
    print(f"  count={len(open_orders)}")
    for o in open_orders:
        print(f"  {o.orderID}  side={o.side}  price={o.price}  size={o.size}")

    print(f"\n=== fill history ({TEST_SYMBOL}, count=3) ===")
    fills = await account.get_fills(symbol=TEST_SYMBOL, count=3)
    print(f"  count={len(fills)}")
    for f in fills:
        print(f"  {f.tradeId[:8]}…  side={f.side}  filledPrice={f.filledPrice:.2f}  filledSize={f.filledSize}")

    # ------------------------------------------------------------------
    # Order lifecycle (optional)
    # ------------------------------------------------------------------
    if not PLACE_ORDERS:
        print("\n[SKIP] Set PLACE_ORDERS=1 to run order submit/cancel tests")
        return

    print(f"\n=== submit LIMIT BUY {TEST_SYMBOL} at 1.0 EUR (deep resting) ===")
    resp = await account.submit_order(
        symbol=TEST_SYMBOL,
        side="BUY",
        order_type="LIMIT",
        size=0.00005,
        price=1.0,
        client_order_id="nt-sandbox-test-001",
    )
    order_id = resp.orderID
    print(f"  orderID={order_id}  status={resp.status}  clOrderID={resp.clOrderID}")
    assert resp.status == 2, f"Expected status 2 (INSERTED), got {resp.status}"

    print(f"\n=== cancel order {order_id} ===")
    cancel = await account.cancel_order(symbol=TEST_SYMBOL, order_id=order_id)
    print(f"  orderID={cancel.orderID}  status={cancel.status}")
    assert cancel.status == 6, f"Expected status 6 (CANCELLED), got {cancel.status}"

    print(f"\n=== open orders after cancel (expect 0) ===")
    remaining = await account.get_open_orders(symbol=TEST_SYMBOL)
    print(f"  count={len(remaining)}")

    print("\n✓ All sandbox HTTP tests passed")


if __name__ == "__main__":
    asyncio.run(run())
