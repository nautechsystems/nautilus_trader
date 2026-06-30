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
Sweep strategy.

Maintains one post-only bid and one post-only ask around the live quote mid at
``mid +/- quote_offset_bps``. The quote pair is modified in place when the mid
moves by ``quote_recenter_threshold_bps`` from the last quote anchor. When either
quote fills, the strategy cancels the remaining quote liquidity and works a
reduce-only unwind order at the configured touch until the filled inventory is
flat. The unwind order is modified when the touch drifts by
``unwind_recenter_threshold_bps`` from the working unwind price.

"""

from __future__ import annotations

import json
from decimal import Decimal

from nautilus_trader.config import NonNegativeFloat
from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderModifyRejected
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.tick_scheme.base import round_down
from nautilus_trader.model.tick_scheme.base import round_up
from nautilus_trader.trading.strategy import Strategy


class SweepStrategyConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``SweepStrategy`` instances.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID to trade.
    order_qty : Decimal
        The size for each quoting order.
    quote_offset_bps : NonNegativeFloat, default 10.0
        Distance from mid for the symmetric quote pair.
    quote_recenter_threshold_bps : NonNegativeFloat, default 5.0
        Mid move from the last quote anchor required before modifying quotes.
        Use zero to recenter every quote tick.
    unwind_recenter_threshold_bps : NonNegativeFloat, default 0.0
        Touch move from the working unwind order required before modifying its
        price. Use zero to recenter on every touch price change.
    unwind_cross_touch : bool, default False
        If ``False``, unwind passively at the same-side touch: sell at best ask
        after a buy fill, buy at best bid after a sell fill. If ``True``, unwind
        by crossing to the opposite touch: sell at best bid or buy at best ask.
    client_id : ClientId, optional
        Explicit client route. If ``None``, routing is inferred from venue.
    close_positions_on_stop : bool, default True
        If true, call ``close_all_positions`` during stop.
    reduce_only_on_stop : bool, default True
        Passed through to ``close_all_positions`` during stop.
    log_data : bool, default False
        If true, log incoming quote ticks.

    """

    instrument_id: InstrumentId
    order_qty: Decimal
    quote_offset_bps: NonNegativeFloat = 10.0
    quote_recenter_threshold_bps: NonNegativeFloat = 5.0
    unwind_recenter_threshold_bps: NonNegativeFloat = 0.0
    unwind_cross_touch: bool = False
    client_id: ClientId | None = None
    close_positions_on_stop: bool = True
    reduce_only_on_stop: bool = True
    log_data: bool = False

    @classmethod
    def parse(cls, raw: bytes | str) -> SweepStrategyConfig:
        config = json.loads(raw)
        if "recenter_threshold_bps" in config:
            config.setdefault(
                "quote_recenter_threshold_bps",
                config.pop("recenter_threshold_bps"),
            )
        return super().parse(json.dumps(config))


