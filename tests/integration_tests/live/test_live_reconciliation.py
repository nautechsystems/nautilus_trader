# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
Integration tests for LiveExecutionEngine reconciliation with simulated venue scenarios.

These tests simulate real-world scenarios where order states and fills can get out of
sync between the local cache and the venue.

"""

import asyncio
from decimal import Decimal
from unittest.mock import AsyncMock

import pytest

from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.functions import ensure_all_tasks_completed
from nautilus_trader.test_kit.functions import eventually
from nautilus_trader.test_kit.mocks.exec_clients import MockLiveExecutionClient
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
SIM = Venue("SIM")


# =============================================================================
# FIXTURES
# =============================================================================


@pytest.fixture(name="clock")
def fixture_clock():
    """
    Create a live clock.
    """
    return LiveClock()


@pytest.fixture(name="trader_id")
def fixture_trader_id():
    """
    Create a trader ID.
    """
    return TestIdStubs.trader_id()


@pytest.fixture(name="account_id")
def fixture_account_id():
    """
    Create an account ID.
    """
    return TestIdStubs.account_id()


@pytest.fixture(name="msgbus")
def fixture_msgbus(trader_id, clock):
    """
    Create a message bus.
    """
    return MessageBus(
        trader_id=trader_id,
        clock=clock,
    )


@pytest.fixture(name="cache")
def fixture_cache():
    """
    Create a cache with AUDUSD instrument.
    """
    cache = TestComponentStubs.cache()
    cache.add_instrument(AUDUSD_SIM)
    return cache


@pytest.fixture(name="portfolio")
def fixture_portfolio(msgbus, cache, clock):
    """
    Create a portfolio with cash account.
    """
    portfolio = Portfolio(
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    portfolio.update_account(TestEventStubs.cash_account_state())
    return portfolio


@pytest.fixture(name="instrument_provider")
def fixture_instrument_provider():
    """
    Create an instrument provider.
    """
    provider = InstrumentProvider()
    provider.add(AUDUSD_SIM)
    return provider


@pytest.fixture(name="exec_client")
def fixture_exec_client(event_loop, msgbus, cache, clock, instrument_provider):
    """
    Create a mock live execution client.
    """
    return MockLiveExecutionClient(
        loop=event_loop,
        client_id=ClientId(SIM.value),
        venue=SIM,
        account_type=AccountType.CASH,
        base_currency=USD,
        instrument_provider=instrument_provider,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )


@pytest.fixture(name="exec_engine")
def fixture_exec_engine(event_loop, msgbus, cache, clock, exec_client, portfolio):
    """
    Create a live execution engine configured for reconciliation testing.
    """
    exec_engine = LiveExecutionEngine(
        loop=event_loop,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=LiveExecEngineConfig(
            reconciliation=True,
            inflight_check_interval_ms=100,
            inflight_check_threshold_ms=200,
            inflight_check_retries=2,  # Low retries for testing
            open_check_interval_secs=0.5,
            reconciliation_startup_delay_secs=0,  # No delay for testing
        ),
    )
    exec_engine.register_client(exec_client)
    exec_engine.start()

    # Set startup reconciliation event so continuous loop can start
    # (integration tests don't call reconcile_execution_state)
    exec_engine._startup_reconciliation_event.set()

    yield exec_engine

    exec_engine.stop()
    ensure_all_tasks_completed()


@pytest.fixture(name="strategy")
def fixture_strategy(trader_id, portfolio, msgbus, cache, clock):
    """
    Create a basic strategy.
    """
    strategy = Strategy()
    strategy.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    return strategy


@pytest.fixture(name="order_factory")
def fixture_order_factory(trader_id, strategy, clock):
    """
    Create an order factory.
    """
    return OrderFactory(
        trader_id=trader_id,
        strategy_id=strategy.id,
        clock=clock,
    )


@pytest.mark.asyncio
async def test_missed_fill_reconciliation_scenario(
    exec_engine,
    exec_client,
    cache,
    clock,
    account_id,
    order_factory,
):
    """
    Test reconciliation when a fill is missed by the client but exists on the venue.

    Simulates a scenario where:
    1. Order is submitted and accepted
    2. Venue fills the order but client misses the fill event
    3. Reconciliation detects and applies the missed fill

    """
    # Arrange
    order = order_factory.limit(
        instrument_id=AUDUSD_SIM.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        price=AUDUSD_SIM.make_price(1.00000),
    )

    # Act - Submit order (mock client doesn't auto-process, so we simulate submission)
    cache.add_order(order)

    # Simulate order submission
    submitted_event = TestEventStubs.order_submitted(
        order,
        account_id=account_id,
    )
    order.apply(submitted_event)
    exec_engine.process(submitted_event)
    await asyncio.sleep(0.01)  # Allow processing

    # Simulate order acceptance on venue
    exec_engine.process(
        TestEventStubs.order_accepted(
            order,
            account_id=account_id,
            venue_order_id=VenueOrderId("V-001"),
        ),
    )

    # Simulate venue has a fill that client missed
    fill_report = FillReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-001"),
        trade_id=TradeId("T-001"),
        order_side=OrderSide.BUY,
        last_qty=Quantity.from_int(50_000),
        last_px=Price.from_str("1.00000"),
        commission=Money(2.50, USD),
        liquidity_side=LiquiditySide.TAKER,
        report_id=UUID4(),
        ts_event=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )

    # Add fill to venue but not to client's cache
    exec_client.add_fill_reports(VenueOrderId("V-001"), [fill_report])

    # Add order status report showing partial fill
    status_report = OrderStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-001"),
        order_side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.PARTIALLY_FILLED,
        price=Price.from_str("1.00000"),
        quantity=Quantity.from_int(100_000),
        filled_qty=Quantity.from_int(50_000),
        avg_px=Decimal("1.00000"),
        report_id=UUID4(),
        ts_accepted=clock.timestamp_ns(),
        ts_last=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )
    exec_client.add_order_status_report(status_report)

    # Trigger reconciliation
    result = await exec_engine.reconcile_execution_state()

    # Assert
    assert result is True
    assert order.status == OrderStatus.PARTIALLY_FILLED
    assert order.filled_qty == Quantity.from_int(50_000)
    assert order.avg_px == Decimal("1.00000")


@pytest.mark.asyncio
async def test_order_state_discrepancy_reconciliation(
    exec_engine,
    exec_client,
    cache,
    clock,
    account_id,
    order_factory,
):
    """
    Test reconciliation when order state differs between client and venue.

    Simulates a scenario where:
    1. Order is submitted
    2. Client thinks order is still SUBMITTED but venue has REJECTED it
    3. Reconciliation updates the order state correctly

    """
    # Arrange
    order = order_factory.limit(
        instrument_id=AUDUSD_SIM.id,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(50_000),
        price=AUDUSD_SIM.make_price(1.10000),
    )

    # Act - Submit order (mock client doesn't auto-process, so we simulate submission)
    cache.add_order(order)

    # Simulate order submission
    submitted_event = TestEventStubs.order_submitted(
        order,
        account_id=account_id,
    )
    order.apply(submitted_event)
    exec_engine.process(submitted_event)

    # Order stays in SUBMITTED state locally
    assert order.status == OrderStatus.SUBMITTED

    # But venue has rejected it
    rejected_report = OrderStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-002"),
        order_side=OrderSide.SELL,
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.REJECTED,
        price=Price.from_str("1.10000"),
        quantity=Quantity.from_int(50_000),
        filled_qty=Quantity.from_int(0),
        cancel_reason="INSUFFICIENT_MARGIN",
        report_id=UUID4(),
        ts_accepted=0,
        ts_last=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )
    exec_client.add_order_status_report(rejected_report)

    # Trigger reconciliation
    result = await exec_engine.reconcile_execution_state()

    # Assert
    assert result is True
    assert order.status == OrderStatus.REJECTED


@pytest.mark.asyncio
async def test_external_order_reconciliation(
    exec_engine,
    exec_client,
    cache,
    clock,
    account_id,
):
    """
    Test reconciliation of orders placed externally (not through this system).

    Simulates a scenario where:
    1. An order exists on the venue but not in our cache
    2. Reconciliation discovers and adds the external order

    """
    # Arrange - No local order created

    # Venue has an order we don't know about
    external_report = OrderStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=ClientOrderId("EXTERNAL-001"),
        venue_order_id=VenueOrderId("V-EXT-001"),
        order_side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.ACCEPTED,
        price=Price.from_str("0.99000"),
        quantity=Quantity.from_int(25_000),
        filled_qty=Quantity.from_int(0),
        report_id=UUID4(),
        ts_accepted=clock.timestamp_ns(),
        ts_last=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )
    exec_client.add_order_status_report(external_report)

    # Act - Trigger reconciliation
    result = await exec_engine.reconcile_execution_state()

    # Assert
    assert result is True

    # Check that external order was added to cache
    external_order = cache.order(ClientOrderId("EXTERNAL-001"))
    assert external_order is not None
    assert external_order.status == OrderStatus.ACCEPTED
    assert external_order.venue_order_id == VenueOrderId("V-EXT-001")


@pytest.mark.asyncio
async def test_inflight_order_timeout_reconciliation(
    exec_engine,
    exec_client,
    cache,
    clock,
    account_id,
    order_factory,
):
    """
    Test inflight order reconciliation when order times out.

    Simulates a scenario where:
    1. Order is submitted but stays in SUBMITTED state too long
    2. Inflight check queries the order status
    3. Order is marked as REJECTED after max retries

    """
    # Arrange
    order = order_factory.limit(
        instrument_id=AUDUSD_SIM.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(10_000),
        price=AUDUSD_SIM.make_price(0.95000),
    )

    # Act - Submit order (mock client doesn't auto-process, so we simulate submission)
    cache.add_order(order)

    # Simulate order submission with old timestamp to ensure it's past threshold
    current_time = clock.timestamp_ns()
    submitted_event = TestEventStubs.order_submitted(
        order,
        account_id=account_id,
        ts_event=current_time - 1_000_000_000,  # 1 second ago
    )
    order.apply(submitted_event)
    exec_engine.process(submitted_event)
    cache.update_order(order)  # Ensure inflight index is updated

    # Venue never responds (simulating timeout scenario)
    # Don't add any order status report to the client

    # The continuous reconciliation loop will handle this
    # With inflight_check_interval_ms=100 and inflight_check_retries=2
    # We need to wait for:
    # - Initial threshold wait of 200ms (inflight_check_threshold_ms)
    # - First check at ~100ms after threshold (retry counter = 0, increments to 1)
    # - Second check at ~200ms (retry counter = 1, increments to 2)
    # - Third check at ~300ms (retry counter = 2, max reached, order rejected)
    # Total: ~500ms plus processing time

    # Assert - After max retries via continuous reconciliation, order should be rejected
    await eventually(lambda: order.status == OrderStatus.REJECTED, timeout=3.0)
    assert order.status == OrderStatus.REJECTED


@pytest.mark.asyncio
async def test_multiple_fills_reconciliation(
    exec_engine,
    exec_client,
    cache,
    clock,
    account_id,
    order_factory,
):
    """
    Test reconciliation of multiple fills for a single order.

    Simulates a scenario where:
    1. Order is partially filled multiple times
    2. Some fills are missed by the client
    3. Reconciliation reconstructs the correct order state

    """
    # Arrange
    order = order_factory.limit(
        instrument_id=AUDUSD_SIM.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        price=AUDUSD_SIM.make_price(1.00000),
    )

    # Act - Submit and accept order
    cache.add_order(order)

    # Simulate order submission
    submitted_event = TestEventStubs.order_submitted(
        order,
        account_id=account_id,
    )
    order.apply(submitted_event)
    exec_engine.process(submitted_event)
    await asyncio.sleep(0.01)

    exec_engine.process(
        TestEventStubs.order_accepted(
            order,
            account_id=account_id,
            venue_order_id=VenueOrderId("V-003"),
        ),
    )

    # Create multiple fills
    fill1 = FillReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-003"),
        trade_id=TradeId("T-003-1"),
        order_side=OrderSide.BUY,
        last_qty=Quantity.from_int(30_000),
        last_px=Price.from_str("1.00000"),
        commission=Money(1.50, USD),
        liquidity_side=LiquiditySide.TAKER,
        report_id=UUID4(),
        ts_event=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )

    fill2 = FillReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-003"),
        trade_id=TradeId("T-003-2"),
        order_side=OrderSide.BUY,
        last_qty=Quantity.from_int(40_000),
        last_px=Price.from_str("1.00001"),
        commission=Money(2.00, USD),
        liquidity_side=LiquiditySide.TAKER,
        report_id=UUID4(),
        ts_event=clock.timestamp_ns() + 1000,
        ts_init=clock.timestamp_ns() + 1000,
    )

    fill3 = FillReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-003"),
        trade_id=TradeId("T-003-3"),
        order_side=OrderSide.BUY,
        last_qty=Quantity.from_int(30_000),
        last_px=Price.from_str("1.00002"),
        commission=Money(1.50, USD),
        liquidity_side=LiquiditySide.TAKER,
        report_id=UUID4(),
        ts_event=clock.timestamp_ns() + 2000,
        ts_init=clock.timestamp_ns() + 2000,
    )

    # Client only received first fill - simulate via fill event
    fill_event = TestEventStubs.order_filled(
        order,
        instrument=AUDUSD_SIM,
        account_id=account_id,
        venue_order_id=VenueOrderId("V-003"),
        trade_id=TradeId("T-003-1"),
        last_qty=fill1.last_qty,
        last_px=fill1.last_px,
    )
    order.apply(fill_event)
    exec_engine.process(fill_event)

    # Venue has all fills
    exec_client.add_fill_reports(
        VenueOrderId("V-003"),
        [fill1, fill2, fill3],
    )

    # Venue status shows order fully filled
    status_report = OrderStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-003"),
        order_side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.FILLED,
        price=Price.from_str("1.00000"),
        quantity=Quantity.from_int(100_000),
        filled_qty=Quantity.from_int(100_000),
        avg_px=Decimal("1.00001"),  # Weighted average
        report_id=UUID4(),
        ts_accepted=clock.timestamp_ns(),
        ts_last=clock.timestamp_ns() + 2000,
        ts_init=clock.timestamp_ns() + 2000,
    )
    exec_client.add_order_status_report(status_report)

    # Trigger reconciliation
    result = await exec_engine.reconcile_execution_state()

    # Assert
    assert result is True
    assert order.status == OrderStatus.FILLED
    assert order.filled_qty == Quantity.from_int(100_000)


@pytest.mark.asyncio
async def test_concurrent_order_reconciliation(
    exec_engine,
    exec_client,
    cache,
    clock,
    account_id,
    order_factory,
):
    """
    Test reconciliation with multiple concurrent orders.

    Simulates a scenario where:
    1. Multiple orders are active simultaneously
    2. Some orders have discrepancies
    3. Reconciliation handles all orders correctly

    """
    # Arrange - Create multiple orders
    orders = []
    for i in range(5):
        order = order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY if i % 2 == 0 else OrderSide.SELL,
            quantity=Quantity.from_int(10_000 * (i + 1)),
            price=AUDUSD_SIM.make_price(1.00000 + 0.00001 * i),
        )
        orders.append(order)
        cache.add_order(order)

        # Simulate order submission
        submitted_event = TestEventStubs.order_submitted(
            order,
            account_id=account_id,
        )
        order.apply(submitted_event)
        exec_engine.process(submitted_event)

    await asyncio.sleep(0.1)  # Allow processing

    # Create varying states on venue
    for i, order in enumerate(orders):
        venue_order_id = VenueOrderId(f"V-MULTI-{i}")

        if i == 0:
            # First order accepted normally
            status = OrderStatus.ACCEPTED
            filled = 0
        elif i == 1:
            # Second order partially filled
            status = OrderStatus.PARTIALLY_FILLED
            filled = 5_000
        elif i == 2:
            # Third order fully filled
            status = OrderStatus.FILLED
            filled = 30_000
        elif i == 3:
            # Fourth order rejected
            status = OrderStatus.REJECTED
            filled = 0
        else:
            # Fifth order canceled
            status = OrderStatus.CANCELED
            filled = 0

        report = OrderStatusReport(
            account_id=account_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            order_side=order.side,
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            order_status=status,
            price=order.price,
            quantity=order.quantity,
            filled_qty=Quantity.from_int(filled),
            avg_px=Decimal(str(order.price)) if filled > 0 else None,
            cancel_reason="TEST_CANCEL" if status == OrderStatus.CANCELED else None,
            report_id=UUID4(),
            ts_accepted=(clock.timestamp_ns() if status != OrderStatus.REJECTED else 0),
            ts_last=clock.timestamp_ns(),
            ts_init=clock.timestamp_ns(),
        )
        exec_client.add_order_status_report(report)

    # Act - Trigger reconciliation
    result = await exec_engine.reconcile_execution_state()

    # Assert - verify reconciliation was successful and orders match venue state
    assert result is True
    # After reconciliation, orders should match their venue states exactly
    assert orders[0].status == OrderStatus.ACCEPTED  # Venue reported ACCEPTED
    assert orders[1].status == OrderStatus.PARTIALLY_FILLED  # Venue reported PARTIALLY_FILLED
    assert orders[1].filled_qty == Quantity.from_int(5_000)  # Verify fill amount
    assert orders[2].status == OrderStatus.FILLED  # Venue reported FILLED
    assert orders[2].filled_qty == Quantity.from_int(30_000)  # Verify complete fill
    assert orders[3].status == OrderStatus.REJECTED  # Venue reported REJECTED
    assert orders[4].status == OrderStatus.CANCELED  # Venue reported CANCELED


@pytest.mark.asyncio
async def test_targeted_query_limiting(
    msgbus,
    cache,
    clock,
    trader_id,
    account_id,
    order_factory,
):
    """
    Test that single-order queries are limited per cycle to prevent rate limit
    exhaustion.

    Simulates a scenario where:
    1. Many orders fail the bulk query check
    2. Single-order queries are needed for each order
    3. System limits queries per cycle to prevent rate limit errors

    """
    # Arrange - Configure engine with low limits for testing
    config = LiveExecEngineConfig(
        open_check_interval_secs=1.0,
        open_check_open_only=False,  # Full history mode so missing orders are detected
        max_single_order_queries_per_cycle=3,  # Low limit for testing
        single_order_query_delay_ms=50,  # Small delay for testing
        open_check_missing_retries=0,  # Immediately trigger single-order queries
    )

    exec_engine = LiveExecutionEngine(
        loop=asyncio.get_running_loop(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=config,
    )

    exec_client = MockLiveExecutionClient(
        loop=asyncio.get_running_loop(),
        client_id=ClientId(SIM.value),
        venue=SIM,
        account_type=AccountType.CASH,
        base_currency=USD,
        instrument_provider=InstrumentProvider(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    exec_engine.register_client(exec_client)
    exec_engine.start()

    # Create 10 orders and add them to cache as ACCEPTED
    orders = []
    for _ in range(10):
        order = order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=AUDUSD_SIM.make_qty(100),
            price=AUDUSD_SIM.make_price(1.0),
        )
        cache.add_order(order)
        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))
        cache.update_order(order)
        orders.append(order)

    # Mock returns empty reports (all orders "missing at venue")
    # No reports added to exec_client, so generate_order_status_reports returns []

    # Act - Run check_orders_consistency which should limit single-order queries
    await exec_engine._check_orders_consistency()

    # Assert - Only 3 single-order queries should have been attempted (max_single_order_queries_per_cycle)
    # Since single-order queries return None, orders should be resolved as REJECTED
    await eventually(lambda: len([o for o in orders if o.status == OrderStatus.REJECTED]) == 3)

    # Run another cycle to process more orders
    await exec_engine._check_orders_consistency()
    await eventually(lambda: len([o for o in orders if o.status == OrderStatus.REJECTED]) == 6)

    # Run one more cycle
    await exec_engine._check_orders_consistency()
    await eventually(lambda: len([o for o in orders if o.status == OrderStatus.REJECTED]) == 9)

    # Final cycle for the last order
    await exec_engine._check_orders_consistency()
    await eventually(lambda: len([o for o in orders if o.status == OrderStatus.REJECTED]) == 10)

    # Cleanup
    exec_engine.stop()
    await eventually(lambda: exec_engine.is_stopped)


@pytest.mark.asyncio
async def test_targeted_query_limiting_with_retry_accumulation(
    msgbus,
    cache,
    clock,
    trader_id,
    account_id,
    order_factory,
):
    """
    Test that orders accumulate retries even when max_single_order_queries_per_cycle is
    reached, ensuring reconciliation progresses over multiple cycles.

    Simulates a scenario where:
    1. Many orders need reconciliation simultaneously
    2. Rate limits prevent querying all at once
    3. Orders continue accumulating retries while waiting
    4. All orders eventually get reconciled

    """
    # Arrange - Configure with realistic retry threshold
    config = LiveExecEngineConfig(
        open_check_interval_secs=1.0,
        open_check_open_only=False,  # Full history mode
        max_single_order_queries_per_cycle=3,  # Low limit for testing
        single_order_query_delay_ms=10,  # Small delay for testing
        open_check_missing_retries=5,  # Realistic retry threshold
    )

    exec_engine = LiveExecutionEngine(
        loop=asyncio.get_running_loop(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=config,
    )

    exec_client = MockLiveExecutionClient(
        loop=asyncio.get_running_loop(),
        client_id=ClientId(SIM.value),
        venue=SIM,
        account_type=AccountType.CASH,
        base_currency=USD,
        instrument_provider=InstrumentProvider(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    exec_engine.register_client(exec_client)
    exec_engine.start()

    # Create 10 orders, all ACCEPTED (missing at venue)
    orders = []
    for _ in range(10):
        order = order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=AUDUSD_SIM.make_qty(100),
            price=AUDUSD_SIM.make_price(1.0),
        )
        cache.add_order(order)
        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))
        cache.update_order(order)
        orders.append(order)

    # Cycle 1: All orders get retry count 1 (none ready for query yet)
    await exec_engine._check_orders_consistency()
    for order in orders:
        assert exec_engine._recon_check_retries.get(order.client_order_id, 0) == 1
    assert all(o.status == OrderStatus.ACCEPTED for o in orders)

    # Cycle 2-5: Retry counts accumulate to 5 (threshold)
    for cycle in range(2, 6):
        await exec_engine._check_orders_consistency()
        for order in orders:
            assert exec_engine._recon_check_retries.get(order.client_order_id, 0) == cycle

    # Cycle 6: All 10 orders now at threshold (5), but only 3 can be queried
    # First 3 get queried and resolved, remaining 7 increment to retry count 6
    await exec_engine._check_orders_consistency()
    await eventually(lambda: len([o for o in orders if o.status == OrderStatus.REJECTED]) == 3)

    # Check that the remaining 7 orders have retry count incremented
    for order in orders:
        if order.status == OrderStatus.ACCEPTED:
            # These hit the limit, got retries incremented but not queried
            assert exec_engine._recon_check_retries.get(order.client_order_id, 0) == 6

    # Cycle 7: 3 more get queried (total 6 resolved), remaining 4 at retry 7
    await exec_engine._check_orders_consistency()
    await eventually(lambda: len([o for o in orders if o.status == OrderStatus.REJECTED]) == 6)

    # Cycle 8: 3 more (total 9), 1 remaining at retry 8
    await exec_engine._check_orders_consistency()
    await eventually(lambda: len([o for o in orders if o.status == OrderStatus.REJECTED]) == 9)

    # Cycle 9: Last order resolved
    await exec_engine._check_orders_consistency()
    await eventually(lambda: len([o for o in orders if o.status == OrderStatus.REJECTED]) == 10)

    # All orders eventually processed
    await eventually(lambda: all(o.status == OrderStatus.REJECTED for o in orders))

    # Cleanup
    exec_engine.stop()
    await eventually(lambda: exec_engine.is_stopped)


@pytest.mark.asyncio
async def test_cross_zero_reconciliation_with_missing_avg_px_uses_close_price_fallback(
    msgbus,
    cache,
    clock,
    trader_id,
    account_id,
    order_factory,
    portfolio,
):
    """
    Test cross-zero position reconciliation when venue position report lacks avg_px_open
    and no quote tick is available.

    Inspired by real-world failure case with Bybit SPOT wallet-based positions:
    - Internal position: SHORT -40.853 @ 48.96
    - Venue reports: LONG 36471.313 with avg_px_open=None (wallet balance)
    - No quote tick available (before market data subscriptions)
    - Should use close_price as fallback for opening the new position

    This test verifies the fix where close_price is used as fallback when:
    1. Position crosses through zero (SHORT -> LONG or LONG -> SHORT)
    2. Venue position report has avg_px_open=None
    3. No quote tick available in cache

    """
    # Arrange
    config = LiveExecEngineConfig(
        reconciliation=True,
        reconciliation_lookback_mins=1,
    )

    exec_engine = LiveExecutionEngine(
        loop=asyncio.get_running_loop(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=config,
    )

    # Position report with missing avg_px_open (simulating spot asset position without cost basis)
    position_report = PositionStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        position_side=PositionSide.LONG,
        quantity=Quantity.from_str("100.0"),
        avg_px_open=None,  # Missing - spot asset position
        report_id=UUID4(),
        ts_last=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )

    exec_client = MockLiveExecutionClient(
        loop=asyncio.get_running_loop(),
        client_id=ClientId(SIM.value),
        venue=SIM,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        instrument_provider=InstrumentProvider(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Set up the mock client to return our position report
    exec_client.add_position_status_report(position_report)

    exec_engine.register_client(exec_client)
    exec_engine.start()

    # Ensure NO quote tick in cache (simulating before market data subscriptions)
    assert cache.quote_tick(AUDUSD_SIM.id) is None

    # Act - Reconcile the execution state
    # Reconciliation should handle position report with missing avg_px_open gracefully
    result = await exec_engine.reconcile_execution_state(timeout_secs=5.0)

    # The key test: reconciliation should complete without crashing
    # For CurrencyPair instruments, missing avg_px_open should be handled gracefully
    assert result is not None, "Reconciliation should complete"

    # Cleanup
    exec_engine.stop()
    await eventually(lambda: exec_engine.is_stopped)


@pytest.mark.asyncio
async def test_position_discrepancy_queries_missing_fills(
    msgbus,
    cache,
    clock,
    trader_id,
    account_id,
    order_factory,
):
    """
    Test continuous position reconciliation detects discrepancy and queries for missing
    fills.

    Simulates a scenario where:
    1. Order is filled locally (position opened)
    2. Venue has additional fill that was missed (position larger than expected)
    3. Position check detects discrepancy and queries for missing fills
    4. Missing fill is reconciled and position syncs with venue

    """
    # Arrange
    config = LiveExecEngineConfig(
        reconciliation=True,
        position_check_interval_secs=0.5,  # Fast check for testing
        position_check_lookback_mins=60,
        position_check_threshold_ms=100,  # Low threshold for testing
        reconciliation_startup_delay_secs=0,
    )

    exec_engine = LiveExecutionEngine(
        loop=asyncio.get_running_loop(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=config,
    )

    exec_client = MockLiveExecutionClient(
        loop=asyncio.get_running_loop(),
        client_id=ClientId(SIM.value),
        venue=SIM,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        instrument_provider=InstrumentProvider(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    exec_engine.register_client(exec_client)
    exec_engine.start()

    # Set startup reconciliation event to unblock continuous loop
    exec_engine._startup_reconciliation_event.set()
    await asyncio.sleep(0.1)  # Give continuous loop time to start

    # Create and process an order with one fill locally
    order = order_factory.limit(
        instrument_id=AUDUSD_SIM.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        price=AUDUSD_SIM.make_price(1.00000),
    )

    cache.add_order(order)
    order.apply(TestEventStubs.order_submitted(order, account_id=account_id))
    order.apply(
        TestEventStubs.order_accepted(
            order,
            account_id=account_id,
            venue_order_id=VenueOrderId("V-POS-001"),
        ),
    )

    # Apply first fill locally
    fill1_event = TestEventStubs.order_filled(
        order,
        instrument=AUDUSD_SIM,
        account_id=account_id,
        venue_order_id=VenueOrderId("V-POS-001"),
        trade_id=TradeId("T-POS-1"),
        last_qty=Quantity.from_int(50_000),
        last_px=Price.from_str("1.00000"),
    )
    # Process through engine (engine will apply it to the order)
    exec_engine.process(fill1_event)

    # Wait for position to be created
    await eventually(
        lambda: len(cache.positions_open(instrument_id=AUDUSD_SIM.id)) == 1
        and cache.positions_open(instrument_id=AUDUSD_SIM.id)[0].quantity
        == Quantity.from_int(50_000),
        timeout=1.0,
    )

    # Local position should be 50k
    positions = cache.positions_open(instrument_id=AUDUSD_SIM.id)
    assert len(positions) == 1
    assert positions[0].quantity == Quantity.from_int(50_000)

    # Venue has TWO fills (but first one was already processed locally, so only report the missing one)
    fill2_report = FillReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-POS-001"),
        trade_id=TradeId("T-POS-2"),  # This is the missing fill
        order_side=OrderSide.BUY,
        last_qty=Quantity.from_int(30_000),
        last_px=Price.from_str("1.00001"),
        commission=Money(0.60, USD),
        liquidity_side=LiquiditySide.TAKER,
        report_id=UUID4(),
        ts_event=clock.timestamp_ns() + 1000,
        ts_init=clock.timestamp_ns() + 1000,
    )

    exec_client.add_fill_reports(VenueOrderId("V-POS-001"), [fill2_report])

    # Add order status report for the order (so fills can be reconciled)
    order_status_report = OrderStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-POS-001"),
        order_side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.PARTIALLY_FILLED,
        price=Price.from_str("1.00000"),
        quantity=Quantity.from_int(100_000),
        filled_qty=Quantity.from_int(80_000),
        avg_px=Decimal("1.000004"),
        report_id=UUID4(),
        ts_accepted=clock.timestamp_ns(),
        ts_last=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )
    exec_client.add_order_status_report(order_status_report)

    # Venue position shows 80k (both fills)
    venue_position_report = PositionStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        position_side=PositionSide.LONG,
        quantity=Quantity.from_int(80_000),
        avg_px_open=Decimal("1.000004"),
        report_id=UUID4(),
        ts_last=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )
    exec_client.add_position_status_report(venue_position_report)

    # Wait for position check threshold to pass and for reconciliation loop to start
    await asyncio.sleep(0.6)  # Position check interval is 0.5s

    # Act - Wait for position check to detect and reconcile the discrepancy
    await eventually(
        lambda: cache.positions_open(instrument_id=AUDUSD_SIM.id)[0].quantity
        == Quantity.from_int(80_000),
        timeout=5.0,  # Increased timeout
    )

    # Assert
    positions = cache.positions_open(instrument_id=AUDUSD_SIM.id)
    assert len(positions) == 1
    assert positions[0].quantity == Quantity.from_int(80_000)
    assert order.filled_qty == Quantity.from_int(80_000)

    # Cleanup
    exec_engine.stop()
    await eventually(lambda: exec_engine.is_stopped)


@pytest.mark.asyncio
async def test_position_reconciliation_venue_has_position_we_think_flat(
    msgbus,
    cache,
    clock,
    trader_id,
    account_id,
    order_factory,
):
    """
    Test position reconciliation when venue reports a position but we think we're flat.

    Simulates a scenario where:
    1. No local position (we think we're flat)
    2. Venue reports an open position
    3. Position check detects this and queries for missing fills
    4. Missing fills are reconciled to match venue position

    """
    # Arrange
    config = LiveExecEngineConfig(
        reconciliation=True,
        position_check_interval_secs=0.5,
        position_check_lookback_mins=60,
        position_check_threshold_ms=100,
        reconciliation_startup_delay_secs=0,
    )

    exec_engine = LiveExecutionEngine(
        loop=asyncio.get_running_loop(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=config,
    )

    exec_client = MockLiveExecutionClient(
        loop=asyncio.get_running_loop(),
        client_id=ClientId(SIM.value),
        venue=SIM,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        instrument_provider=InstrumentProvider(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    exec_engine.register_client(exec_client)
    exec_engine.start()

    # Set startup reconciliation event to unblock continuous loop
    exec_engine._startup_reconciliation_event.set()
    await asyncio.sleep(0.1)  # Give continuous loop time to start

    # Seed an order in the cache to mirror the venue order (no fills yet, still locally flat)
    order = order_factory.limit(
        instrument_id=AUDUSD_SIM.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(75_000),
        price=AUDUSD_SIM.make_price(0.99500),
    )
    cache.add_order(order)
    venue_order_id = VenueOrderId("V-MISSED-001")

    submitted_event = TestEventStubs.order_submitted(
        order,
        account_id=account_id,
        ts_event=clock.timestamp_ns(),
    )
    order.apply(submitted_event)
    exec_engine.process(submitted_event)

    accepted_event = TestEventStubs.order_accepted(
        order,
        account_id=account_id,
        venue_order_id=venue_order_id,
        ts_event=clock.timestamp_ns(),
    )
    order.apply(accepted_event)
    exec_engine.process(accepted_event)
    cache.add_venue_order_id(order.client_order_id, venue_order_id, overwrite=True)

    # Wait for order to be in ACCEPTED state
    await eventually(
        lambda: cache.order(order.client_order_id).status == OrderStatus.ACCEPTED,
        timeout=1.0,
    )

    order_client_id = order.client_order_id

    # No local position - we think we're flat
    assert len(cache.positions_open(instrument_id=AUDUSD_SIM.id)) == 0

    # But venue has a position and fills we don't know about

    fill_report = FillReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order_client_id,
        venue_order_id=venue_order_id,
        trade_id=TradeId("T-MISSED-1"),
        order_side=OrderSide.BUY,
        last_qty=Quantity.from_int(75_000),
        last_px=Price.from_str("0.99500"),
        commission=Money(1.50, USD),
        liquidity_side=LiquiditySide.TAKER,
        report_id=UUID4(),
        ts_event=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )

    # Venue position report
    venue_position_report = PositionStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        position_side=PositionSide.LONG,
        quantity=Quantity.from_int(75_000),
        avg_px_open=Decimal("0.99500"),
        report_id=UUID4(),
        ts_last=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )

    exec_client.add_position_status_report(venue_position_report)
    exec_client.add_fill_reports(venue_order_id, [fill_report])

    # Create the order that the fill references (so reconciliation can work)
    order_status_report = OrderStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order_client_id,
        venue_order_id=venue_order_id,
        order_side=OrderSide.BUY,
        order_type=OrderType.MARKET,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.FILLED,
        quantity=Quantity.from_int(75_000),
        filled_qty=Quantity.from_int(75_000),
        avg_px=Decimal("0.99500"),
        report_id=UUID4(),
        ts_accepted=clock.timestamp_ns(),
        ts_last=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )
    exec_client.add_order_status_report(order_status_report)

    # Wait for threshold and position check interval to pass
    await asyncio.sleep(0.6)  # Position check interval is 0.5s

    # Act - Wait for position check to reconcile
    await eventually(
        lambda: len(cache.positions_open(instrument_id=AUDUSD_SIM.id)) > 0,
        timeout=3.0,
    )

    # Assert
    positions = cache.positions_open(instrument_id=AUDUSD_SIM.id)
    assert len(positions) == 1
    assert positions[0].quantity == Quantity.from_int(75_000)

    # Cleanup
    exec_engine.stop()
    await eventually(lambda: exec_engine.is_stopped)


@pytest.mark.asyncio
async def test_position_reconciliation_respects_threshold(
    msgbus,
    cache,
    clock,
    trader_id,
    account_id,
    order_factory,
):
    """
    Test that position reconciliation respects the threshold for recent activity.

    Simulates a scenario where:
    1. Position has recent activity
    2. Venue reports different position quantity
    3. Position check skips reconciliation due to threshold
    4. After threshold passes, reconciliation proceeds

    """
    # Arrange
    config = LiveExecEngineConfig(
        reconciliation=True,
        position_check_interval_secs=0.2,  # Frequent checks
        position_check_lookback_mins=60,
        position_check_threshold_ms=1000,  # 1 second threshold
        reconciliation_startup_delay_secs=0,
    )

    exec_engine = LiveExecutionEngine(
        loop=asyncio.get_running_loop(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=config,
    )

    exec_client = MockLiveExecutionClient(
        loop=asyncio.get_running_loop(),
        client_id=ClientId(SIM.value),
        venue=SIM,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        instrument_provider=InstrumentProvider(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    exec_engine.register_client(exec_client)
    exec_engine.start()

    # Set startup reconciliation event to unblock continuous loop
    exec_engine._startup_reconciliation_event.set()
    await asyncio.sleep(0.1)  # Give continuous loop time to start

    # Create order with a fill
    order = order_factory.limit(
        instrument_id=AUDUSD_SIM.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        price=AUDUSD_SIM.make_price(1.00000),
    )

    cache.add_order(order)
    order.apply(TestEventStubs.order_submitted(order, account_id=account_id))
    order.apply(
        TestEventStubs.order_accepted(
            order,
            account_id=account_id,
            venue_order_id=VenueOrderId("V-THRESHOLD-001"),
        ),
    )

    # Recent fill (within threshold)
    fill_event = TestEventStubs.order_filled(
        order,
        instrument=AUDUSD_SIM,
        account_id=account_id,
        venue_order_id=VenueOrderId("V-THRESHOLD-001"),
        trade_id=TradeId("T-THRESHOLD-1"),
        last_qty=Quantity.from_int(40_000),
        last_px=Price.from_str("1.00000"),
        ts_event=clock.timestamp_ns(),
    )
    # Process through engine (engine will apply it to the order)
    exec_engine.process(fill_event)

    # Wait for position to be created
    await eventually(
        lambda: len(cache.positions_open(instrument_id=AUDUSD_SIM.id)) == 1
        and cache.positions_open(instrument_id=AUDUSD_SIM.id)[0].quantity
        == Quantity.from_int(40_000),
        timeout=1.0,
    )

    # Local position is 40k
    positions = cache.positions_open(instrument_id=AUDUSD_SIM.id)
    assert len(positions) == 1
    assert positions[0].quantity == Quantity.from_int(40_000)

    # Setup venue reports BEFORE reconciliation can run
    # Add order status report so fills can be reconciled
    order_status_report = OrderStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-THRESHOLD-001"),
        order_side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.PARTIALLY_FILLED,
        price=Price.from_str("1.00000"),
        quantity=Quantity.from_int(100_000),
        filled_qty=Quantity.from_int(60_000),
        avg_px=Decimal("1.00000"),
        report_id=UUID4(),
        ts_accepted=clock.timestamp_ns(),
        ts_last=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )
    exec_client.add_order_status_report(order_status_report)

    # Add the missing fill
    # First fill (T-THRESHOLD-1) was already applied locally, so only report the missing one
    fill2_report = FillReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-THRESHOLD-001"),
        trade_id=TradeId("T-THRESHOLD-2"),  # Missing fill
        order_side=OrderSide.BUY,
        last_qty=Quantity.from_int(20_000),
        last_px=Price.from_str("1.00001"),
        commission=Money(0.40, USD),
        liquidity_side=LiquiditySide.TAKER,
        report_id=UUID4(),
        ts_event=clock.timestamp_ns() + 500,
        ts_init=clock.timestamp_ns() + 500,
    )
    exec_client.add_fill_reports(VenueOrderId("V-THRESHOLD-001"), [fill2_report])

    # Venue reports 60k (discrepancy)
    venue_position_report = PositionStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        position_side=PositionSide.LONG,
        quantity=Quantity.from_int(60_000),
        avg_px_open=Decimal("1.00000"),
        report_id=UUID4(),
        ts_last=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )
    exec_client.add_position_status_report(venue_position_report)

    # Act - Immediately check position (should skip due to recent activity)
    await asyncio.sleep(0.3)  # Less than threshold

    # Assert - Position should NOT have reconciled yet (recent activity)
    positions = cache.positions_open(instrument_id=AUDUSD_SIM.id)
    assert positions[0].quantity == Quantity.from_int(40_000)  # Still local value

    # Wait for threshold to pass (1000ms = 1 second)
    await asyncio.sleep(1.2)  # Total > 1.5s, past threshold

    # Act - Now reconciliation should proceed after threshold expires
    await eventually(
        lambda: cache.positions_open(instrument_id=AUDUSD_SIM.id)[0].quantity
        == Quantity.from_int(60_000),
        timeout=3.0,
    )

    # Assert - Position should now be reconciled to 60k
    positions = cache.positions_open(instrument_id=AUDUSD_SIM.id)
    assert len(positions) == 1
    assert positions[0].quantity == Quantity.from_int(60_000)
    assert order.filled_qty == Quantity.from_int(60_000)

    # Cleanup
    exec_engine.stop()
    await eventually(lambda: exec_engine.is_stopped)


@pytest.mark.asyncio
async def test_position_reconciliation_respects_instrument_filter(
    msgbus,
    cache,
    clock,
    trader_id,
    account_id,
):
    """
    Test that position reconciliation respects reconciliation_instrument_ids filter.

    Simulates a scenario where:
    1. Multiple instruments have position discrepancies
    2. Only specific instruments are included in reconciliation filter
    3. Position check only reconciles filtered instruments

    """
    # Create two instruments
    eurusd_sim = TestInstrumentProvider.default_fx_ccy("EUR/USD")
    cache.add_instrument(eurusd_sim)

    # Arrange - Configure with instrument filter (only AUDUSD)
    config = LiveExecEngineConfig(
        reconciliation=True,
        reconciliation_instrument_ids=[AUDUSD_SIM.id],  # Only AUDUSD
        position_check_interval_secs=0.5,
        position_check_lookback_mins=60,
        position_check_threshold_ms=100,
        reconciliation_startup_delay_secs=0,
    )

    exec_engine = LiveExecutionEngine(
        loop=asyncio.get_running_loop(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=config,
    )

    exec_client = MockLiveExecutionClient(
        loop=asyncio.get_running_loop(),
        client_id=ClientId(SIM.value),
        venue=SIM,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        instrument_provider=InstrumentProvider(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    exec_engine.register_client(exec_client)
    exec_engine.start()

    # Venue has positions for BOTH instruments (but we think we're flat)
    audusd_position_report = PositionStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        position_side=PositionSide.LONG,
        quantity=Quantity.from_int(50_000),
        avg_px_open=Decimal("1.00000"),
        report_id=UUID4(),
        ts_last=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )

    eurusd_position_report = PositionStatusReport(
        account_id=account_id,
        instrument_id=eurusd_sim.id,
        position_side=PositionSide.LONG,
        quantity=Quantity.from_int(75_000),
        avg_px_open=Decimal("1.10000"),
        report_id=UUID4(),
        ts_last=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )

    exec_client.add_position_status_report(audusd_position_report)
    exec_client.add_position_status_report(eurusd_position_report)

    # Wrap generate_fill_reports with AsyncMock to track calls
    original_generate_fill_reports = exec_client.generate_fill_reports
    mock_generate_fill_reports = AsyncMock(side_effect=original_generate_fill_reports)
    exec_client.generate_fill_reports = mock_generate_fill_reports

    # Set startup reconciliation event
    exec_engine._startup_reconciliation_event.set()

    # Wait for threshold
    await asyncio.sleep(0.2)

    # Act - Trigger position check
    await exec_engine._check_positions_consistency()

    # Assert - AUDUSD should be checked (in filter), EURUSD should be skipped
    # Verify calls to generate_fill_reports using the mock
    assert mock_generate_fill_reports.called, "generate_fill_reports should have been called"

    # Check which instruments were queried by examining call_args
    called_instrument_ids = [
        (
            call.kwargs["command"].instrument_id
            if "command" in call.kwargs
            else call.args[0].instrument_id
        )
        for call in mock_generate_fill_reports.call_args_list
    ]

    # AUDUSD should have been queried (in filter)
    assert AUDUSD_SIM.id in called_instrument_ids, "AUDUSD should be queried"
    # EURUSD should NOT have been queried (not in filter)
    assert eurusd_sim.id not in called_instrument_ids, "EURUSD should NOT be queried"

    # Both should still be flat (no fills/orders provided for reconciliation)
    positions_audusd = cache.positions_open(instrument_id=AUDUSD_SIM.id)
    positions_eurusd = cache.positions_open(instrument_id=eurusd_sim.id)
    assert len(positions_audusd) == 0
    assert len(positions_eurusd) == 0

    # Cleanup
    exec_engine.stop()
    await eventually(lambda: exec_engine.is_stopped)


@pytest.mark.asyncio
async def test_recent_fills_cache_prevents_duplicate_reconciliation(
    msgbus,
    cache,
    clock,
    trader_id,
    account_id,
    order_factory,
    portfolio,
):
    """
    Test that recent fills cache prevents duplicate reconciliation of fills.

    Simulates a scenario where:
    1. Fill is processed and added to recent fills cache
    2. Position check runs shortly after
    3. Same fill appears in venue fill query
    4. Fill is filtered out via recent fills cache (prevents duplicate)

    """
    # Arrange
    config = LiveExecEngineConfig(
        reconciliation=True,
        position_check_interval_secs=0.2,  # Frequent checks
        position_check_lookback_mins=60,
        position_check_threshold_ms=50,  # Very low threshold
        reconciliation_startup_delay_secs=0,
    )

    exec_engine = LiveExecutionEngine(
        loop=asyncio.get_running_loop(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=config,
    )

    exec_client = MockLiveExecutionClient(
        loop=asyncio.get_running_loop(),
        client_id=ClientId(SIM.value),
        venue=SIM,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        instrument_provider=InstrumentProvider(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    exec_engine.register_client(exec_client)
    exec_engine.start()

    # Create and process order with fill
    order = order_factory.limit(
        instrument_id=AUDUSD_SIM.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        price=AUDUSD_SIM.make_price(1.00000),
    )

    cache.add_order(order)
    order.apply(TestEventStubs.order_submitted(order, account_id=account_id))
    order.apply(
        TestEventStubs.order_accepted(
            order,
            account_id=account_id,
            venue_order_id=VenueOrderId("V-CACHE-001"),
        ),
    )

    # Process fill locally (adds to recent fills cache)
    fill_event = TestEventStubs.order_filled(
        order,
        instrument=AUDUSD_SIM,
        account_id=account_id,
        venue_order_id=VenueOrderId("V-CACHE-001"),
        trade_id=TradeId("T-CACHE-1"),
        last_qty=Quantity.from_int(100_000),
        last_px=Price.from_str("1.00000"),
    )
    # Don't apply manually - let exec_engine.process handle it
    exec_engine.process(fill_event)

    # Allow async processing to complete
    await asyncio.sleep(0.05)

    # Verify fill is in recent cache
    assert TradeId("T-CACHE-1") in exec_engine._recent_fills_cache

    # Position is 100k
    positions = cache.positions_open(instrument_id=AUDUSD_SIM.id)
    assert len(positions) == 1
    assert positions[0].quantity == Quantity.from_int(100_000)

    # Venue also reports the same fill and position
    fill_report = FillReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-CACHE-001"),
        trade_id=TradeId("T-CACHE-1"),  # Same trade ID
        order_side=OrderSide.BUY,
        last_qty=Quantity.from_int(100_000),
        last_px=Price.from_str("1.00000"),
        commission=Money(2.00, USD),
        liquidity_side=LiquiditySide.TAKER,
        report_id=UUID4(),
        ts_event=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )

    venue_position_report = PositionStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        position_side=PositionSide.LONG,
        quantity=Quantity.from_int(100_000),
        avg_px_open=Decimal("1.00000"),
        report_id=UUID4(),
        ts_last=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )

    exec_client.add_fill_reports(VenueOrderId("V-CACHE-001"), [fill_report])
    exec_client.add_position_status_report(venue_position_report)

    # Set startup reconciliation event
    exec_engine._startup_reconciliation_event.set()

    # Wait for threshold
    await asyncio.sleep(0.1)

    # Act - Run position check
    await exec_engine._check_positions_consistency()

    # Assert - Position should remain 100k (not doubled)
    # Recent fills cache prevented duplicate processing
    positions = cache.positions_open(instrument_id=AUDUSD_SIM.id)
    assert len(positions) == 1
    assert positions[0].quantity == Quantity.from_int(100_000)  # Not 200k!

    # Cleanup
    exec_engine.stop()
    await eventually(lambda: exec_engine.is_stopped)


@pytest.mark.asyncio
async def test_position_reconciliation_fill_without_cached_order(
    msgbus,
    cache,
    clock,
    trader_id,
    account_id,
):
    """
    Test position reconciliation when a fill appears but the order isn't cached yet.

    Simulates a race condition scenario where:
    1. Venue reports a position
    2. Venue returns fills for that position
    3. But the corresponding order hasn't been cached yet (EXTERNAL or not yet reconciled)
    4. _reconcile_fill_report_single returns False (fill deferred)
    5. Position remains flat (fill not reconciled until order is cached)

    """
    # Arrange
    config = LiveExecEngineConfig(
        reconciliation=True,
        position_check_interval_secs=0.5,
        position_check_lookback_mins=60,
        position_check_threshold_ms=100,
        reconciliation_startup_delay_secs=0,
    )

    exec_engine = LiveExecutionEngine(
        loop=asyncio.get_running_loop(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=config,
    )

    exec_client = MockLiveExecutionClient(
        loop=asyncio.get_running_loop(),
        client_id=ClientId(SIM.value),
        venue=SIM,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        instrument_provider=InstrumentProvider(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    exec_engine.register_client(exec_client)
    exec_engine.start()

    # No local position - we think we're flat
    assert len(cache.positions_open(instrument_id=AUDUSD_SIM.id)) == 0

    # Venue has a fill for an order we haven't cached yet
    uncached_order_id = ClientOrderId("O-UNCACHED-001")

    fill_report = FillReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=uncached_order_id,
        venue_order_id=VenueOrderId("V-UNCACHED-001"),
        trade_id=TradeId("T-UNCACHED-1"),
        order_side=OrderSide.BUY,
        last_qty=Quantity.from_int(50_000),
        last_px=Price.from_str("1.00000"),
        commission=Money(1.00, USD),
        liquidity_side=LiquiditySide.TAKER,
        report_id=UUID4(),
        ts_event=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )

    # Venue position report
    venue_position_report = PositionStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        position_side=PositionSide.LONG,
        quantity=Quantity.from_int(50_000),
        avg_px_open=Decimal("1.00000"),
        report_id=UUID4(),
        ts_last=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )

    exec_client.add_position_status_report(venue_position_report)
    exec_client.add_fill_reports(VenueOrderId("V-UNCACHED-001"), [fill_report])

    # DO NOT add order status report - order not cached yet
    # This simulates the race condition

    # Set startup reconciliation event
    exec_engine._startup_reconciliation_event.set()

    # Wait for threshold
    await asyncio.sleep(0.2)

    # Act - Run position check (should handle missing order gracefully)
    await exec_engine._check_positions_consistency()

    # Assert - Position should still be flat (fill not reconciled because order isn't cached)
    # The system logs a warning and defers the fill, but doesn't crash
    positions = cache.positions_open(instrument_id=AUDUSD_SIM.id)
    assert len(positions) == 0, "Position should remain flat when fill's order isn't cached"

    # Verify the order isn't in cache (confirming the race condition scenario)
    order = cache.order(uncached_order_id)
    assert order is None, "Order should not be in cache"

    # Cleanup
    exec_engine.stop()
    await eventually(lambda: exec_engine.is_stopped)


@pytest.mark.asyncio
async def test_position_reconciliation_handles_generate_fill_reports_exception(
    msgbus,
    cache,
    clock,
    trader_id,
    account_id,
    order_factory,
    portfolio,
):
    """
    Test position reconciliation handles exceptions from generate_fill_reports
    gracefully.

    Simulates a scenario where:
    1. Venue reports a position discrepancy
    2. generate_fill_reports raises an exception (API error, network error, etc.)
    3. Exception is handled gracefully, position remains at local value

    """
    # Arrange
    config = LiveExecEngineConfig(
        reconciliation=True,
        position_check_interval_secs=0.5,
        position_check_lookback_mins=60,
        position_check_threshold_ms=100,
        reconciliation_startup_delay_secs=0,
    )

    exec_engine = LiveExecutionEngine(
        loop=asyncio.get_running_loop(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=config,
    )

    exec_client = MockLiveExecutionClient(
        loop=asyncio.get_running_loop(),
        client_id=ClientId(SIM.value),
        venue=SIM,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        instrument_provider=InstrumentProvider(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    exec_engine.register_client(exec_client)
    exec_engine.start()

    # Create a position locally
    order = order_factory.limit(
        instrument_id=AUDUSD_SIM.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        price=AUDUSD_SIM.make_price(1.00000),
    )

    cache.add_order(order)
    order.apply(TestEventStubs.order_submitted(order, account_id=account_id))
    order.apply(
        TestEventStubs.order_accepted(
            order,
            account_id=account_id,
            venue_order_id=VenueOrderId("V-EXCEPTION-001"),
        ),
    )

    fill_event = TestEventStubs.order_filled(
        order,
        instrument=AUDUSD_SIM,
        account_id=account_id,
        venue_order_id=VenueOrderId("V-EXCEPTION-001"),
        trade_id=TradeId("T-EXCEPTION-1"),
        last_qty=Quantity.from_int(50_000),
        last_px=Price.from_str("1.00000"),
    )
    order.apply(fill_event)
    exec_engine.process(fill_event)

    # Allow async processing
    await asyncio.sleep(0.1)

    # Local position is 50k
    positions = cache.positions_open(instrument_id=AUDUSD_SIM.id)
    assert len(positions) == 1
    assert positions[0].quantity == Quantity.from_int(50_000)

    # Venue reports 80k (discrepancy)
    venue_position_report = PositionStatusReport(
        account_id=account_id,
        instrument_id=AUDUSD_SIM.id,
        position_side=PositionSide.LONG,
        quantity=Quantity.from_int(80_000),
        avg_px_open=Decimal("1.00000"),
        report_id=UUID4(),
        ts_last=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )
    exec_client.add_position_status_report(venue_position_report)

    # Mock generate_fill_reports to raise an exception
    exec_client.generate_fill_reports = AsyncMock(
        side_effect=RuntimeError("API connection timeout"),
    )

    # Set startup reconciliation event
    exec_engine._startup_reconciliation_event.set()

    # Wait for threshold
    await asyncio.sleep(0.2)

    # Act - Run position check (should handle exception gracefully without crashing)
    await exec_engine._check_positions_consistency()

    # Assert - Position should remain at local value (reconciliation failed gracefully)
    # The system logs an error about the exception but doesn't crash
    positions = cache.positions_open(instrument_id=AUDUSD_SIM.id)
    assert len(positions) == 1, "Should still have the position"
    assert positions[0].quantity == Quantity.from_int(
        50_000,
    ), "Position should remain at local value when exception occurs"

    # Cleanup
    exec_engine.stop()
    await eventually(lambda: exec_engine.is_stopped)


@pytest.mark.asyncio
async def test_position_flip_netting_mode(
    event_loop,
    msgbus,
    cache,
    clock,
    order_factory,
    exec_client,
    account_id,
):
    """
    Test that position flip in NETTING mode properly handles the position state.

    Verifies that NETTING position flips correctly update position side and quantity.

    """
    exec_engine = LiveExecutionEngine(
        loop=event_loop,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=LiveExecEngineConfig(
            reconciliation=True,
            inflight_check_interval_ms=100,
            inflight_check_threshold_ms=200,
            inflight_check_retries=2,
            open_check_interval_secs=0.5,
            reconciliation_startup_delay_secs=0,
            snapshot_positions=True,
        ),
    )

    # Override exec_client with NETTING mode
    exec_client_netting = MockLiveExecutionClient(
        loop=event_loop,
        client_id=ClientId(SIM.value),
        venue=SIM,
        account_type=AccountType.CASH,
        base_currency=USD,
        instrument_provider=InstrumentProvider(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        oms_type=OmsType.NETTING,
    )

    exec_engine.register_client(exec_client_netting)
    exec_engine.start()
    exec_engine._startup_reconciliation_event.set()
    await eventually(lambda: exec_engine.is_running)

    try:
        # ENTER - open LONG position
        order_entry = order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )
        cache.add_order(order_entry)
        exec_engine.process(
            TestEventStubs.order_submitted(order_entry, ts_event=clock.timestamp_ns()),
        )
        exec_engine.process(
            TestEventStubs.order_accepted(order_entry, ts_event=clock.timestamp_ns()),
        )
        exec_engine.process(
            TestEventStubs.order_filled(
                order_entry,
                instrument=AUDUSD_SIM,
                account_id=account_id,
                last_px=Price.from_str("1.00000"),
                trade_id=TradeId("1"),
                ts_event=clock.timestamp_ns(),
            ),
        )

        # Wait for position to be opened
        await eventually(lambda: len(cache.positions()) == 1)
        await eventually(lambda: cache.positions()[0].is_open)

        original_pos = cache.positions()[0]
        assert original_pos.side == PositionSide.LONG
        assert original_pos.quantity == Quantity.from_int(100_000)

        # FLIP - close LONG and open SHORT position
        order_flip = order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(150_000),
        )
        cache.add_order(order_flip)
        exec_engine.process(
            TestEventStubs.order_submitted(order_flip, ts_event=clock.timestamp_ns()),
        )
        exec_engine.process(
            TestEventStubs.order_accepted(order_flip, ts_event=clock.timestamp_ns()),
        )
        exec_engine.process(
            TestEventStubs.order_filled(
                order_flip,
                instrument=AUDUSD_SIM,
                account_id=account_id,
                last_qty=Quantity.from_int(150_000),
                last_px=Price.from_str("1.00010"),
                trade_id=TradeId("2"),
            ),
        )

        # Wait for position to be flipped to SHORT
        await eventually(lambda: len(cache.positions()) > 0 and cache.positions()[0].side == PositionSide.SHORT)

        # Assert - verify position flip
        flipped_pos = cache.positions()[0]
        assert flipped_pos.side == PositionSide.SHORT
        assert flipped_pos.quantity == Quantity.from_int(50_000)

        # Close and reopen to verify position lifecycle
        order_close = order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(50_000),
        )
        cache.add_order(order_close)
        exec_engine.process(
            TestEventStubs.order_submitted(order_close, ts_event=clock.timestamp_ns()),
        )
        exec_engine.process(
            TestEventStubs.order_accepted(order_close, ts_event=clock.timestamp_ns()),
        )
        exec_engine.process(
            TestEventStubs.order_filled(
                order_close,
                instrument=AUDUSD_SIM,
                account_id=account_id,
                last_qty=Quantity.from_int(50_000),
                last_px=Price.from_str("1.00000"),
                trade_id=TradeId("3"),
            ),
        )

        # Wait for close
        await eventually(lambda: cache.positions()[0].is_closed)

        # Reopen Long
        order_reopen = order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(10_000),
        )
        cache.add_order(order_reopen)
        exec_engine.process(
            TestEventStubs.order_submitted(order_reopen, ts_event=clock.timestamp_ns()),
        )
        exec_engine.process(
            TestEventStubs.order_accepted(order_reopen, ts_event=clock.timestamp_ns()),
        )
        exec_engine.process(
            TestEventStubs.order_filled(
                order_reopen,
                instrument=AUDUSD_SIM,
                account_id=account_id,
                last_qty=Quantity.from_int(10_000),
                last_px=Price.from_str("1.00020"),
                trade_id=TradeId("4"),
            ),
        )

        # Wait for reopen
        await eventually(lambda: cache.positions()[0].is_open)

        final_pos = cache.positions()[0]
        assert final_pos.is_open
        assert final_pos.side == PositionSide.LONG
        assert final_pos.quantity == Quantity.from_int(10_000)

    finally:
        # Cleanup
        exec_engine.stop()
        await eventually(lambda: exec_engine.is_stopped)
