#!/usr/bin/env python3
"""
Exec tester for Bullet.xyz adapter.

Places a passive limit BUY well below the market, amends the price and quantity,
then cancels it — exercising the full submit → amend → cancel lifecycle without
taking a position.

Bullet amends are cancel-replace: the adapter suppresses the CANCELED for the old
order and emits OrderUpdated when the replacement NEW arrives.

Exits 0 on success (OrderAccepted → OrderUpdated → OrderCanceled received), 1 on timeout.

Usage:
    BULLET_PRIVATE_KEY=<key> python bullet_exec_tester.py
    BULLET_KEY_FILE=~/.config/bullet/id.json python bullet_exec_tester.py
"""

from __future__ import annotations

import os
import sys
import threading
import time
from decimal import Decimal, ROUND_DOWN

from nautilus_trader.adapters.bullet.config import BulletDataClientConfig, BulletExecClientConfig
from nautilus_trader.adapters.bullet.factories import BulletLiveDataClientFactory, BulletLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig, LiveExecEngineConfig, TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import OrderSide, TimeInForce
from nautilus_trader.model.events import OrderAccepted, OrderCanceled, OrderUpdated
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price, Quantity
from nautilus_trader.trading.strategy import Strategy, StrategyConfig

_SYMBOL = os.environ.get("BULLET_SYMBOL", "SOL-USD-PERP.BULLET")
_BASE_URL = os.environ.get("BULLET_BASE_URL", "https://tradingapi.testnet.bullet.xyz")
_TIMEOUT_SECS = 45
# Place order this many ticks below the best bid — passive, will not fill
_TICKS_BELOW_BID = 500


class ExecTesterConfig(StrategyConfig, frozen=True):
    instrument_id: InstrumentId
    ticks_below_bid: int = _TICKS_BELOW_BID


class ExecTesterStrategy(Strategy):
    """
    Runs a place → amend → cancel sequence on a passive order.

    State machine:
      idle → placing (wait for OrderAccepted)
           → amending (wait for OrderUpdated — adapter converts Bullet's cancel-replace)
           → canceling (wait for OrderCanceled)
           → done

    Sets `done` event and `success = True` when all three events have fired.
    """

    def __init__(self, config: ExecTesterConfig, done: threading.Event) -> None:
        super().__init__(config)
        self.instrument_id = config.instrument_id
        self.ticks_below_bid = config.ticks_below_bid
        self._done = done
        self.success = False

        self._state = "idle"
        self._order = None

    def on_start(self) -> None:
        self.subscribe_quote_ticks(self.instrument_id)
        self.log.info("Waiting for first quote tick...")

    def on_stop(self) -> None:
        if self._order is not None and not self._order.is_closed:
            self.cancel_order(self._order)

    def on_quote_tick(self, tick: QuoteTick) -> None:
        if self._state != "idle":
            return

        instrument = self.cache.instrument(self.instrument_id)
        if instrument is None:
            return

        tick_size = Decimal(str(instrument.price_increment))
        size_step = Decimal(str(instrument.size_increment))

        bid_px = Decimal(str(tick.bid_price))
        buy_px = bid_px - tick_size * self.ticks_below_bid
        buy_px = (buy_px / tick_size).to_integral_value(rounding=ROUND_DOWN) * tick_size
        if buy_px <= 0:
            self.log.warning(f"Computed price {buy_px} <= 0, skipping")
            return

        qty = size_step  # minimum

        self._order = self.order_factory.limit(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=Quantity(float(qty), instrument.size_precision),
            price=Price(float(buy_px), instrument.price_precision),
            time_in_force=TimeInForce.GTC,
        )
        self.submit_order(self._order)
        self._state = "placing"
        self.log.info(f"[1/3] Placing passive BUY {qty} @ {buy_px}")

    def on_event(self, event) -> None:
        if self._order is None:
            return

        if isinstance(event, OrderAccepted) and event.client_order_id == self._order.client_order_id:
            if self._state == "placing":
                self._state = "amending"
                self.log.info(f"[1/3] Accepted (venue_order_id={event.venue_order_id}) — amending...")
                instrument = self.cache.instrument(self.instrument_id)
                tick_size = Decimal(str(instrument.price_increment))
                new_price = Decimal(str(self._order.price)) - tick_size
                new_price = (new_price / tick_size).to_integral_value(rounding=ROUND_DOWN) * tick_size
                size_step = Decimal(str(instrument.size_increment))
                new_qty = Decimal(str(self._order.quantity)) + size_step

                self.modify_order(
                    order=self._order,
                    quantity=Quantity(float(new_qty), instrument.size_precision),
                    price=Price(float(new_price), instrument.price_precision),
                )
                self.log.info(f"[2/3] Amend sent: qty={new_qty} px={new_price}")

        elif isinstance(event, OrderUpdated) and event.client_order_id == self._order.client_order_id:
            if self._state == "amending":
                # The adapter translates Bullet's cancel-replace into OrderUpdated.
                self.log.info(
                    f"[2/3] Amend confirmed (OrderUpdated, new venue_id={event.venue_order_id}) "
                    f"— canceling..."
                )
                self._state = "canceling"
                self.cancel_order(self._order)
                self.log.info("[3/3] Cancel sent")

        elif isinstance(event, OrderCanceled) and event.client_order_id == self._order.client_order_id:
            if self._state == "canceling":
                self.log.info("[3/3] Canceled — SUCCESS")
                self.success = True
                self._state = "done"
                self._done.set()


def build_node(done: threading.Event) -> tuple[TradingNode, ExecTesterStrategy]:
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
    strategy = ExecTesterStrategy(
        config=ExecTesterConfig(instrument_id=instrument_id),
        done=done,
    )
    node_config = TradingNodeConfig(
        trader_id="BULLET-EXEC-TEST-001",
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
        print("\nEXEC TEST PASSED — place → amend → cancel lifecycle complete", flush=True)
        sys.exit(0)
    else:
        print(f"\nEXEC TEST FAILED — did not complete within {_TIMEOUT_SECS}s", flush=True)
        sys.exit(1)
