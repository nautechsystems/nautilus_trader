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

    # Tolerances at size_precision=6:
    #   SNAP_UNDERFILL_ULPS = 10_000 -> 0.01
    #   SNAP_OVERFILL_ULPS  = 100    -> 0.0001
    @pytest.mark.parametrize(
        ("submitted", "fill", "expected"),
        [
            # Underfill well within tolerance: CLOB truncated the fill to a
            # cent tick, snap UP so the order can reach FILLED cleanly.
            pytest.param(23.696681, 23.690000, 23.696681, id="underfill_dust"),
            # Underfill near tolerance (0.0099 < 0.01): still snaps.
            pytest.param(100.000000, 99.990100, 100.000000, id="underfill_near_tolerance"),
            # Underfill at exactly the tolerance must NOT snap.
            pytest.param(100.000000, 99.990000, 99.990000, id="underfill_at_tolerance"),
            # Underfill above the tolerance: no snap.
            pytest.param(100.000000, 99.980000, 99.980000, id="underfill_above_tolerance"),
            # Underfill far past tolerance: leave fill alone.
            pytest.param(100.000000, 50.000000, 50.000000, id="large_underfill"),
            # Overfill within the tighter tolerance: V2 market BUY where the
            # SDK truncates registered qty to USDC scale but the on-chain
            # fill comes back at full precision. Observed drift is 4 ulps
            # (4e-6 at size_precision=6). Snap DOWN so the engine does not
            # reject as overfill.
            pytest.param(714.285710, 714.285714, 714.285710, id="overfill_dust"),
            # Overfill near the overfill tolerance (0.000099 < 0.0001):
            # still snaps.
            pytest.param(100.000000, 100.000099, 100.000000, id="overfill_near_tolerance"),
            # Overfill at exactly the overfill tolerance must NOT snap.
            pytest.param(100.000000, 100.000100, 100.000100, id="overfill_at_tolerance"),
            # Overfill above the overfill tolerance but below the underfill
            # tolerance must NOT snap. This is the asymmetry: a 0.005
            # overfill is suspicious and surfaces as an engine-side error
            # rather than being silently absorbed.
            pytest.param(100.000000, 100.005000, 100.005000, id="overfill_above_tolerance"),
            # Overfill far past tolerance: leave fill alone.
            pytest.param(100.000000, 150.000000, 150.000000, id="large_overfill"),
            # Exact match: no-op (returns the fill qty, which equals submitted).
            pytest.param(100.000000, 100.000000, 100.000000, id="exact"),
        ],
    )
    def test_snap_fill_qty(self, tracker, submitted, fill, expected):
        venue_order_id = VenueOrderId("order-1")
        tracker.register(
            venue_order_id=venue_order_id,
            submitted_qty=Quantity(submitted, SIZE_PRECISION),
            order_side=OrderSide.BUY,
            instrument_id=INSTRUMENT_ID,
            size_precision=SIZE_PRECISION,
            price_precision=PRICE_PRECISION,
        )

        snapped = tracker.snap_fill_qty(venue_order_id, Quantity(fill, SIZE_PRECISION))
        assert snapped == Quantity(expected, SIZE_PRECISION)

    # Tolerances scale with size_precision: at size_precision=3 the
    # underfill tolerance becomes 10 (10_000 * 1e-3) and the overfill
    # tolerance becomes 0.1 (100 * 1e-3). This case verifies the scaling.
    @pytest.mark.parametrize(
        ("submitted", "fill", "expected"),
        [
            # Underfill within scaled tolerance (5 < 10): snap.
            pytest.param(100.000, 95.000, 100.000, id="underfill_scaled_within"),
            # Underfill above scaled tolerance (15 > 10): no snap.
            pytest.param(100.000, 85.000, 85.000, id="underfill_scaled_above"),
            # Overfill within scaled tolerance (0.05 < 0.1): snap.
            pytest.param(100.000, 100.050, 100.000, id="overfill_scaled_within"),
            # Overfill above scaled tolerance (0.2 > 0.1): no snap.
            pytest.param(100.000, 100.200, 100.200, id="overfill_scaled_above"),
        ],
    )
    def test_snap_fill_qty_scales_with_precision(self, tracker, submitted, fill, expected):
        precision = 3
        venue_order_id = VenueOrderId("order-1")
        tracker.register(
            venue_order_id=venue_order_id,
            submitted_qty=Quantity(submitted, precision),
            order_side=OrderSide.BUY,
            instrument_id=INSTRUMENT_ID,
            size_precision=precision,
            price_precision=PRICE_PRECISION,
        )

        snapped = tracker.snap_fill_qty(venue_order_id, Quantity(fill, precision))
        assert snapped == Quantity(expected, precision)

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
