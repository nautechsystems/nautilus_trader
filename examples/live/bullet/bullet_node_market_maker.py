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
Market maker for Bullet.xyz perpetuals using the full NautilusTrader engine.

Runs inside a TradingNode, giving access to:
  - Portfolio: real-time P&L, unrealized PnL, net position
  - Cache: order state, fill history, account balances
  - RiskEngine: order/fill reconciliation at startup
  - LiveClock: nanosecond timestamps, scheduled callbacks
  - LiveLogger: structured logging with color and context

Strategy logic: maintain a bid/ask quote around the mid price, refresh when
the mid moves by more than half the spread, and skew or disable one side when
the net position approaches the max limit.

Usage:
    BULLET_PRIVATE_KEY=<base58key> python bullet_node_market_maker.py
    BULLET_KEY_FILE=~/.config/bullet/id.json python bullet_node_market_maker.py

Environment variables:
    BULLET_PRIVATE_KEY     Base58 or hex ed25519 private key (64-byte Solana format)
    BULLET_KEY_FILE        Path to Solana-compatible JSON keystore (alternative)
    BULLET_ACCOUNT_ADDRESS Main account address if using a delegate key
    BULLET_BASE_URL        Override HTTP base URL (default: testnet)
    BULLET_SYMBOL          Market symbol (default: SOL-USD-PERP.BULLET)
    MM_SPREAD_BPS          Half-spread in basis points (default: 10)
    MM_QTY                 Order size per side (default: 0.5)
    MM_MAX_POSITION        Max net position before skewing (default: 5.0)
    MM_REFRESH_THRESHOLD   Fraction of half-spread that triggers a refresh (default: 0.5)
