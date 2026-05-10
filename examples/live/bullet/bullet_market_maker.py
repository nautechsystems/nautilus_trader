#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
Simple market maker for Bullet.xyz perpetuals.

Polls the L2 depth for a mid-price reference, then maintains a bid and ask
limit order on each side of the mid. Orders are refreshed whenever the mid
moves by more than half the spread. One-sided quoting kicks in when the net
position approaches the max limit.

Position is tracked in real-time via the authenticated WS order-update feed.

Usage:
    BULLET_KEY_FILE=~/.config/bullet/id.json python bullet_market_maker.py

Environment variables:
    BULLET_KEY_FILE        Path to Solana-compatible JSON keystore (required)
    BULLET_PRIVATE_KEY     Ed25519 hex private key (alternative to key file)
    BULLET_ACCOUNT_ADDRESS Main account address (only if using a delegate key)
    BULLET_BASE_URL        Override HTTP base URL (default: testnet)
    BULLET_SYMBOL          Market symbol (default: SOL-USD)
    MM_SPREAD_BPS          Half-spread in basis points (default: 10, i.e. 20bps total)
    MM_QTY                 Order size per side (default: 0.5)
    MM_MAX_POSITION        Max net position before skewing (default: 5.0)
    MM_POLL_SECS           Depth polling interval (default: 2.0)
    MM_REFRESH_THRESHOLD   Fraction of half-spread that triggers a refresh (default: 0.5)
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import sys
import time
from dataclasses import dataclass, field
from decimal import Decimal, ROUND_DOWN, ROUND_UP

# Add repo root to path so we can import nautilus_pyo3 without installing
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "../../.."))

from nautilus_trader.core.nautilus_pyo3 import BulletHttpClient, BulletOrderClient, BulletWebSocketClient


logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)-7s %(message)s",
    datefmt="%H:%M:%S",
)
log = logging.getLogger("mm")


# ── Config ─────────────────────────────────────────────────────────────────────

@dataclass
class MMConfig:
    symbol: str = os.environ.get("BULLET_SYMBOL", "SOL-USD")
    base_url: str = os.environ.get("BULLET_BASE_URL", "https://tradingapi.testnet.bullet.xyz")
    half_spread_bps: Decimal = Decimal(os.environ.get("MM_SPREAD_BPS", "10"))
    qty: Decimal = Decimal(os.environ.get("MM_QTY", "0.5"))
    max_position: Decimal = Decimal(os.environ.get("MM_MAX_POSITION", "5.0"))
    poll_secs: float = float(os.environ.get("MM_POLL_SECS", "2.0"))
    refresh_threshold: Decimal = Decimal(os.environ.get("MM_REFRESH_THRESHOLD", "0.5"))
    # Precision (populated from exchangeInfo)
    tick_size: Decimal = Decimal("0.01")
    step_size: Decimal = Decimal("0.01")


# ── State ──────────────────────────────────────────────────────────────────────

@dataclass
class MMState:
    bid_order_id: int | None = None    # venue order ID
    ask_order_id: int | None = None
    bid_price: Decimal | None = None   # price we're currently quoting
    ask_price: Decimal | None = None
    position: Decimal = Decimal("0")   # net position tracked from WS order updates
    last_mid: Decimal | None = None
    errors: int = 0


# ── Helpers ────────────────────────────────────────────────────────────────────

def snap_down(value: Decimal, tick: Decimal) -> Decimal:
    return (value / tick).to_integral_value(rounding=ROUND_DOWN) * tick


def snap_up(value: Decimal, tick: Decimal) -> Decimal:
    return (value / tick).to_integral_value(rounding=ROUND_UP) * tick


def snap_qty(value: Decimal, step: Decimal) -> Decimal:
    return (value / step).to_integral_value(rounding=ROUND_DOWN) * step


def target_prices(mid: Decimal, half_spread: Decimal, cfg: MMConfig) -> tuple[Decimal, Decimal]:
    raw_bid = mid - half_spread
    raw_ask = mid + half_spread
    bid = snap_down(raw_bid, cfg.tick_size)
    ask = snap_up(raw_ask, cfg.tick_size)
    return bid, ask


def needs_refresh(
    state: MMState,
    new_bid: Decimal,
    new_ask: Decimal,
    half_spread: Decimal,
    threshold: Decimal,
) -> bool:
    if state.bid_price is None or state.ask_price is None:
        return True
    bid_drift = abs(new_bid - state.bid_price)
    ask_drift = abs(new_ask - state.ask_price)
    return bid_drift > half_spread * threshold or ask_drift > half_spread * threshold


# ── Market maker logic ─────────────────────────────────────────────────────────

