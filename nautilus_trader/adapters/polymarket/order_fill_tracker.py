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
Per-order fill tracking with dust detection for the Polymarket adapter.
"""

from __future__ import annotations

import logging
from collections import OrderedDict
from dataclasses import dataclass

from nautilus_trader.adapters.polymarket.common.constants import SNAP_OVERFILL_ULPS
from nautilus_trader.adapters.polymarket.common.constants import SNAP_UNDERFILL_ULPS
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


log = logging.getLogger(__name__)
CAPACITY = 10_000


@dataclass
class _OrderFillState:
    submitted_qty: Quantity
    cumulative_filled: float
    last_fill_px: float
    last_fill_ts: int
    order_side: OrderSide
    instrument_id: InstrumentId
    size_precision: int
    price_precision: int


class OrderFillTracker:
    """
    Tracks per-order fill accumulation and detects dust residuals.
    """

    def __init__(self) -> None:
        self._orders: OrderedDict[VenueOrderId, _OrderFillState] = OrderedDict()

    def register(
        self,
        venue_order_id: VenueOrderId,
        submitted_qty: Quantity,
        order_side: OrderSide,
        instrument_id: InstrumentId,
        size_precision: int,
        price_precision: int,
    ) -> None:
        """
        Register an order after HTTP accept.
        """
        state = _OrderFillState(
            submitted_qty=submitted_qty,
            cumulative_filled=0.0,
            last_fill_px=0.0,
            last_fill_ts=0,
            order_side=order_side,
            instrument_id=instrument_id,
            size_precision=size_precision,
            price_precision=price_precision,
        )
        self._orders[venue_order_id] = state
        # Evict oldest if over capacity
        while len(self._orders) > CAPACITY:
            self._orders.popitem(last=False)

    def contains(self, venue_order_id: VenueOrderId) -> bool:
        """
        Return true if the order has been registered.
        """
        return venue_order_id in self._orders

    def snap_fill_qty(self, venue_order_id: VenueOrderId, fill_qty: Quantity) -> Quantity:
        """
        Snap a single fill qty to ``submitted_qty`` when the diff is dust.

        See ``docs/integrations/polymarket.md`` (Fill quantity normalization).

        """
        state = self._orders.get(venue_order_id)
        if state is None:
            return fill_qty
        diff = float(state.submitted_qty) - float(fill_qty)
        ulp = 10 ** (-state.size_precision)
        if diff > 0.0:
            tolerance = SNAP_UNDERFILL_ULPS * ulp
        elif diff < 0.0:
            tolerance = SNAP_OVERFILL_ULPS * ulp
        else:
            return fill_qty
        if abs(diff) < tolerance:
            log.info(
                "Snapping fill qty %s -> %s (dust=%.6f)",
                fill_qty,
                state.submitted_qty,
                diff,
            )
            return state.submitted_qty
        return fill_qty

    def record_fill(
        self,
        venue_order_id: VenueOrderId,
        qty: float,
        px: float,
        ts: int,
    ) -> None:
        """
        Record a fill, updating cumulative total and last price/ts.
        """
        state = self._orders.get(venue_order_id)
        if state is not None:
            state.cumulative_filled += qty
            state.last_fill_px = px
            state.last_fill_ts = ts

    def check_dust_residual(
        self,
        venue_order_id: VenueOrderId,
    ) -> tuple[Quantity, Price] | None:
        """
        Check if an order has a dust residual after all fills.

        Returns (dust_qty, fill_px) if a synthetic fill should be emitted, or None
        otherwise. Removes the entry on dust settlement to prevent duplicate synthetic
        fills from repeated MATCHED events.

        """
        state = self._orders.get(venue_order_id)
        if state is None:
            return None
        leaves = float(state.submitted_qty) - state.cumulative_filled
        underfill_tolerance = SNAP_UNDERFILL_ULPS * (10 ** (-state.size_precision))
        if 0.0 < leaves < underfill_tolerance:
            log.info(
                "Order %s MATCHED with dust residual %.6f -- "
                "emitting synthetic fill to reach FILLED",
                venue_order_id,
                leaves,
            )
            dust_qty = Quantity(leaves, state.size_precision)
            px = max(0.0, state.last_fill_px)
            fill_px = Price(px, state.price_precision)
            # Remove entry — order is settled, prevents duplicate dust fills
            del self._orders[venue_order_id]
            return dust_qty, fill_px
        if leaves >= underfill_tolerance:
            log.info(
                "Order %s MATCHED with significant residual %.6f (filled %s/%s)",
                venue_order_id,
                leaves,
                state.cumulative_filled,
                state.submitted_qty,
            )
        return None