"""

from __future__ import annotations

import os
from decimal import Decimal, ROUND_DOWN, ROUND_UP

from nautilus_trader.adapters.bullet.config import BulletDataClientConfig
from nautilus_trader.adapters.bullet.config import BulletExecClientConfig
from nautilus_trader.adapters.bullet.constants import BULLET_VENUE
from nautilus_trader.adapters.bullet.factories import BulletLiveDataClientFactory
from nautilus_trader.adapters.bullet.factories import BulletLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.strategy import Strategy
from nautilus_trader.trading.strategy import StrategyConfig


# ── Config ─────────────────────────────────────────────────────────────────────

_SYMBOL = os.environ.get("BULLET_SYMBOL", "SOL-USD-PERP.BULLET")
_BASE_URL = os.environ.get("BULLET_BASE_URL", "https://tradingapi.testnet.bullet.xyz")


class BulletMMConfig(StrategyConfig, frozen=True):
    instrument_id: InstrumentId
    half_spread_bps: Decimal = Decimal("10")
    qty: Decimal = Decimal("0.5")
    max_position: Decimal = Decimal("5.0")
    refresh_threshold: Decimal = Decimal("0.5")


# ── Strategy ───────────────────────────────────────────────────────────────────

class BulletMarketMaker(Strategy):
    """
    Simple market maker strategy for Bullet.xyz perpetuals.

    Maintains a bid/ask quote around the mid price derived from live quote ticks.
    Refreshes orders when the mid drifts beyond `refresh_threshold * half_spread`.
    Skips quoting one side when the net position approaches `max_position`.
    """

    def __init__(self, config: BulletMMConfig) -> None:
        super().__init__(config)
        self.instrument_id = config.instrument_id
        self.half_spread_bps = config.half_spread_bps
        self.qty = config.qty
        self.max_position = config.max_position
        self.refresh_threshold = config.refresh_threshold

        self._bid_client_order_id = None
        self._ask_client_order_id = None
        self._bid_price: Decimal | None = None
        self._ask_price: Decimal | None = None
        self._tick_size: Decimal | None = None
        self._size_increment: Decimal | None = None

    def on_start(self) -> None:
        instrument = self.cache.instrument(self.instrument_id)
        if instrument is None:
            self.log.error(f"Instrument {self.instrument_id} not found in cache")
            return

        self._tick_size = Decimal(str(instrument.price_increment))
        self._size_increment = Decimal(str(instrument.size_increment))
        self.subscribe_quote_ticks(self.instrument_id)
        self.log.info(f"Started — tick={self._tick_size}  step={self._size_increment}")

    def on_stop(self) -> None:
        self.cancel_all_orders(self.instrument_id)
        self.log.info("Stopped — all orders canceled")

    def on_quote_tick(self, tick: QuoteTick) -> None:
        if self._tick_size is None:
            return

        bid_px = Decimal(str(tick.bid_price))
        ask_px = Decimal(str(tick.ask_price))
        mid = (bid_px + ask_px) / 2

        half_spread = mid * self.half_spread_bps / Decimal("10000")
        target_bid = self._snap_down(mid - half_spread, self._tick_size)
        target_ask = self._snap_up(mid + half_spread, self._tick_size)

        if not self._needs_refresh(target_bid, target_ask, half_spread):
            return

        qty = self._snap_qty(self.qty, self._size_increment)

        position = self.portfolio.net_position(self.instrument_id)
        net_pos = Decimal(str(position)) if position is not None else Decimal("0")
        quote_bid = net_pos < self.max_position
        quote_ask = net_pos > -self.max_position

        self.log.info(
            f"Refresh: mid={mid:.4f}  bid={target_bid}  ask={target_ask}  pos={net_pos}"
        )

        # Cancel existing orders then re-quote
        if self._bid_client_order_id:
            self.cancel_order(self.cache.order(self._bid_client_order_id))
            self._bid_client_order_id = None
            self._bid_price = None

        if self._ask_client_order_id:
            self.cancel_order(self.cache.order(self._ask_client_order_id))
            self._ask_client_order_id = None
            self._ask_price = None

        instrument = self.cache.instrument(self.instrument_id)
        if instrument is None:
            return

        if quote_bid:
            order = self.order_factory.limit(
                instrument_id=self.instrument_id,
                order_side=OrderSide.BUY,
                quantity=Quantity(float(qty), instrument.size_precision),
                price=Price(float(target_bid), instrument.price_precision),
                time_in_force=TimeInForce.GTC,
            )
            self.submit_order(order)
            self._bid_client_order_id = order.client_order_id
            self._bid_price = target_bid
            self.log.info(f"  BID {qty} @ {target_bid}")
        else:
            self.log.info("Position limit on long side — skipping bid")

        if quote_ask:
            order = self.order_factory.limit(
                instrument_id=self.instrument_id,
                order_side=OrderSide.SELL,
                quantity=Quantity(float(qty), instrument.size_precision),
                price=Price(float(target_ask), instrument.price_precision),
                time_in_force=TimeInForce.GTC,
            )
            self.submit_order(order)
            self._ask_client_order_id = order.client_order_id
            self._ask_price = target_ask
            self.log.info(f"  ASK {qty} @ {target_ask}")
        else:
            self.log.info("Position limit on short side — skipping ask")

    def on_event(self, event) -> None:
        if isinstance(event, OrderFilled):
            position = self.portfolio.net_position(self.instrument_id)
            self.log.info(
                f"Fill: {event.order_side.name} {event.last_qty} @ {event.last_px}"
                f"  pos={position}"
            )
            # Clear the tracked order ID so on_quote_tick re-quotes on next tick
            if event.client_order_id == self._bid_client_order_id:
                self._bid_client_order_id = None
                self._bid_price = None
            elif event.client_order_id == self._ask_client_order_id:
                self._ask_client_order_id = None
                self._ask_price = None

    # ── Helpers ───────────────────────────────────────────────────────────────

    def _needs_refresh(self, new_bid: Decimal, new_ask: Decimal, half_spread: Decimal) -> bool:
        if self._bid_price is None or self._ask_price is None:
            return True
        bid_drift = abs(new_bid - self._bid_price)
        ask_drift = abs(new_ask - self._ask_price)
        return (
            bid_drift > half_spread * self.refresh_threshold
            or ask_drift > half_spread * self.refresh_threshold
        )

    @staticmethod
    def _snap_down(value: Decimal, tick: Decimal) -> Decimal:
        return (value / tick).to_integral_value(rounding=ROUND_DOWN) * tick

    @staticmethod
    def _snap_up(value: Decimal, tick: Decimal) -> Decimal:
        return (value / tick).to_integral_value(rounding=ROUND_UP) * tick

    @staticmethod
    def _snap_qty(value: Decimal, step: Decimal) -> Decimal:
        return (value / step).to_integral_value(rounding=ROUND_DOWN) * step


# ── Node configuration ─────────────────────────────────────────────────────────

def build_node() -> TradingNode:
    instrument_id = InstrumentId.from_str(_SYMBOL)
    ws_url = _BASE_URL.replace("https://", "wss://").replace("http://", "ws://") + "/ws"

    data_config = BulletDataClientConfig(
        base_url_http=_BASE_URL,
        base_url_ws=ws_url,
        instrument_provider=InstrumentProviderConfig(load_all=True),
    )

    exec_config = BulletExecClientConfig(
        base_url_http=_BASE_URL,
        base_url_ws=ws_url,
        private_key=os.environ.get("BULLET_PRIVATE_KEY"),
        key_file=os.environ.get("BULLET_KEY_FILE"),
        account_address=os.environ.get("BULLET_ACCOUNT_ADDRESS"),
    )

    strategy = BulletMarketMaker(
        config=BulletMMConfig(
            instrument_id=instrument_id,
            half_spread_bps=Decimal(os.environ.get("MM_SPREAD_BPS", "10")),
            qty=Decimal(os.environ.get("MM_QTY", "0.5")),
            max_position=Decimal(os.environ.get("MM_MAX_POSITION", "5.0")),
            refresh_threshold=Decimal(os.environ.get("MM_REFRESH_THRESHOLD", "0.5")),
        )
    )

    node_config = TradingNodeConfig(
        trader_id="BULLET-MM-001",
        data_clients={"BULLET": data_config},
        exec_clients={"BULLET": exec_config},
        exec_engine=LiveExecEngineConfig(reconciliation=True),
    )

    node = TradingNode(config=node_config)
    node.add_data_client_factory("BULLET", BulletLiveDataClientFactory)
    node.add_exec_client_factory("BULLET", BulletLiveExecClientFactory)
    node.build()
    node.trader.add_strategy(strategy)
    return node


if __name__ == "__main__":
    node = build_node()
    try:
        node.run()
    except KeyboardInterrupt:
        node.stop()