class MarketMaker:
    def __init__(
        self,
        cfg: MMConfig,
        http: BulletHttpClient,
        order_client: BulletOrderClient,
        ws_client: BulletWebSocketClient,
    ):
        self.cfg = cfg
        self.http = http
        self.order = order_client
        self.ws_client = ws_client
        self.state = MMState()
        # nanosecond base avoids cross-restart cloid collisions
        self._cloid_counter = time.time_ns()
        # tracks cumulative executedQty per order_id for delta-based position tracking
        self._order_exec_qty: dict[int, Decimal] = {}

    def _next_cloid(self) -> int:
        self._cloid_counter += 1
        return self._cloid_counter

    # ── WS order-update feed ──────────────────────────────────────────────────

    async def _start_order_feed(self) -> None:
        address = self.order.account_address
        if not address:
            log.warning("No account address — order-update feed disabled, position will not be tracked")
            return
        loop = asyncio.get_event_loop()
        await self.ws_client.connect(loop, [], self._on_ws_msg)
        await self.ws_client.wait_until_active(10.0)
        await self.ws_client.subscribe_order_updates(address)
        log.info(f"Subscribed to order updates for {address}")

    def _on_ws_msg(self, json_str: str) -> None:
        try:
            msg = json.loads(json_str)
        except Exception:
            return
        if msg.get("e") == "ORDER_TRADE_UPDATE":
            self._handle_order_update(msg)

    def _handle_order_update(self, msg: dict) -> None:
        order_id = msg.get("orderId")
        status = msg.get("status", "")
        side = msg.get("side", "")
        new_exec_qty = Decimal(str(msg.get("executedQty", "0")))

        # Delta vs cumulative: executedQty is cumulative, so take the increment.
        prev = self._order_exec_qty.get(order_id, Decimal("0"))
        delta = new_exec_qty - prev

        if delta > 0:
            if side == "BUY":
                self.state.position += delta
            elif side == "SELL":
                self.state.position -= delta
            log.info(f"Fill: {side} {delta} @ id={order_id} → pos={self.state.position}")

        if status in ("FILLED", "CANCELED", "EXPIRED"):
            self._order_exec_qty.pop(order_id, None)
        else:
            self._order_exec_qty[order_id] = new_exec_qty

    # ── REST helpers ──────────────────────────────────────────────────────────

    async def fetch_mid(self) -> Decimal | None:
        """Fetch best bid + ask from depth and return the mid price."""
        try:
            result = await self.http.best_bid_ask(self.cfg.symbol)
            bid_str, ask_str = result
            if bid_str is None or ask_str is None:
                return None
            return (Decimal(bid_str) + Decimal(ask_str)) / 2
        except Exception as e:
            log.warning(f"best_bid_ask failed: {e}")
            return None

    async def cancel_bid(self) -> None:
        if self.state.bid_order_id is None:
            return
        try:
            await self.order.cancel_order(
                symbol=self.cfg.symbol,
                venue_order_id=self.state.bid_order_id,
                client_order_id=None,
            )
            log.debug(f"Canceled bid {self.state.bid_order_id}")
        except Exception as e:
            log.warning(f"cancel bid failed: {e}")
        self.state.bid_order_id = None
        self.state.bid_price = None

    async def cancel_ask(self) -> None:
        if self.state.ask_order_id is None:
            return
        try:
            await self.order.cancel_order(
                symbol=self.cfg.symbol,
                venue_order_id=self.state.ask_order_id,
                client_order_id=None,
            )
            log.debug(f"Canceled ask {self.state.ask_order_id}")
        except Exception as e:
            log.warning(f"cancel ask failed: {e}")
        self.state.ask_order_id = None
        self.state.ask_price = None

    async def cancel_all(self) -> None:
        try:
            await self.order.cancel_market_orders(symbol=self.cfg.symbol)
            log.info("Canceled all market orders")
        except Exception as e:
            log.warning(f"cancel_market_orders failed: {e}")
        self.state.bid_order_id = None
        self.state.ask_order_id = None
        self.state.bid_price = None
        self.state.ask_price = None

    async def place_bid(self, price: Decimal, qty: Decimal) -> None:
        cloid = self._next_cloid()
        try:
            await self.order.place_order(
                symbol=self.cfg.symbol,
                is_buy=True,
                price=str(price),
                qty=str(qty),
                is_limit=True,
                client_order_id=cloid,
                reduce_only=False,
            )
            self.state.bid_price = price
            log.info(f"  BID {qty} @ {price}  (cloid={cloid})")
        except Exception as e:
            log.warning(f"place bid failed: {e}")

    async def place_ask(self, price: Decimal, qty: Decimal) -> None:
        cloid = self._next_cloid()
        try:
            await self.order.place_order(
                symbol=self.cfg.symbol,
                is_buy=False,
                price=str(price),
                qty=str(qty),
                is_limit=True,
                client_order_id=cloid,
                reduce_only=False,
            )
            self.state.ask_price = price
            log.info(f"  ASK {qty} @ {price}  (cloid={cloid})")
        except Exception as e:
            log.warning(f"place ask failed: {e}")

    async def sync_open_orders(self) -> None:
        """Pull open orders from REST to keep our venue_order_id tracking correct."""
        address = self.order.account_address
        if not address:
            return
        try:
            raw_json = await self.http.open_orders_json(address, self.cfg.symbol)
            orders = json.loads(raw_json)
            bids = [o for o in orders if o.get("side") == "BUY"]
            asks = [o for o in orders if o.get("side") == "SELL"]
            self.state.bid_order_id = bids[0]["orderId"] if bids else None
            self.state.ask_order_id = asks[0]["orderId"] if asks else None
        except Exception as e:
            log.debug(f"sync_open_orders failed (non-fatal): {e}")

    # ── Main loop ─────────────────────────────────────────────────────────────

    async def run_once(self) -> None:
        cfg = self.cfg
        state = self.state

        mid = await self.fetch_mid()
        if mid is None:
            log.warning("No mid price available — skipping cycle")
            return

        half_spread = mid * cfg.half_spread_bps / Decimal("10000")
        bid_px, ask_px = target_prices(mid, half_spread, cfg)
        qty = snap_qty(cfg.qty, cfg.step_size)

        log.info(
            f"{cfg.symbol} mid={mid:.4f}  spread={half_spread * 2:.4f}  "
            f"bid={bid_px}  ask={ask_px}  pos={state.position}"
        )

        if not needs_refresh(state, bid_px, ask_px, half_spread, cfg.refresh_threshold):
            log.debug("Prices within threshold — no refresh needed")
            return

        # Determine which sides to quote based on position
        quote_bid = state.position < cfg.max_position
        quote_ask = state.position > -cfg.max_position

        # Cancel existing orders before re-quoting
        if state.bid_order_id is not None:
            await self.cancel_bid()
        if state.ask_order_id is not None:
            await self.cancel_ask()

        # Small delay to let cancels settle
        await asyncio.sleep(0.3)

        # Re-quote
        if quote_bid:
            await self.place_bid(bid_px, qty)
        else:
            log.info("Position limit reached on long side — skipping bid")

        if quote_ask:
            await self.place_ask(ask_px, qty)
        else:
            log.info("Position limit reached on short side — skipping ask")

        # Sync venue order IDs
        await asyncio.sleep(0.5)
        await self.sync_open_orders()

        state.last_mid = mid

    async def run(self) -> None:
        log.info(
            f"Starting market maker: {self.cfg.symbol}  "
            f"spread={self.cfg.half_spread_bps * 2}bps  "
            f"qty={self.cfg.qty}  max_pos={self.cfg.max_position}"
        )
        try:
            await self._start_order_feed()
            while True:
                try:
                    await self.run_once()
                except Exception as e:
                    self.state.errors += 1
                    log.error(f"run_once error (#{self.state.errors}): {e}")
                    if self.state.errors > 10:
                        log.error("Too many errors — canceling all orders and stopping")
                        await self.cancel_all()
                        raise

                await asyncio.sleep(self.cfg.poll_secs)
        except (KeyboardInterrupt, asyncio.CancelledError):
            log.info("Shutting down — canceling all open orders...")
            await self.cancel_all()
            await self.ws_client.close()
            log.info("Done.")


