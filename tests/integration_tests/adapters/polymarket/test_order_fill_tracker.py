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

import pytest

from nautilus_trader.adapters.polymarket.order_fill_tracker import CAPACITY
from nautilus_trader.adapters.polymarket.order_fill_tracker import OrderFillTracker
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


INSTRUMENT_ID = InstrumentId.from_str("TEST.POLYMARKET")
SIZE_PRECISION = 6
PRICE_PRECISION = 2


@pytest.fixture
def tracker():
    return OrderFillTracker()


class TestOrderFillTracker:
    def test_register_and_contains(self, tracker):
        vid = VenueOrderId("order-1")
        assert not tracker.contains(vid)

        tracker.register(
            venue_order_id=vid,
            submitted_qty=Quantity(100.0, SIZE_PRECISION),
            order_side=OrderSide.BUY,
            instrument_id=INSTRUMENT_ID,
            size_precision=SIZE_PRECISION,
            price_precision=PRICE_PRECISION,
        )
        assert tracker.contains(vid)

    def test_snap_fill_qty_dust(self, tracker):
        vid = VenueOrderId("order-1")
        tracker.register(
            venue_order_id=vid,
            submitted_qty=Quantity(23.696681, SIZE_PRECISION),
            order_side=OrderSide.SELL,
            instrument_id=INSTRUMENT_ID,
            size_precision=SIZE_PRECISION,
            price_precision=PRICE_PRECISION,
        )

        # Fill is 23.69 (truncated by CLOB), diff = 0.006681 < 0.01 -> snap
        fill_qty = Quantity(23.69, SIZE_PRECISION)
        snapped = tracker.snap_fill_qty(vid, fill_qty)
        assert snapped == Quantity(23.696681, SIZE_PRECISION)

    def test_snap_fill_qty_no_snap_large_diff(self, tracker):
        vid = VenueOrderId("order-1")
        tracker.register(
            venue_order_id=vid,
            submitted_qty=Quantity(100.0, SIZE_PRECISION),
            order_side=OrderSide.BUY,
            instrument_id=INSTRUMENT_ID,
            size_precision=SIZE_PRECISION,
            price_precision=PRICE_PRECISION,
        )

        # Fill is 50.0, diff = 50 >> 0.01 -> no snap
        fill_qty = Quantity(50.0, SIZE_PRECISION)
        result = tracker.snap_fill_qty(vid, fill_qty)
        assert result == fill_qty

    def test_snap_fill_qty_unregistered(self, tracker):
        vid = VenueOrderId("unknown")
        fill_qty = Quantity(50.0, SIZE_PRECISION)
        result = tracker.snap_fill_qty(vid, fill_qty)
        assert result == fill_qty

    def test_record_fill_accumulates(self, tracker):
        vid = VenueOrderId("order-1")
        tracker.register(
            venue_order_id=vid,
            submitted_qty=Quantity(100.0, SIZE_PRECISION),
            order_side=OrderSide.BUY,
            instrument_id=INSTRUMENT_ID,
            size_precision=SIZE_PRECISION,
            price_precision=PRICE_PRECISION,
        )

        tracker.record_fill(vid, 50.0, 0.55, 1000)
        tracker.record_fill(vid, 49.997714, 0.55, 2000)

        # Dust check: 100.0 - 99.997714 = 0.002286 < 0.01 -> emit
        result = tracker.check_dust_residual(vid)
        assert result is not None
        dust_qty, dust_px = result
        assert abs(float(dust_qty) - 0.002286) < 1e-9
        assert dust_px == Price(0.55, PRICE_PRECISION)

    def test_check_dust_no_residual(self, tracker):
        vid = VenueOrderId("order-1")
        tracker.register(
            venue_order_id=vid,
            submitted_qty=Quantity(100.0, SIZE_PRECISION),
            order_side=OrderSide.BUY,
            instrument_id=INSTRUMENT_ID,
            size_precision=SIZE_PRECISION,
            price_precision=PRICE_PRECISION,
        )

        # Exact fill
        tracker.record_fill(vid, 100.0, 0.55, 1000)
        result = tracker.check_dust_residual(vid)
        assert result is None

    def test_check_dust_significant_residual(self, tracker):
        vid = VenueOrderId("order-1")
        tracker.register(
            venue_order_id=vid,
            submitted_qty=Quantity(100.0, SIZE_PRECISION),
            order_side=OrderSide.BUY,
            instrument_id=INSTRUMENT_ID,
            size_precision=SIZE_PRECISION,
            price_precision=PRICE_PRECISION,
        )

        # Only half filled — residual = 50 >> 0.01
        tracker.record_fill(vid, 50.0, 0.55, 1000)
        result = tracker.check_dust_residual(vid)
        assert result is None

    def test_check_dust_unregistered(self, tracker):
        vid = VenueOrderId("unknown")
        result = tracker.check_dust_residual(vid)
        assert result is None

    def test_dust_uses_last_fill_price(self, tracker):
        vid = VenueOrderId("order-1")
        tracker.register(
            venue_order_id=vid,
            submitted_qty=Quantity(100.0, SIZE_PRECISION),
            order_side=OrderSide.BUY,
            instrument_id=INSTRUMENT_ID,
            size_precision=SIZE_PRECISION,
            price_precision=PRICE_PRECISION,
        )

        tracker.record_fill(vid, 99.995, 0.60, 1000)

        result = tracker.check_dust_residual(vid)
        assert result is not None
        dust_qty, dust_px = result
        # Should use last fill price (0.60), not default
        assert dust_px == Price(0.60, PRICE_PRECISION)

    def test_dust_settlement_removes_entry(self, tracker):
        vid = VenueOrderId("order-1")
        tracker.register(
            venue_order_id=vid,
            submitted_qty=Quantity(100.0, SIZE_PRECISION),
            order_side=OrderSide.BUY,
            instrument_id=INSTRUMENT_ID,
            size_precision=SIZE_PRECISION,
            price_precision=PRICE_PRECISION,
        )

        tracker.record_fill(vid, 99.995, 0.55, 1000)

        # First check returns dust
        result = tracker.check_dust_residual(vid)
        assert result is not None

        # Entry should be removed — second check returns None (no duplicate)
        assert not tracker.contains(vid)
        result2 = tracker.check_dust_residual(vid)
        assert result2 is None

    def test_capacity_eviction(self, tracker):
        # Insert CAPACITY + 1 orders
        for i in range(CAPACITY + 1):
            vid = VenueOrderId(f"order-{i}")
            tracker.register(
                venue_order_id=vid,
                submitted_qty=Quantity(100.0, SIZE_PRECISION),
                order_side=OrderSide.BUY,
                instrument_id=INSTRUMENT_ID,
                size_precision=SIZE_PRECISION,
                price_precision=PRICE_PRECISION,
            )

        # Oldest should be evicted
        assert not tracker.contains(VenueOrderId("order-0"))
        # Latest should still be present
        assert tracker.contains(VenueOrderId(f"order-{CAPACITY}"))
