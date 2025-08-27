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
Tests for Portfolio functionality with position snapshots and PnL calculations.
"""


from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


def test_portfolio_calculates_realized_pnl_with_position_snapshots(
    portfolio,
    cache,
    exec_engine,
    clock,
    account_id,
):
    """
    Test that portfolio correctly includes position snapshot PnLs in realized PnL.

    Critical for NETTING OMS where positions can close and reopen with same ID.

    """
    # Arrange
    exec_engine.start()

    account_state = AccountState(
        account_id=account_id,
        account_type=AccountType.CASH,
        base_currency=USD,
        reported=True,
        balances=[AccountBalance(Money(1_000_000, USD), Money(0, USD), Money(1_000_000, USD))],
        margins=[],
        info={},
        event_id=UUID4(),
        ts_event=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )
    account = TestExecStubs.cash_account()
    cache.add_account(account)
    portfolio.update_account(account_state)

    # Create and close first position
    order1 = TestExecStubs.market_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
    )

    fill1 = TestEventStubs.order_filled(
        order=order1,
        instrument=AUDUSD_SIM,
        position_id=PositionId("P-001"),
        last_px=Price.from_str("1.00000"),
    )

    position1 = Position(instrument=AUDUSD_SIM, fill=fill1)

    # Close the position
    order2 = TestExecStubs.market_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
    )

    fill2 = TestEventStubs.order_filled(
        order=order2,
        instrument=AUDUSD_SIM,
        position_id=PositionId("P-001"),
        last_px=Price.from_str("1.00100"),  # 10 pips profit
    )

    position1.apply(fill2)

    # Snapshot the closed position (this is what NETTING OMS would do)
    cache.snapshot_position(position1)

    # Create new position with same ID (NETTING OMS behavior)
    order3 = TestExecStubs.market_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(50_000),
    )

    fill3 = TestEventStubs.order_filled(
        order=order3,
        instrument=AUDUSD_SIM,
        position_id=PositionId("P-001"),  # Same ID
        last_px=Price.from_str("1.00200"),
    )

    position2 = Position(instrument=AUDUSD_SIM, fill=fill3)
    cache.add_position(position2, OmsType.NETTING)

    # Act - Calculate realized PnL
    realized_pnl = portfolio.realized_pnl(AUDUSD_SIM.id)

    # Assert - Should include snapshot PnL
    # Expected: 96 USD from snapshot (100 profit - 4 commission) + (-1) from open position = 95 USD
    assert realized_pnl == Money(95.00, USD)  # 96 from snapshot - 1 from current


def test_portfolio_reset_clears_all_state(
    portfolio,
    exec_engine,
    clock,
    account_id,
):
    """
    Test that portfolio.reset() properly clears all internal state.
    """
    # Arrange
    exec_engine.start()

    # Create some state
    account_state = AccountState(
        account_id=account_id,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        reported=True,
        balances=[AccountBalance(Money(1_000_000, USD), Money(0, USD), Money(1_000_000, USD))],
        margins=[],
        info={},
        event_id=UUID4(),
        ts_event=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )
    portfolio.update_account(account_state)

    # Act
    portfolio.reset()

    # Assert
    assert portfolio.is_completely_flat()


def test_portfolio_dispose_releases_resources(
    portfolio,
    exec_engine,
):
    """
    Test that portfolio.dispose() properly releases resources.
    """
    # Arrange
    exec_engine.start()

    # Act
    portfolio.dispose()

    # Assert - Should not raise errors
    assert portfolio.is_completely_flat()


def test_netting_oms_position_lifecycle_with_snapshots(
    portfolio,
    cache,
    exec_engine,
    clock,
    account_id,
):
    """
    Test complete NETTING OMS position lifecycle:
    1. Open position
    2. Close position (creates snapshot)
    3. Reopen with same ID
    4. Verify PnL includes snapshot
    """
    # Arrange
    exec_engine.start()

    account_state = AccountState(
        account_id=account_id,
        account_type=AccountType.CASH,
        base_currency=USD,
        reported=True,
        balances=[AccountBalance(Money(1_000_000, USD), Money(0, USD), Money(1_000_000, USD))],
        margins=[],
        info={},
        event_id=UUID4(),
        ts_event=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )
    account = TestExecStubs.cash_account()
    cache.add_account(account)
    portfolio.update_account(account_state)

    position_id = PositionId("NETTING-001")

    # Phase 1: Open initial position
    order1 = TestExecStubs.market_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
    )

    fill1 = TestEventStubs.order_filled(
        order=order1,
        instrument=AUDUSD_SIM,
        position_id=position_id,
        last_px=Price.from_str("1.00000"),
    )

    initial_position = Position(instrument=AUDUSD_SIM, fill=fill1)
    cache.add_position(initial_position, OmsType.NETTING)

    # Phase 2: Close position (would trigger snapshot in real NETTING OMS)
    order2 = TestExecStubs.market_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
    )

    fill2 = TestEventStubs.order_filled(
        order=order2,
        instrument=AUDUSD_SIM,
        position_id=position_id,
        last_px=Price.from_str("1.00100"),  # 10 pips profit
    )

    initial_position.apply(fill2)

    # Snapshot the closed position (this is what NETTING OMS would do)
    cache.snapshot_position(initial_position)

    # Phase 3: Reopen position with same ID
    order3 = TestExecStubs.market_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(75_000),
    )

    fill3 = TestEventStubs.order_filled(
        order=order3,
        instrument=AUDUSD_SIM,
        position_id=position_id,  # Same ID!
        last_px=Price.from_str("1.00150"),
    )

    reopened_position = Position(instrument=AUDUSD_SIM, fill=fill3)
    cache.add_position(reopened_position, OmsType.NETTING)

    # Act - Calculate total realized PnL
    realized_pnl = portfolio.realized_pnl(AUDUSD_SIM.id)

    # Assert - Should include both snapshot and current position PnL
    # Expected: snapshot PnL (96 USD) + current position PnL (-1.50 USD) = 94.50 USD
    assert realized_pnl == Money(94.50, USD)

    # Verify snapshot exists
    snapshots = cache.position_snapshots()
    assert len(snapshots) == 1
    assert snapshots[0].realized_pnl == Money(96.00, USD)  # 100 profit - 4 commission


def test_pnl_aggregation_multiple_position_cycles(
    portfolio,
    cache,
    exec_engine,
    clock,
    account_id,
):
    """
    Test PnL aggregation across multiple position open-flat-reopen cycles.

    This test validates that:
    1. Each position cycle tracks its own realized PnL independently
    2. Portfolio correctly aggregates PnL from all cycles using snapshots
    3. Reports sum PnL correctly across all position cycles

    This is the intended behavior for handling position cycles in NETTING OMS.

    """
    # Arrange
    exec_engine.start()

    account_state = AccountState(
        account_id=account_id,
        account_type=AccountType.CASH,
        base_currency=USD,
        reported=True,
        balances=[AccountBalance(Money(1_000_000, USD), Money(0, USD), Money(1_000_000, USD))],
        margins=[],
        info={},
        event_id=UUID4(),
        ts_event=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )
    account = TestExecStubs.cash_account()
    cache.add_account(account)
    portfolio.update_account(account_state)

    position_id = PositionId("MULTI-CYCLE-001")
    cycle_pnls = []

    # Cycle 1: Long position with profit
    order1_open = TestExecStubs.market_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
    )

    fill1_open = TestEventStubs.order_filled(
        order=order1_open,
        instrument=AUDUSD_SIM,
        position_id=position_id,
        last_px=Price.from_str("0.80000"),
    )

    position = Position(instrument=AUDUSD_SIM, fill=fill1_open)
    cache.add_position(position, OmsType.NETTING)

    # Close Cycle 1
    order1_close = TestExecStubs.market_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
    )

    fill1_close = TestEventStubs.order_filled(
        order=order1_close,
        instrument=AUDUSD_SIM,
        position_id=position_id,
        last_px=Price.from_str("0.80020"),  # 20 pips profit
    )

    position.apply(fill1_close)
    cycle_pnls.append(position.realized_pnl)

    # Snapshot Cycle 1 (simulating NETTING OMS behavior)
    cache.snapshot_position(position)

    # Cycle 2: Reopen long position with loss
    order2_open = TestExecStubs.market_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(150_000),
    )

    fill2_open = TestEventStubs.order_filled(
        order=order2_open,
        instrument=AUDUSD_SIM,
        position_id=position_id,
        last_px=Price.from_str("0.80030"),
    )

    position2 = Position(instrument=AUDUSD_SIM, fill=fill2_open)
    cache.add_position(position2, OmsType.NETTING)

    # Close Cycle 2
    order2_close = TestExecStubs.market_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(150_000),
    )

    fill2_close = TestEventStubs.order_filled(
        order=order2_close,
        instrument=AUDUSD_SIM,
        position_id=position_id,
        last_px=Price.from_str("0.80020"),  # 10 pips loss
    )

    position2.apply(fill2_close)
    cycle_pnls.append(position2.realized_pnl)

    # Snapshot Cycle 2
    cache.snapshot_position(position2)

    # Cycle 3: Short position with profit
    order3_open = TestExecStubs.market_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(200_000),
    )

    fill3_open = TestEventStubs.order_filled(
        order=order3_open,
        instrument=AUDUSD_SIM,
        position_id=position_id,
        last_px=Price.from_str("0.80015"),
    )

    position3 = Position(instrument=AUDUSD_SIM, fill=fill3_open)
    cache.add_position(position3, OmsType.NETTING)

    # Close Cycle 3
    order3_close = TestExecStubs.market_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(200_000),
    )

    fill3_close = TestEventStubs.order_filled(
        order=order3_close,
        instrument=AUDUSD_SIM,
        position_id=position_id,
        last_px=Price.from_str("0.80005"),  # 10 pips profit
    )

    position3.apply(fill3_close)
    cycle_pnls.append(position3.realized_pnl)

    # Act
    # Calculate total realized PnL from portfolio (should aggregate all snapshots)
    total_realized_pnl = portfolio.realized_pnl(AUDUSD_SIM.id)

    # Get all position snapshots
    snapshots = cache.position_snapshots()

    # Assert - each cycle's PnL is independent
    assert cycle_pnls[0] == Money(16.80, USD)  # Cycle 1: 20 pips on 100k - 3.20 commission
    assert cycle_pnls[1] == Money(-19.80, USD)  # Cycle 2: -15 pips on 150k - 4.80 commission
    assert cycle_pnls[2] == Money(13.60, USD)  # Cycle 3: 10 pips on 200k - 6.40 commission

    # Verify snapshots preserve each cycle
    assert len(snapshots) == 2  # First 2 cycles are snapshotted
    assert snapshots[0].realized_pnl == cycle_pnls[0]
    assert snapshots[1].realized_pnl == cycle_pnls[1]

    # Verify portfolio aggregates all cycles correctly
    expected_total_pnl = Money(10.60, USD)  # 16.80 - 19.80 + 13.60 = 10.60
    assert total_realized_pnl == expected_total_pnl

    # Generate positions report to verify aggregation
    from nautilus_trader.analysis.reporter import ReportProvider

    positions = [position3]  # Current active or last closed position
    report = ReportProvider.generate_positions_report(positions, snapshots)

    # Verify report includes all cycles
    assert len(report) == 3  # 2 snapshots + 1 current

    # Sum realized PnL from report using Money objects for robust parsing
    from decimal import Decimal

    report_total_pnl = Decimal(0)
    for pnl_str in report["realized_pnl"]:
        pnl_money = Money.from_str(pnl_str)
        report_total_pnl += pnl_money.as_decimal()

    assert report_total_pnl == expected_total_pnl.as_decimal()