class SweepStrategy(Strategy):
    """
    Quote ``mid +/- N bps``, modify in place on mid movement, then unwind fills.

    The strategy intentionally keeps only one active behavior at a time:

    - quoting mode: maintain one bid and one ask using ``modify_order``;
    - unwind mode: cancel quote liquidity and keep one reduce-only order at touch.

    """

    def __init__(self, config: SweepStrategyConfig) -> None:
        super().__init__(config)
        self._instrument: Instrument | None = None
        self._quote_qty: Quantity | None = None
        self._price_precision: int | None = None
        self._last_quote: QuoteTick | None = None
        self._anchor_mid: Decimal | None = None
        self._inventory_to_unwind = Decimal(0)
        self._bid_order: LimitOrder | None = None
        self._ask_order: LimitOrder | None = None
        self._unwind_order: LimitOrder | None = None
        self._quote_order_ids: set[ClientOrderId] = set()

    def on_start(self) -> None:
        self._instrument = self.cache.instrument(self.config.instrument_id)
        if self._instrument is None:
            self.log.error(f"Could not find instrument for {self.config.instrument_id}")
            self.stop()
            return

        self._quote_qty = self._instrument.make_qty(self.config.order_qty)
        self._price_precision = self._instrument.price_precision
        self.subscribe_quote_ticks(self.config.instrument_id, client_id=self.config.client_id)

    def on_quote_tick(self, tick: QuoteTick) -> None:
        if self._instrument is None or self._quote_qty is None:
            return

        self._last_quote = tick
        if self.config.log_data:
            self.log.info(repr(tick))

        self._clear_closed_refs()

        if self._needs_unwind():
            self._cancel_quote_orders()
            self._maintain_unwind_order(tick)
            return

        mid = self._mid(tick)
        if self._anchor_mid is None or not self._has_live_quote_pair():
            self._maintain_quote_pair(mid)
            self._anchor_mid = mid
            return

        if self._should_recenter(mid):
            self._maintain_quote_pair(mid)
            self._anchor_mid = mid

    def on_order_filled(self, event: OrderFilled) -> None:
        if event.instrument_id != self.config.instrument_id:
            return

        fill_qty = self._qty_decimal(event.last_qty)
        if event.client_order_id in self._quote_order_ids:
            if event.order_side == OrderSide.BUY:
                self._inventory_to_unwind += fill_qty
            else:
                self._inventory_to_unwind -= fill_qty
            self.log.info(
                f"Quote filled: side={event.order_side}, qty={event.last_qty}, "
                f"inventory_to_unwind={self._inventory_to_unwind}",
            )
            self._cancel_quote_orders()
            if self._last_quote is not None:
                self._maintain_unwind_order(self._last_quote)
            return

        if self._unwind_order and event.client_order_id == self._unwind_order.client_order_id:
            self._inventory_to_unwind += (
                fill_qty if event.order_side == OrderSide.BUY else -fill_qty
            )
            if not self._needs_unwind():
                self.log.info("Unwind complete; resuming quote mode")
                self._inventory_to_unwind = Decimal(0)
                self._unwind_order = None
                self._anchor_mid = None
            elif self._last_quote is not None:
                self._maintain_unwind_order(self._last_quote)

    def on_order_rejected(self, event: OrderRejected) -> None:
        if self._is_reduce_only_would_increase_unwind_rejection(event):
            self._clear_stale_unwind_target(event.reason)
            return
        self._handle_terminal_or_rejected(event.client_order_id)

    def on_order_modify_rejected(self, event: OrderModifyRejected) -> None:
        if self._is_reduce_only_would_increase_unwind_rejection(event):
            self._clear_stale_unwind_target(event.reason)
            return
        if self._matches_working_order(event.client_order_id):
            self._anchor_mid = None
        if self._unwind_order and event.client_order_id == self._unwind_order.client_order_id:
            self._unwind_order = None

    def on_order_expired(self, event: OrderExpired) -> None:
        self._handle_terminal_or_rejected(event.client_order_id)

    def on_order_canceled(self, event: OrderCanceled) -> None:
        self._handle_terminal_or_rejected(event.client_order_id)

    def on_stop(self) -> None:
        self.cancel_all_orders(self.config.instrument_id, client_id=self.config.client_id)
        if self.config.close_positions_on_stop:
            self.close_all_positions(
                self.config.instrument_id,
                client_id=self.config.client_id,
                reduce_only=self.config.reduce_only_on_stop,
            )
        self.unsubscribe_quote_ticks(self.config.instrument_id, client_id=self.config.client_id)

    def on_reset(self) -> None:
        self._instrument = None
        self._quote_qty = None
        self._price_precision = None
        self._last_quote = None
        self._anchor_mid = None
        self._inventory_to_unwind = Decimal(0)
        self._bid_order = None
        self._ask_order = None
        self._unwind_order = None
        self._quote_order_ids.clear()

    def _maintain_quote_pair(self, mid: Decimal) -> None:
        prices = self._quote_prices(mid)
        if prices is None:
            return
        bid_price, ask_price = prices
        self._maintain_quote_order(OrderSide.BUY, bid_price)
        self._maintain_quote_order(OrderSide.SELL, ask_price)

    def _maintain_quote_order(self, side: OrderSide, price: Price) -> None:
        if self._instrument is None or self._quote_qty is None:
            return

        order = self._bid_order if side == OrderSide.BUY else self._ask_order
        if order is None or order.is_closed:
            order = self.order_factory.limit(
                instrument_id=self.config.instrument_id,
                order_side=side,
                quantity=self._quote_qty,
                price=price,
                time_in_force=TimeInForce.GTC,
                post_only=True,
            )
            self._set_quote_order(side, order)
            self._quote_order_ids.add(order.client_order_id)
            self.submit_order(order, client_id=self.config.client_id)
            return

        if order.is_pending_cancel:
            return

        if order.price != price:
            self.modify_order(order, price=price, client_id=self.config.client_id)

    def _maintain_unwind_order(self, tick: QuoteTick) -> None:
        if self._instrument is None:
            return
        if not self._needs_unwind():
            return

        side = OrderSide.SELL if self._inventory_to_unwind > 0 else OrderSide.BUY
        price = self._unwind_price(tick, side)
        if price is None:
            return

        if self._unwind_order is not None and self._unwind_order.is_closed:
            self._unwind_order = None

        if self._unwind_order is not None and self._unwind_order.side != side:
            self._cancel_if_active(self._unwind_order)
            return

        if self._unwind_order is None:
            self._unwind_order = self.order_factory.limit(
                instrument_id=self.config.instrument_id,
                order_side=side,
                quantity=self._instrument.make_qty(abs(self._inventory_to_unwind)),
                price=price,
                time_in_force=TimeInForce.GTC,
                post_only=not self.config.unwind_cross_touch,
                reduce_only=True,
            )
            self.submit_order(self._unwind_order, client_id=self.config.client_id)
            return

        if self._unwind_order.is_pending_cancel:
            return

        desired_total = abs(self._inventory_to_unwind) + self._qty_decimal(
            self._unwind_order.filled_qty,
        )
        quantity = self._instrument.make_qty(desired_total)
        recenter_price = self._should_recenter_unwind_order(price)
        quantity_changed = self._unwind_order.quantity != quantity
        if recenter_price or quantity_changed:
            self.modify_order(
                self._unwind_order,
                quantity=quantity if quantity_changed else None,
                price=price if recenter_price else None,
                client_id=self.config.client_id,
            )

    def _quote_prices(self, mid: Decimal) -> tuple[Price, Price] | None:
        pct = Decimal(str(self.config.quote_offset_bps)) / Decimal(10_000)
        bid_price = self._make_bid_price(mid * (Decimal(1) - pct))
        ask_price = self._make_ask_price(mid * (Decimal(1) + pct))
        if bid_price is None or ask_price is None:
            return None
        if bid_price >= ask_price:
            self.log.warning(
                f"Skipping quote pair because rounded bid {bid_price} >= ask {ask_price}",
            )
            return None
        return bid_price, ask_price

    def _unwind_price(self, tick: QuoteTick, side: OrderSide) -> Price | None:
        if side == OrderSide.SELL:
            if self.config.unwind_cross_touch:
                return self._make_bid_price(tick.bid_price.as_decimal())
            return self._make_ask_price(tick.ask_price.as_decimal())

        if self.config.unwind_cross_touch:
            return self._make_ask_price(tick.ask_price.as_decimal())
        return self._make_bid_price(tick.bid_price.as_decimal())

    def _make_bid_price(self, raw: Decimal) -> Price | None:
        return self._make_price(raw, is_bid=True)

    def _make_ask_price(self, raw: Decimal) -> Price | None:
        return self._make_price(raw, is_bid=False)

    def _make_price(self, raw: Decimal, is_bid: bool) -> Price | None:
        if self._instrument is None or self._price_precision is None or raw <= 0:
            return None

        if self._instrument.tick_scheme_name is not None:
            price = (
                self._instrument.next_bid_price(float(raw))
                if is_bid
                else self._instrument.next_ask_price(float(raw))
            )
        else:
            increment = float(self._instrument.price_increment)
            rounded = (
                round_down(float(raw), increment)
                if is_bid
                else round_up(float(raw), increment)
            )
            price = Price(rounded, self._price_precision)

        min_px = self._instrument.min_price
        max_px = self._instrument.max_price
        if min_px is not None and price < min_px:
            return None
        if max_px is not None and price > max_px:
            return None
        return price

    def _mid(self, tick: QuoteTick) -> Decimal:
        return (tick.bid_price.as_decimal() + tick.ask_price.as_decimal()) / Decimal(2)

    def _should_recenter(self, mid: Decimal) -> bool:
        if self._anchor_mid is None or self._anchor_mid <= 0:
            return True
        if self.config.quote_recenter_threshold_bps == 0:
            return True
        moved_bps = abs(mid - self._anchor_mid) / self._anchor_mid * Decimal(10_000)
        return moved_bps >= Decimal(str(self.config.quote_recenter_threshold_bps))

    def _should_recenter_unwind_order(self, price: Price) -> bool:
        if self._unwind_order is None or self._unwind_order.price == price:
            return False
        if self.config.unwind_recenter_threshold_bps == 0:
            return True

        current_price = self._unwind_order.price.as_decimal()
        if current_price <= 0:
            return True

        drift_bps = abs(price.as_decimal() - current_price) / current_price * Decimal(10_000)
        return drift_bps >= Decimal(str(self.config.unwind_recenter_threshold_bps))

    def _needs_unwind(self) -> bool:
        return self._inventory_to_unwind != 0

    def _has_live_quote_pair(self) -> bool:
        return self._is_working(self._bid_order) and self._is_working(self._ask_order)

    def _set_quote_order(self, side: OrderSide, order: LimitOrder | None) -> None:
        if side == OrderSide.BUY:
            self._bid_order = order
        else:
            self._ask_order = order

    def _cancel_quote_orders(self) -> None:
        self._cancel_if_active(self._bid_order)
        self._cancel_if_active(self._ask_order)
        self._bid_order = None
        self._ask_order = None
        self._anchor_mid = None

    def _cancel_if_active(self, order: LimitOrder | None) -> None:
        if order is None or order.is_closed or order.is_pending_cancel:
            return
        self.cancel_order(order, client_id=self.config.client_id)

    def _clear_closed_refs(self) -> None:
        if self._bid_order is not None and self._bid_order.is_closed:
            self._bid_order = None
        if self._ask_order is not None and self._ask_order.is_closed:
            self._ask_order = None
        if self._unwind_order is not None and self._unwind_order.is_closed:
            self._unwind_order = None

    def _handle_terminal_or_rejected(self, client_order_id: ClientOrderId) -> None:
        if self._bid_order and client_order_id == self._bid_order.client_order_id:
            self._bid_order = None
            self._anchor_mid = None
        if self._ask_order and client_order_id == self._ask_order.client_order_id:
            self._ask_order = None
            self._anchor_mid = None
        if self._unwind_order and client_order_id == self._unwind_order.client_order_id:
            self._unwind_order = None

    def _is_reduce_only_would_increase_unwind_rejection(
        self,
        event: OrderRejected | OrderModifyRejected,
    ) -> bool:
        if event.instrument_id != self.config.instrument_id:
            return False
        if self._unwind_order is None:
            return False
        if event.client_order_id != self._unwind_order.client_order_id:
            return False

        reason = event.reason.lower().replace("-", " ")
        return "reduce only" in reason and "increase position" in reason

    def _clear_stale_unwind_target(self, reason: str) -> None:
        self.log.info(
            "Clearing stale unwind target after reduce-only rejection; "
            f"resuming quote mode: {reason}",
        )
        self._inventory_to_unwind = Decimal(0)
        self._unwind_order = None
        self._anchor_mid = None

    def _matches_working_order(self, client_order_id: ClientOrderId) -> bool:
        return bool(
            (self._bid_order and client_order_id == self._bid_order.client_order_id)
            or (self._ask_order and client_order_id == self._ask_order.client_order_id)
            or (self._unwind_order and client_order_id == self._unwind_order.client_order_id),
        )

    @staticmethod
    def _is_working(order: LimitOrder | None) -> bool:
        return order is not None and not order.is_closed and not order.is_pending_cancel

    @staticmethod
    def _qty_decimal(quantity: Quantity) -> Decimal:
        return Decimal(str(quantity))
