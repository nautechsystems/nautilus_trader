#!/usr/bin/env python3
"""
Fill tester for Bullet.xyz adapter.

Places a limit BUY priced 10 ticks above the current best ask (guaranteed fill),
then verifies that `OrderFilled` fires via the WS TradeFill path.

Exits 0 on success, 1 on timeout.

Usage:
    BULLET_PRIVATE_KEY=<key> python bullet_fill_tester.py
    BULLET_KEY_FILE=~/.config/bullet/id.json python bullet_fill_tester.py
"""

from __future__ import annotations

import os
import sys
import threading
import time
from decimal import Decimal, ROUND_UP

from nautilus_trader.adapters.bullet.config import BulletDataClientConfig, BulletExecClientConfig
from nautilus_trader.adapters.bullet.factories import BulletLiveDataClientFactory, BulletLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig, LiveExecEngineConfig, TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import OrderSide, TimeInForce
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price, Quantity
from nautilus_trader.trading.strategy import Strategy, StrategyConfig

_SYMBOL = os.environ.get("BULLET_SYMBOL", "SOL-USD-PERP.BULLET")
_BASE_URL = os.environ.get("BULLET_BASE_URL", "https://tradingapi.testnet.bullet.xyz")
_TIMEOUT_SECS = 45


class FillTesterConfig(StrategyConfig, frozen=True):
    instrument_id: InstrumentId
    ticks_above_ask: int = 10


class FillTesterStrategy(Strategy):
    """
    Places one aggressive limit BUY above the ask, waits for OrderFilled.
    Sets the `done` event (threading.Event) on fill.
    """

    def __init__(self, config: FillTesterConfig, done: threading.Event) -> None:
        super().__init__(config)
        self.instrument_id = config.instrument_id
        self.ticks_above_ask = config.ticks_above_ask
        self._done = done
        self._order_placed = False
        self.success = False

    def on_start(self) -> None:
        self.subscribe_quote_ticks(self.instrument_id)
        self.log.info(f"Waiting for quote tick on {self.instrument_id}...")

    def on_stop(self) -> None:
        self.cancel_all_orders(self.instrument_id)

    def on_quote_tick(self, tick: QuoteTick) -> None:
        if self._order_placed:
            return

        instrument = self.cache.instrument(self.instrument_id)
        if instrument is None:
            return

        tick_size = Decimal(str(instrument.price_increment))
        size_step = Decimal(str(instrument.size_increment))

        ask_px = Decimal(str(tick.ask_price))
        buy_px = ask_px + tick_size * self.ticks_above_ask
        buy_px = (buy_px / tick_size).to_integral_value(rounding=ROUND_UP) * tick_size

        qty = size_step  # minimum order size

        order = self.order_factory.limit(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=Quantity(float(qty), instrument.size_precision),
            price=Price(float(buy_px), instrument.price_precision),
            time_in_force=TimeInForce.GTC,
        )
        self.submit_order(order)
        self._order_placed = True
        self.log.info(f"Placed market-crossing BUY {qty} @ {buy_px} (ask was {ask_px})")

    def on_event(self, event) -> None:
        if isinstance(event, OrderFilled):
            self.log.info(
                f"SUCCESS: OrderFilled — {event.order_side.name} {event.last_qty} @ {event.last_px}"
            )
            self.success = True
            self._done.set()


def build_node(done: threading.Event) -> tuple[TradingNode, FillTesterStrategy]:
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
    strategy = FillTesterStrategy(
        config=FillTesterConfig(instrument_id=instrument_id),
        done=done,
    )
    node_config = TradingNodeConfig(
        trader_id="BULLET-FILL-TEST-001",
        data_clients={"BULLET": data_config},
        exec_clients={"BULLET": exec_config},
        exec_engine=LiveExecEngineConfig(reconciliation=True),
    )
    node = TradingNode(config=node_config)
    node.add_data_client_factory("BULLET", BulletLiveDataClientFactory)
    node.add_exec_client_factory("BULLET", BulletLiveExecClientFactory)
    node.build()
    node.trader.add_strategy(strategy)
    return node, strategy


if __name__ == "__main__":
    done = threading.Event()
    node, strategy = build_node(done)

    t = threading.Thread(target=node.run, daemon=True)
    t.start()

    triggered = done.wait(timeout=_TIMEOUT_SECS)
    node.stop()
    t.join(timeout=10)

    if triggered and strategy.success:
        print("\nFILL TEST PASSED", flush=True)
        sys.exit(0)
    else:
        print(f"\nFILL TEST FAILED — no OrderFilled within {_TIMEOUT_SECS}s", flush=True)
        sys.exit(1)
