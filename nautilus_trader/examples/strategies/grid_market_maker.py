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

# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***
"""
Grid market making strategy with inventory-based skewing.

Subscribes to quotes for a single instrument and maintains a symmetric grid of limit
orders around the mid-price. Orders are only replaced when the mid-price moves beyond a
configurable threshold. The grid shifts by a skew proportional to the net position to
discourage inventory buildup (Avellaneda-Stoikov inspired).

"""

from __future__ import annotations

from datetime import timedelta
from decimal import Decimal

from nautilus_trader.config import NonNegativeFloat
from nautilus_trader.config import PositiveInt
from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick_scheme.base import round_down
from nautilus_trader.model.tick_scheme.base import round_up
from nautilus_trader.trading.strategy import Strategy


class GridMarketMakerConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``GridMarketMaker`` instances.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID to trade.
    max_position : Quantity
        The maximum net exposure (long or short).
    trade_size : Quantity, optional
        The order size per grid level. If ``None``, resolved from the instrument's
        ``min_quantity`` on start, falling back to 1.0.
    num_levels : PositiveInt, default 3
        The number of buy and sell levels.
    grid_step_bps : PositiveInt, default 10
        The grid spacing in basis points of mid-price (geometric grid).
        E.g. ``10`` = 10 bps = 0.1%. Buy level N = mid * (1 - bps/10000)^N.
    skew_factor : NonNegativeFloat, default 0.0
        How aggressively to shift the grid based on inventory.
        Each unit of net position shifts prices by ``skew_factor`` price units.
    requote_threshold_bps : PositiveInt, default 5
        The minimum mid-price move (bps) before re-quoting.
    expire_time_secs : PositiveInt, optional
        Order expiry in seconds. Uses GTD when set, GTC otherwise.
    on_cancel_resubmit : bool, default False
        If ``True``, reset the requote anchor on unexpected cancel events so the
        next quote tick triggers a full grid resubmission.

    """

    instrument_id: InstrumentId
    max_position: Quantity
    trade_size: Quantity | None = None
    num_levels: PositiveInt = 3
    grid_step_bps: PositiveInt = 10
    skew_factor: NonNegativeFloat = 0.0
    requote_threshold_bps: PositiveInt = 5
    expire_time_secs: PositiveInt | None = None
    on_cancel_resubmit: bool = False


class GridMarketMaker(Strategy):
    """
    Grid market making strategy with inventory-based skewing.

    Places a symmetric grid of post-only limit buy and sell orders around the mid-price.
    Orders persist across ticks and are only replaced when the mid-price moves by at
    least ``requote_threshold_bps``. The grid shifts proportionally to the net position
    to discourage inventory buildup.

    Parameters
    ----------
    config : GridMarketMakerConfig
        The strategy configuration.

    """

    def __init__(self, config: GridMarketMakerConfig) -> None:
        super().__init__(config)
        self._instrument: Instrument | None = None
        self._trade_size: Quantity | None = config.trade_size
        self._price_precision: int | None = None
        self._last_quoted_mid: Price | None = None
        self._pending_self_cancels: set[ClientOrderId] = set()

    def on_start(self) -> None:
        """
        Actions to be performed on strategy start.
        """
        instrument_id = self.config.instrument_id
        self._instrument = self.cache.instrument(instrument_id)
        if self._instrument is None:
            self.log.error(f"Could not find instrument for {instrument_id}")
            self.stop()
            return

        self._price_precision = self._instrument.price_precision

        if self._trade_size is None:
            min_qty = self._instrument.min_quantity
            size_precision = self._instrument.size_precision
            self._trade_size = min_qty if min_qty is not None else Quantity(1.0, size_precision)

        self.subscribe_quote_ticks(instrument_id)

    def on_stop(self) -> None:
        """
        Actions to be performed on strategy stop.
        """
        if self._instrument is None:
            return
        instrument_id = self._instrument.id
        self.cancel_all_orders(instrument_id)
        self.close_all_positions(instrument_id)
        self.unsubscribe_quote_ticks(instrument_id)

    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Actions to be performed when a quote tick is received.
        """
        mid_f64 = (float(tick.bid_price) + float(tick.ask_price)) / 2.0
        mid = Price(mid_f64, self._price_precision)

        instrument_id = self.config.instrument_id

        # Always requote when the grid is empty, even if mid is within threshold
        has_resting = bool(
            self.cache.orders_open(instrument_id=instrument_id, strategy_id=self.id)
            or self.cache.orders_inflight(instrument_id=instrument_id, strategy_id=self.id),
        )
        if not self._should_requote(mid) and has_resting:
            return

        self.log.info(
            f"Requoting grid: mid={mid}, last_mid={self._last_quoted_mid}, "
            f"instrument={instrument_id}",
        )

        if self.config.on_cancel_resubmit:
            for order in (
                *self.cache.orders_open(instrument_id=instrument_id, strategy_id=self.id),
                *self.cache.orders_inflight(instrument_id=instrument_id, strategy_id=self.id),
            ):
                self._pending_self_cancels.add(order.client_order_id)

        self.cancel_all_orders(instrument_id)

        # Compute worst-case per-side exposure since cancels are async
        # and pending orders may still fill before the ack arrives
        net_position, worst_long, worst_short = self._compute_exposure(instrument_id)

        grid = self._grid_orders(mid, net_position, worst_long, worst_short)

        # Don't advance the requote anchor when no orders are placed,
        # otherwise the strategy can stall with zero resting orders
        if not grid:
            return

        expire_time = None
        tif = TimeInForce.GTC
        if self.config.expire_time_secs is not None:
            expire_time = self.clock.utc_now() + timedelta(seconds=self.config.expire_time_secs)
            tif = TimeInForce.GTD

        for side, price in grid:
            order = self.order_factory.limit(
                instrument_id=instrument_id,
                order_side=side,
                quantity=self._trade_size,
                price=price,
                time_in_force=tif,
                expire_time=expire_time,
                post_only=True,
            )
            self.submit_order(order)

        self._last_quoted_mid = mid

    def on_order_filled(self, event: OrderFilled) -> None:
        """
        Actions to be performed when an order is filled.
        """
        # Only remove from tracking once fully filled; for partial fills the ID must
        # remain so a subsequent self-cancel is not misclassified as external
        order = self.cache.order(event.client_order_id)
        if order is not None and order.is_closed:
            self._pending_self_cancels.discard(event.client_order_id)

    def on_order_rejected(self, event: OrderRejected) -> None:
        """
        Actions to be performed when an order is rejected.
        """
        self._pending_self_cancels.discard(event.client_order_id)
        # Reset so the next quote tick can retry placing the full grid
        self._last_quoted_mid = None

    def on_order_expired(self, event: OrderExpired) -> None:
        """
        Actions to be performed when an order expires.
        """
        self._pending_self_cancels.discard(event.client_order_id)
        # GTD expiry means the grid is gone; reset so re-quoting is not suppressed
        self._last_quoted_mid = None

    def on_order_canceled(self, event: OrderCanceled) -> None:
        """
        Actions to be performed when an order is canceled.
        """
        if event.client_order_id in self._pending_self_cancels:
            self._pending_self_cancels.discard(event.client_order_id)
            return

        if self.config.on_cancel_resubmit:
            # Reset so the next quote tick triggers a full grid resubmission
            self._last_quoted_mid = None

    def on_reset(self) -> None:
        """
        Actions to be performed when the strategy is reset.
        """
        self._instrument = None
        self._trade_size = self.config.trade_size
        self._price_precision = None
        self._last_quoted_mid = None
        self._pending_self_cancels.clear()

    def _should_requote(self, mid: Price) -> bool:
        if self._last_quoted_mid is None:
            return True
        last_f64 = float(self._last_quoted_mid)
        if last_f64 == 0.0:
            return True
        threshold = self.config.requote_threshold_bps / 10_000.0
        return abs(float(mid) - last_f64) / last_f64 >= threshold

    def _compute_exposure(
        self,
        instrument_id: InstrumentId,
    ) -> tuple[float, Decimal, Decimal]:
        net_qty = 0.0
        net_dec = Decimal(0)
        for pos in self.cache.positions_open(instrument_id=instrument_id, strategy_id=self.id):
            net_qty += pos.signed_qty
            qty_dec = Decimal(str(pos.quantity))
            net_dec += qty_dec if pos.signed_qty > 0 else -qty_dec

        pending_buy = Decimal(0)
        pending_sell = Decimal(0)
        seen: set[ClientOrderId] = set()
        for order in (
            *self.cache.orders_open(instrument_id=instrument_id, strategy_id=self.id),
            *self.cache.orders_inflight(instrument_id=instrument_id, strategy_id=self.id),
        ):
            if order.client_order_id in seen:
                continue
            seen.add(order.client_order_id)
            qty = Decimal(str(order.leaves_qty))
            if order.side == OrderSide.BUY:
                pending_buy += qty
            else:
                pending_sell += qty

        return net_qty, net_dec + pending_buy, net_dec - pending_sell

    def _grid_orders(
        self,
        mid: Price,
        net_position: float,
        worst_long: Decimal,
        worst_short: Decimal,
    ) -> list[tuple[OrderSide, Price]]:
        if self._instrument is None:
            return []
        mid_f64 = float(mid)
        skew_f64 = self.config.skew_factor * net_position
        pct = self.config.grid_step_bps / 10_000.0
        trade_size = Decimal(str(self._trade_size))
        max_pos = Decimal(str(self.config.max_position))
        projected_long = worst_long
        projected_short = worst_short
        orders = []
        price_inc = float(self._instrument.price_increment)

        for level in range(1, self.config.num_levels + 1):
            buy_f64 = mid_f64 * (1.0 - pct) ** level - skew_f64
            sell_f64 = mid_f64 * (1.0 + pct) ** level - skew_f64
            # next_bid_price floors to the nearest valid bid tick (<=buy_f64),
            # next_ask_price ceils to the nearest valid ask tick (>=sell_f64),
            # preventing self-cross on coarse-tick instruments.
            # Fall back to round_down/round_up when no tick scheme is configured.
            if self._instrument.tick_scheme_name is not None:
                buy_price = self._instrument.next_bid_price(buy_f64)
                sell_price = self._instrument.next_ask_price(sell_f64)
            else:
                buy_price = Price(round_down(buy_f64, price_inc), self._price_precision)
                sell_price = Price(round_up(sell_f64, price_inc), self._price_precision)
                min_px = self._instrument.min_price
                max_px = self._instrument.max_price
                if (min_px is not None and buy_price < min_px) or (
                    max_px is not None and buy_price > max_px
                ):
                    buy_price = None
                if (min_px is not None and sell_price < min_px) or (
                    max_px is not None and sell_price > max_px
                ):
                    sell_price = None

            if buy_price is not None and projected_long + trade_size <= max_pos:
                orders.append((OrderSide.BUY, buy_price))
                projected_long += trade_size

            if sell_price is not None and projected_short - trade_size >= -max_pos:
                orders.append((OrderSide.SELL, sell_price))
                projected_short -= trade_size

        return orders
