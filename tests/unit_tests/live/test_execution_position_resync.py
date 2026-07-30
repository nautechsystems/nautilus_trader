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
Magnitude-tiered position resync traces.

The policy under test: when missing-fill reconciliation succeeds (or finds nothing) but
the cached position still diverges from the venue report, a divergence whose notional is
at or below ``position_resync_auto_max_notional_usd`` is auto-resynced by snapping the
cache to venue truth, while a divergence above it (or one the engine cannot currently
price, or one on a book that keeps needing snaps) halts and alerts via
``_alert_position_resync_halt`` instead of moving the book. ``None`` (the default)
disables tiering entirely and preserves legacy behavior.
"""

from collections import deque
from decimal import Decimal

import pytest

from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.config import LiveExecEngineConfig
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.risk_engine import LiveRiskEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.mocks.exec_clients import MockLiveExecutionClient
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


SIM = Venue("SIM")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestPositionResyncTiers:
    @pytest.fixture(autouse=True)
    def setup(self, request):
        # Fixture Setup
        self.loop = request.getfixturevalue("event_loop")
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.account_id = TestIdStubs.account_id()
        self.trader_id = TestIdStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.data_engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=LiveExecEngineConfig(
                reconciliation=True,
                # Generous default: AUD/USD divergences in these traces are ~1000
                # notional, far below this; individual tests tighten or clear it.
                position_resync_auto_max_notional_usd=100_000.0,
            ),
        )

        self.risk_engine = LiveRiskEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.client = MockLiveExecutionClient(
            loop=self.loop,
            client_id=ClientId(SIM.value),
            venue=SIM,
            account_type=AccountType.CASH,
            base_currency=USD,
            instrument_provider=InstrumentProvider(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        self.portfolio.update_account(TestEventStubs.cash_account_state())
        self.exec_engine.register_client(self.client)

        self.cache.add_instrument(AUDUSD_SIM)
        self.key = (AUDUSD_SIM.id, self.account_id)

        # No missing fills at the venue, and the query SUCCEEDS
        async def query_ok(instrument_id, clients):
            return [], False

        self.exec_engine._query_and_find_missing_fills = query_ok

        # Spy on snaps: every synthetic venue-truth snap goes through
        # _reconcile_position_report
        self.snap_calls = []
        original_reconcile = self.exec_engine._reconcile_position_report

        def spy_reconcile(report):
            self.snap_calls.append(report)
            return original_reconcile(report)

        self.exec_engine._reconcile_position_report = spy_reconcile

        # Spy on the halt-and-alert channel
        self.halt_alerts = []
        original_alert = self.exec_engine._alert_position_resync_halt

        def spy_alert(**kwargs):
            self.halt_alerts.append(kwargs)
            return original_alert(**kwargs)

        self.exec_engine._alert_position_resync_halt = spy_alert

    def venue_report(self, qty: int, avg_px: Decimal | None = Decimal("1.00000")):
        return PositionStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(qty),
            avg_px_open=avg_px,
            report_id=UUID4(),
            ts_last=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )

    def add_quote(self, age_secs: float = 0.0):
        # Stamp the tick at (now - age_secs): the pricing tick-age cap halts on
        # anything older than 60s, so a default stub tick (ts_event=0) would
        # read as decades stale.
        ts_event = self.clock.timestamp_ns() - int(age_secs * 1_000_000_000)
        self.cache.add_quote_tick(
            TestDataStubs.quote_tick(instrument=AUDUSD_SIM, ts_event=ts_event),
        )

    def cached_qty(self) -> Decimal:
        positions = self.cache.positions_open(
            instrument_id=AUDUSD_SIM.id,
            account_id=self.account_id,
        )
        return sum((p.signed_decimal_qty() for p in positions), Decimal(0))

    # -- Flat path (cache flat, venue long) -------------------------------------------------------

    @pytest.mark.asyncio
    async def test_flat_venue_long_divergence_self_heals_within_one_interval_of_detection(self):
        """
        Pass 1 observes the divergence and defers (in-flight-fill guard); pass 2 snaps
        the cache to venue truth (auto tier) and clears the retry counter.

        Without tiering this path never snaps: a cache that missed a fill stays wedged
        until restart on a divergence a resync would have fixed within one check
        interval.
        """
        # Arrange
        self.add_quote()
        report = self.venue_report(1000)

        # Act - pass 1: first observation of the divergence
        await self.exec_engine._process_venue_reported_positions({}, {self.key: report})

        # Assert - first-pass defer: NO snap, retry accounting only
        assert not self.snap_calls
        assert self.cached_qty() == 0
        assert self.exec_engine._position_recon_retries[self.key] == 1

        # Act - pass 2: divergence has survived a full prior check cycle
        await self.exec_engine._process_venue_reported_positions({}, {self.key: report})

        # Assert - snapped to venue truth, retries cleared, snap recorded for the breaker
        assert len(self.snap_calls) == 1
        assert self.cached_qty() == Decimal(1000)
        assert self.key not in self.exec_engine._position_recon_retries
        assert len(self.exec_engine._position_resync_snap_ts[self.key]) == 1
        assert not self.halt_alerts

    @pytest.mark.asyncio
    async def test_oversized_divergence_halts_and_alerts_instead_of_snapping(self):
        """
        A divergence whose notional exceeds the tier threshold must never be snapped:
        the engine halts and alerts (the halt-and-alert channel).
        """
        # Arrange - 1000 AUD/USD at ~1.0 is 1000 notional, over a 50 threshold
        self.add_quote()
        self.exec_engine.position_resync_auto_max_notional_usd = 50.0
        report = self.venue_report(1000)

        # Act - pass 1 defers (no decision at all), passes 2+ decide "halt"
        await self.exec_engine._process_venue_reported_positions({}, {self.key: report})
        assert not self.halt_alerts  # First pass only defers

        await self.exec_engine._process_venue_reported_positions({}, {self.key: report})
        await self.exec_engine._process_venue_reported_positions({}, {self.key: report})

        # Assert - no snap on any pass, alert raised with the divergence numbers
        assert not self.snap_calls
        assert self.cached_qty() == 0
        assert len(self.halt_alerts) == 2
        alert = self.halt_alerts[0]
        assert alert["instrument_id"] == AUDUSD_SIM.id
        assert alert["account_id"] == self.account_id
        assert alert["cached_qty"] == Decimal(0)
        assert alert["venue_qty"] == Decimal(1000)

    @pytest.mark.asyncio
    async def test_inflight_fill_interleaving_never_double_counts(self):
        """
        A venue-booked fill visible in the position report but not yet booked locally
        must NOT trigger a snap on first observation: the real fill books during the
        deferred interval and the cache converges to venue truth EXACTLY — never 2x
        via a synthetic reconciliation fill on top of the real one.
        """
        # Arrange
        self.add_quote()
        report = self.venue_report(1000)

        # Act - pass 1: venue sees the position, the WS fill is still in flight
        await self.exec_engine._process_venue_reported_positions({}, {self.key: report})

        # Assert - deferred, no snap
        assert not self.snap_calls
        assert self.cached_qty() == 0

        # The real fill now lands through the engine's event handling path
        order = TestExecStubs.limit_order(
            instrument=AUDUSD_SIM,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(1000),
        )
        self.cache.add_order(order, position_id=None)
        order.apply(TestEventStubs.order_submitted(order, account_id=self.account_id))
        self.cache.update_order(order)
        order.apply(TestEventStubs.order_accepted(order, account_id=self.account_id))
        self.cache.update_order(order)
        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            account_id=self.account_id,
            position_id=PositionId("P-INFLIGHT"),
            last_qty=Quantity.from_int(1000),
            last_px=Price.from_str("1.00000"),
        )
        self.exec_engine._handle_event_with_tracking(fill)
        assert self.cached_qty() == Decimal(1000)

        # Act - pass 2: the key now carries a cached position, so a full check pass
        # routes it to the cached-discrepancies branch and the venue branch skips it
        positions = self.cache.positions_open(
            instrument_id=AUDUSD_SIM.id,
            account_id=self.account_id,
        )
        await self.exec_engine._process_cached_position_discrepancies(
            {self.key: positions},
            {self.key: report},
        )
        await self.exec_engine._process_venue_reported_positions(
            {self.key: positions},
            {self.key: report},
        )

        # Assert - converged EXACTLY (never 2x), no synthetic snap ever happened
        assert self.cached_qty() == Decimal(1000)
        assert not self.snap_calls
        assert self.key not in self.exec_engine._position_recon_retries
        assert not self.halt_alerts

    @pytest.mark.asyncio
    async def test_snap_count_breaker_halts_repeated_resyncs(self):
        """
        A book already at the snap limit inside the rolling window halts regardless of
        magnitude: repeated snaps are the signature of a broken feed, and auto-snapping
        onto a broken feed just chases it.
        """
        # Arrange - small notional (auto tier territory) but breaker already at limit
        self.add_quote()
        ts_now = self.clock.timestamp_ns()
        limit = self.exec_engine.position_resync_snap_limit
        self.exec_engine._position_resync_snap_ts[self.key] = deque([ts_now] * limit)
        report = self.venue_report(1000)

        # Act - pass 1 defers, pass 2 decides
        await self.exec_engine._process_venue_reported_positions({}, {self.key: report})
        await self.exec_engine._process_venue_reported_positions({}, {self.key: report})

        # Assert - halted: no snap, alert raised
        assert not self.snap_calls
        assert self.cached_qty() == 0
        assert len(self.halt_alerts) == 1
        assert self.halt_alerts[0]["venue_qty"] == Decimal(1000)

    @pytest.mark.asyncio
    async def test_no_market_data_halts_instead_of_snapping(self):
        """
        With no quote or trade tick in cache the divergence cannot be priced: never
        auto-snap a book without market data — halt conservatively.
        """
        # Arrange - NO quote or trade tick added
        assert self.cache.quote_tick(AUDUSD_SIM.id) is None
        assert self.cache.trade_tick(AUDUSD_SIM.id) is None
        report = self.venue_report(1000)

        # Act
        await self.exec_engine._process_venue_reported_positions({}, {self.key: report})
        await self.exec_engine._process_venue_reported_positions({}, {self.key: report})

        # Assert - halted: no snap, alert raised
        assert not self.snap_calls
        assert self.cached_qty() == 0
        assert len(self.halt_alerts) == 1

    @pytest.mark.asyncio
    async def test_stale_pricing_tick_halts_instead_of_snapping(self):
        """
        A tick older than the 60s cap must not price an auto-snap: never auto-snap a
        book you cannot CURRENTLY price — a dead feed is exactly the broken-feed case
        the breaker exists for.
        """
        # Arrange - generous threshold, but the only tick is 61s old
        self.add_quote(age_secs=61.0)
        report = self.venue_report(1000)

        # Act - pass 1 defers, pass 2 decides
        await self.exec_engine._process_venue_reported_positions({}, {self.key: report})
        await self.exec_engine._process_venue_reported_positions({}, {self.key: report})

        # Assert - halted: no snap, alert raised
        assert not self.snap_calls
        assert self.cached_qty() == 0
        assert len(self.halt_alerts) == 1

        # A fresh tick on the same book prices the divergence and the snap proceeds
        self.add_quote()
        await self.exec_engine._process_venue_reported_positions({}, {self.key: report})
        assert len(self.snap_calls) == 1
        assert self.cached_qty() == Decimal(1000)

    @pytest.mark.asyncio
    async def test_threshold_none_preserves_legacy_behavior(self):
        """
        ``position_resync_auto_max_notional_usd=None`` disables tiering: the flat path
        never snaps and never alerts from the tier block — identical to before the
        option existed. This is what makes the feature a pure opt-in.
        """
        # Arrange
        assert LiveExecEngineConfig().position_resync_auto_max_notional_usd is None  # Default off
        self.add_quote()
        self.exec_engine.position_resync_auto_max_notional_usd = None
        report = self.venue_report(1000)

        # Act - run past the retry budget
        for _ in range(self.exec_engine.position_check_retries + 1):
            await self.exec_engine._process_venue_reported_positions({}, {self.key: report})

        # Assert - never snapped, never alerted, legacy retry latch reached
        assert not self.snap_calls
        assert self.cached_qty() == 0
        assert not self.halt_alerts
        assert not self.exec_engine._position_resync_snap_ts
        assert (
            self.exec_engine._position_recon_retries[self.key]
            == self.exec_engine.position_check_retries
        )

    # -- Cached-positions path (existing snap, now tier-gated) ------------------------------------

    def _add_cached_long(self, qty: int) -> Position:
        order = TestExecStubs.limit_order(
            instrument=AUDUSD_SIM,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(qty),
        )
        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            account_id=self.account_id,
            position_id=PositionId("P-CACHED"),
            last_qty=Quantity.from_int(qty),
            last_px=Price.from_str("1.00000"),
        )
        position = Position(instrument=AUDUSD_SIM, fill=fill)
        self.cache.add_position(position, OmsType.NETTING)
        return position

    @pytest.mark.asyncio
    async def test_cached_path_snap_is_tier_gated_when_threshold_set(self):
        """
        The cached-positions path's EXISTING snap is tier-gated: an over-threshold
        divergence skips the snap (the book does not move) and alerts on that pass,
        rather than waiting for the retry counter to exhaust.
        """
        # Arrange - cache long 1000, venue reports 400: divergence 600 over a 50 threshold
        self.add_quote()
        self.exec_engine.position_resync_auto_max_notional_usd = 50.0
        position = self._add_cached_long(1000)
        report = self.venue_report(400)

        # Act - no first-pass defer on this path: the legacy snap timing is preserved
        await self.exec_engine._process_cached_position_discrepancies(
            {self.key: [position]},
            {self.key: report},
        )

        # Assert - snap skipped, book unmoved, alert raised
        assert not self.snap_calls
        assert self.cached_qty() == Decimal(1000)
        assert len(self.halt_alerts) == 1
        assert self.halt_alerts[0]["cached_qty"] == Decimal(1000)
        assert self.halt_alerts[0]["venue_qty"] == Decimal(400)
        assert self.exec_engine._position_recon_retries[self.key] == 1

        # And an under-threshold divergence on the same path snaps and records it
        self.exec_engine.position_resync_auto_max_notional_usd = 100_000.0
        await self.exec_engine._process_cached_position_discrepancies(
            {self.key: [position]},
            {self.key: report},
        )
        assert len(self.snap_calls) == 1
        assert self.cached_qty() == Decimal(400)
        assert len(self.exec_engine._position_resync_snap_ts[self.key]) == 1
        assert self.key not in self.exec_engine._position_recon_retries

    @pytest.mark.asyncio
    async def test_cached_path_snap_unconditional_when_threshold_none(self):
        """
        With the threshold ``None`` the cached-positions path keeps its legacy
        unconditional snap — same divergence the halt case above refused to touch.
        """
        # Arrange
        self.add_quote()
        self.exec_engine.position_resync_auto_max_notional_usd = None
        position = self._add_cached_long(1000)
        report = self.venue_report(400)

        # Act
        await self.exec_engine._process_cached_position_discrepancies(
            {self.key: [position]},
            {self.key: report},
        )

        # Assert - legacy snap happened, and "off" recorded nothing for the breaker
        assert len(self.snap_calls) == 1
        assert self.cached_qty() == Decimal(400)
        assert not self.halt_alerts
        assert not self.exec_engine._position_resync_snap_ts
        assert self.key not in self.exec_engine._position_recon_retries

    # -- Live-fill activity stamp at receipt ------------------------------------------------------

    def test_live_fill_receipt_stamps_position_activity(self):
        """
        ``process()`` must stamp position-level activity at RECEIPT time for an
        ``OrderFilled``: the position-check threshold gate reads this map, and without
        the stamp a live WS fill is invisible to the gate until the event queue drains.
        """
        # Arrange
        order = TestExecStubs.limit_order(
            instrument=AUDUSD_SIM,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(1000),
        )
        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            account_id=self.account_id,
            position_id=PositionId("P-STAMP"),
            last_qty=Quantity.from_int(1000),
            last_px=Price.from_str("1.00000"),
        )
        assert self.key not in self.exec_engine._position_local_activity_ns

        # Act - enqueue only; the event queue is NOT running
        before_ns = self.clock.timestamp_ns()
        self.exec_engine.process(fill)

        # Assert - stamped at receipt (clock) time, before any dequeue/handling
        stamped_ns = self.exec_engine._position_local_activity_ns.get(self.key)
        assert stamped_ns is not None
        assert stamped_ns >= before_ns
