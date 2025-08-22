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
Extended tests for Portfolio functionality, especially position snapshots and PnL
calculations.
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
    # Arrange - Setup account
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
    cache.add_account(TestExecStubs.cash_account())
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
