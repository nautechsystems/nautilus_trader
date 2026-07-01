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

Maintains one post-only bid and one post-only ask around the live quote mid. The
quote pair is modified in place when the active quote offset changes, or when the
mid moves by ``quote_recenter_threshold_bps`` from the last quote anchor. When
either quote fills, the strategy cancels the remaining quote liquidity and works
a reduce-only unwind order at the configured touch until the filled inventory is
flat. The unwind order is modified when the touch drifts by
``unwind_recenter_threshold_bps`` from the working unwind price.

"""

from __future__ import annotations

import json
from datetime import datetime
from datetime import time
from datetime import timedelta
from datetime import timezone
from decimal import Decimal
from time import monotonic_ns
from zoneinfo import ZoneInfo

from nautilus_trader.config import NonNegativeFloat
from nautilus_trader.config import PositiveFloat
from nautilus_trader.config import StrategyConfig
from nautilus_trader.datadog import enabled as dd_enabled
from nautilus_trader.datadog import gauge as dd_gauge
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


WIDE_MODE_DURATION_MINUTES = Decimal(30)


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
    wide_mode_quote_offset_bps : PositiveFloat, optional
        If set, use this quote offset for the first 30 minutes after the
        configured market open and after-hours start.
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
    bbo_distance_telemetry_interval_ms : NonNegativeFloat, default 1000.0
        Minimum interval between BBO distance telemetry samples.
    market_open_embargo_minutes : NonNegativeFloat, default 0.0
        Number of minutes after ``market_open_embargo_start`` to keep the
        embargo active. Use zero to disable.
    market_open_embargo_pre_open_minutes : NonNegativeFloat, default 0.0
        Number of minutes before ``market_open_embargo_start`` to begin the
        embargo.
    market_open_embargo_timezone : str, default "America/New_York"
        IANA timezone used to evaluate market boundary embargoes.
    market_open_embargo_start : str, default "09:30:00"
        Local market open time in ``HH:MM[:SS]`` format.
    market_after_hours_embargo_minutes : NonNegativeFloat, default 0.0
        Number of minutes after ``market_after_hours_embargo_start`` to keep the
        embargo active. Use zero to disable.
    market_after_hours_embargo_pre_start_minutes : NonNegativeFloat, default 0.0
        Number of minutes before ``market_after_hours_embargo_start`` to begin the
        after-hours embargo.
    market_after_hours_embargo_start : str, default "16:00:00"
        Local after-hours start time in ``HH:MM[:SS]`` format.
    close_positions_on_embargo : bool, default False
        If true, call ``close_all_positions`` once when entering the embargo.
    reduce_only_on_embargo : bool, default True
        Passed through to ``close_all_positions`` during embargo handling.
    log_data : bool, default False
        If true, log incoming quote ticks.

    """

    instrument_id: InstrumentId
    order_qty: Decimal
    quote_offset_bps: NonNegativeFloat = 10.0
    wide_mode_quote_offset_bps: PositiveFloat | None = None
    quote_recenter_threshold_bps: NonNegativeFloat = 5.0
    unwind_recenter_threshold_bps: NonNegativeFloat = 0.0
    unwind_cross_touch: bool = False
    client_id: ClientId | None = None
    close_positions_on_stop: bool = True
    reduce_only_on_stop: bool = True
    bbo_distance_telemetry_interval_ms: NonNegativeFloat = 1000.0
    market_open_embargo_minutes: NonNegativeFloat = 0.0
    market_open_embargo_pre_open_minutes: NonNegativeFloat = 0.0
    market_open_embargo_timezone: str = "America/New_York"
    market_open_embargo_start: str = "09:30:00"
    market_after_hours_embargo_minutes: NonNegativeFloat = 0.0
    market_after_hours_embargo_pre_start_minutes: NonNegativeFloat = 0.0
    market_after_hours_embargo_start: str = "16:00:00"
    close_positions_on_embargo: bool = False
    reduce_only_on_embargo: bool = True
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
        self._quote_offset_bps_in_use: Decimal | None = None
        self._inventory_to_unwind = Decimal(0)
        self._bid_order: LimitOrder | None = None
        self._ask_order: LimitOrder | None = None
        self._unwind_order: LimitOrder | None = None
        self._quote_order_ids: set[ClientOrderId] = set()
        self._last_bbo_distance_telemetry_ns = 0
        self._embargo_active = False
        self._embargo_tz = ZoneInfo(config.market_open_embargo_timezone)
        self._embargo_start_time = time.fromisoformat(config.market_open_embargo_start)
        self._after_hours_embargo_start_time = time.fromisoformat(
            config.market_after_hours_embargo_start,
        )

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
        if self._handle_market_boundary_embargo():
            return

        mid: Decimal | None = None
        if self._should_record_bbo_distance():
            mid = self._mid(tick)
            self._record_bbo_distance_from_mid(tick, mid)

        if self._needs_unwind():
            self._cancel_quote_orders()
            self._maintain_unwind_order(tick)
            return

        if mid is None:
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
        embargo_active = self._is_market_boundary_embargo()
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
            if embargo_active:
                self.log.info(
                    "Quote fill received during market boundary embargo; "
                    f"client_order_id={event.client_order_id}",
                )
                self._embargo_active = True
                self._apply_market_boundary_embargo()
                return
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
                if embargo_active:
                    self._embargo_active = True
                    self._apply_market_boundary_embargo()
            elif embargo_active:
                self._embargo_active = True
                self._apply_market_boundary_embargo()
            elif self._last_quote is not None:
                self._maintain_unwind_order(self._last_quote)
            return

        if embargo_active:
            self.log.info(
                "Order fill received during market boundary embargo; "
                f"client_order_id={event.client_order_id}",
            )
            self._embargo_active = True
            self._apply_market_boundary_embargo()

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
        self._quote_offset_bps_in_use = None
        self._inventory_to_unwind = Decimal(0)
        self._bid_order = None
        self._ask_order = None
        self._unwind_order = None
        self._quote_order_ids.clear()
        self._last_bbo_distance_telemetry_ns = 0
        self._embargo_active = False

    def _maintain_quote_pair(self, mid: Decimal) -> None:
        quote_offset_bps = self._active_quote_offset_bps()
        prices = self._quote_prices(mid, quote_offset_bps)
        if prices is None:
            return
        bid_price, ask_price = prices
        self._maintain_quote_order(OrderSide.BUY, bid_price)
        self._maintain_quote_order(OrderSide.SELL, ask_price)
        self._quote_offset_bps_in_use = quote_offset_bps

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

    def _quote_prices(
        self,
        mid: Decimal,
        quote_offset_bps: Decimal,
    ) -> tuple[Price, Price] | None:
        pct = quote_offset_bps / Decimal(10_000)
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
                round_down(float(raw), increment) if is_bid else round_up(float(raw), increment)
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

    def _should_record_bbo_distance(self) -> bool:
        if not dd_enabled():
            return False

        interval_ns = int(self.config.bbo_distance_telemetry_interval_ms * 1_000_000)
        if interval_ns <= 0:
            return True

        now_ns = monotonic_ns()
        if now_ns - self._last_bbo_distance_telemetry_ns < interval_ns:
            return False

        self._last_bbo_distance_telemetry_ns = now_ns
        return True

    def _record_bbo_distance_from_mid(self, tick: QuoteTick, mid: Decimal) -> None:
        if mid <= 0:
            return

        bid_distance_bps = (mid - tick.bid_price.as_decimal()) / mid * Decimal(10_000)
        ask_distance_bps = (tick.ask_price.as_decimal() - mid) / mid * Decimal(10_000)
        tags = (
            f"venue:{self.config.instrument_id.venue}",
            f"instrument:{self.config.instrument_id}",
            f"strategy:{self.id}",
        )
        dd_gauge(
            "market_data.quote.best_bid_distance_bps",
            float(bid_distance_bps),
            tags=tags,
        )
        dd_gauge(
            "market_data.quote.best_ask_distance_bps",
            float(ask_distance_bps),
            tags=tags,
        )

    def _should_recenter(self, mid: Decimal) -> bool:
        if self._anchor_mid is None or self._anchor_mid <= 0:
            return True
        if self._quote_offset_bps_in_use != self._active_quote_offset_bps():
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

    def _handle_market_boundary_embargo(self) -> bool:
        if not self._is_market_boundary_embargo():
            if self._embargo_active:
                self.log.info("Market boundary embargo ended; resuming order maintenance")
                self._embargo_active = False
                self._anchor_mid = None
            return False

        if not self._embargo_active:
            self.log.warning(
                "Market boundary embargo active; canceling risk-adding orders"
                + (
                    " and closing positions"
                    if self.config.close_positions_on_embargo
                    else ""
                ),
            )
            self._embargo_active = True
            self._apply_market_boundary_embargo()
        else:
            self._cancel_risk_adding_orders_for_embargo()
        return True

    def _is_market_open_embargo(self) -> bool:
        if self.config.market_open_embargo_minutes <= 0:
            return False

        now_utc = self.clock.utc_now()
        if now_utc.tzinfo is None:
            now_utc = now_utc.replace(tzinfo=timezone.utc)

        return self._datetime_in_market_boundary_embargo(
            now_utc,
            self._embargo_tz,
            self._embargo_start_time,
            Decimal(str(self.config.market_open_embargo_minutes)),
            Decimal(str(self.config.market_open_embargo_pre_open_minutes)),
        )

    def _is_market_after_hours_embargo(self) -> bool:
        if self.config.market_after_hours_embargo_minutes <= 0:
            return False

        now_utc = self.clock.utc_now()
        if now_utc.tzinfo is None:
            now_utc = now_utc.replace(tzinfo=timezone.utc)

        return self._datetime_in_market_boundary_embargo(
            now_utc,
            self._embargo_tz,
            self._after_hours_embargo_start_time,
            Decimal(str(self.config.market_after_hours_embargo_minutes)),
            Decimal(str(self.config.market_after_hours_embargo_pre_start_minutes)),
        )

    def _is_market_boundary_embargo(self) -> bool:
        return self._is_market_open_embargo() or self._is_market_after_hours_embargo()

    def _is_wide_mode(self) -> bool:
        if self.config.wide_mode_quote_offset_bps is None:
            return False

        now_utc = self.clock.utc_now()
        if now_utc.tzinfo is None:
            now_utc = now_utc.replace(tzinfo=timezone.utc)

        return self._datetime_in_market_boundary_window(
            now_utc,
            self._embargo_tz,
            self._embargo_start_time,
            WIDE_MODE_DURATION_MINUTES,
        ) or self._datetime_in_market_boundary_window(
            now_utc,
            self._embargo_tz,
            self._after_hours_embargo_start_time,
            WIDE_MODE_DURATION_MINUTES,
        )

    def _active_quote_offset_bps(self) -> Decimal:
        if self.config.wide_mode_quote_offset_bps is not None and self._is_wide_mode():
            return Decimal(str(self.config.wide_mode_quote_offset_bps))
        return Decimal(str(self.config.quote_offset_bps))

    def _apply_market_boundary_embargo(self) -> None:
        if self.config.close_positions_on_embargo:
            self.cancel_all_orders(self.config.instrument_id, client_id=self.config.client_id)
            self._clear_local_order_refs_for_embargo()
            self._inventory_to_unwind = Decimal(0)
            self.close_all_positions(
                self.config.instrument_id,
                client_id=self.config.client_id,
                reduce_only=self.config.reduce_only_on_embargo,
            )
            return

        self._cancel_risk_adding_orders_for_embargo()

    def _cancel_risk_adding_orders_for_embargo(self) -> None:
        self._cancel_quote_orders()

    def _clear_local_order_refs_for_embargo(self) -> None:
        self._bid_order = None
        self._ask_order = None
        self._unwind_order = None
        self._anchor_mid = None
        self._quote_offset_bps_in_use = None

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
        self._quote_offset_bps_in_use = None

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
            self._quote_offset_bps_in_use = None
        if self._ask_order and client_order_id == self._ask_order.client_order_id:
            self._ask_order = None
            self._anchor_mid = None
            self._quote_offset_bps_in_use = None
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
        self._quote_offset_bps_in_use = None

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

    @staticmethod
    def _datetime_in_market_boundary_embargo(
        value: datetime,
        tz: ZoneInfo,
        start_time: time,
        minutes: Decimal,
        pre_start_minutes: Decimal = Decimal(0),
    ) -> bool:
        return SweepStrategy._datetime_in_market_boundary_window(
            value,
            tz,
            start_time,
            minutes,
            pre_start_minutes,
        )

    @staticmethod
    def _datetime_in_market_boundary_window(
        value: datetime,
        tz: ZoneInfo,
        start_time: time,
        minutes: Decimal,
        pre_start_minutes: Decimal = Decimal(0),
    ) -> bool:
        if minutes <= 0 and pre_start_minutes <= 0:
            return False

        local = value.astimezone(tz)
        if local.weekday() >= 5:
            return False

        start = local.replace(
            hour=start_time.hour,
            minute=start_time.minute,
            second=start_time.second,
            microsecond=start_time.microsecond,
        )
        start -= timedelta(minutes=float(pre_start_minutes))
        end = local.replace(
            hour=start_time.hour,
            minute=start_time.minute,
            second=start_time.second,
            microsecond=start_time.microsecond,
        ) + timedelta(minutes=float(minutes))

        elapsed_seconds = Decimal(str((local - start).total_seconds()))
        duration_seconds = Decimal(str((end - start).total_seconds()))
        return Decimal(0) <= elapsed_seconds < duration_seconds
