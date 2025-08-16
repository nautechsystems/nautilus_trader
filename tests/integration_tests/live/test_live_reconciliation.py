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

import pytest

from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
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
def fixture_exec_client(msgbus, cache, clock, instrument_provider):
    """
    Create a mock live execution client.
    """
    loop = asyncio.get_event_loop_policy().get_event_loop()
    return MockLiveExecutionClient(
        loop=loop,
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
def fixture_exec_engine(msgbus, cache, clock, exec_client, portfolio):
    """
    Create a live execution engine configured for reconciliation testing.
    """
    loop = asyncio.get_event_loop_policy().get_event_loop()
    exec_engine = LiveExecutionEngine(
        loop=loop,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=LiveExecEngineConfig(
            reconciliation=True,
            inflight_check_interval_ms=100,
            inflight_check_threshold_ms=200,
            inflight_check_retries=2,  # Low retries for testing
            open_check_interval_secs=0.5,
        ),
    )
    exec_engine.register_client(exec_client)
    exec_engine.start()

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


@pytest.mark.asyncio()
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


@pytest.mark.asyncio()
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


@pytest.mark.asyncio()
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


@pytest.mark.asyncio()
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

    # Wait a bit to ensure time has passed
    await asyncio.sleep(0.3)

    # Trigger inflight checks via reconciliation method (public API)
    # Note: Using public reconcile_execution_state() instead of private _check_inflight_orders()
    for _ in range(3):
        # This triggers inflight checks as part of the reconciliation process
        await exec_engine.reconcile_execution_state()

    # Assert - After max retries, order should be rejected
    await eventually(lambda: order.status == OrderStatus.REJECTED, timeout=1.0)
    assert order.status == OrderStatus.REJECTED


@pytest.mark.asyncio()
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


@pytest.mark.asyncio()
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