# ── Entrypoint ─────────────────────────────────────────────────────────────────

async def main() -> None:
    cfg = MMConfig()

    http = BulletHttpClient(base_url=cfg.base_url, timeout_secs=10)

    # Load precision from exchangeInfo via our HTTP client
    info = json.loads(await http.exchange_info_json())
    sym = next((s for s in info["symbols"] if s["symbol"] == cfg.symbol), None)
    if sym is None:
        raise SystemExit(f"Symbol {cfg.symbol} not found in exchangeInfo")
    for f in sym.get("filters", []):
        if f.get("filterType") == "PRICE_FILTER":
            cfg.tick_size = Decimal(f["tickSize"])
        elif f.get("filterType") == "LOT_SIZE":
            cfg.step_size = Decimal(f["stepSize"])
    log.info(f"Loaded precision: tick={cfg.tick_size}  step={cfg.step_size}")

    order_client = BulletOrderClient(
        base_url=cfg.base_url,
        timeout_secs=10,
        key_file=os.environ.get("BULLET_KEY_FILE"),
        private_key=os.environ.get("BULLET_PRIVATE_KEY"),
        account_address=os.environ.get("BULLET_ACCOUNT_ADDRESS"),
    )
    log.info("Connecting order client (loading chain data)...")
    await order_client.connect()
    log.info(f"Connected. Account: {order_client.account_address}")

    ws_url = cfg.base_url.replace("https://", "wss://").replace("http://", "ws://") + "/ws"
    ws_client = BulletWebSocketClient(url=ws_url)

    mm = MarketMaker(cfg=cfg, http=http, order_client=order_client, ws_client=ws_client)
    await mm.run()


if __name__ == "__main__":
    asyncio.run(main())
