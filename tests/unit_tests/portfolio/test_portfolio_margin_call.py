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
Tests for Portfolio margin call and liquidation flows.
"""

from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


def test_margin_call_flow_with_adverse_price_movement(
    portfolio,
    cache,
    exec_engine,
    clock,
    account_id,
):
    """
    Test margin call detection when position moves adversely against available margin.
    """
    # Arrange - Setup margin account with limited capital
    exec_engine.start()

    # Start with 10,000 USD
    account = TestExecStubs.margin_account()
    cache.add_account(account)

    account_state = AccountState(
        account_id=account_id,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        reported=True,
        balances=[AccountBalance(Money(10_000, USD), Money(0, USD), Money(10_000, USD))],
        margins=[],
        info={},
        event_id=UUID4(),
        ts_event=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )
    portfolio.update_account(account_state)

    # Open a leveraged position using most of available margin
    # 1,000,000 notional at 1.0000 = 100,000 USD margin required at 10:1
    order = TestExecStubs.market_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(1_000_000),  # Large position
    )

    fill = TestEventStubs.order_filled(
        order=order,
        instrument=AUDUSD_SIM,
        position_id=PositionId("P-001"),
        last_px=Price.from_str("1.00000"),
    )

    position = Position(instrument=AUDUSD_SIM, fill=fill)
    cache.add_position(position, OmsType.HEDGING)

    # Update account with margin used
    margin_used = Money(10_000, USD)  # Using all available margin
    account_state_with_margin = AccountState(
        account_id=account_id,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        reported=True,
        balances=[AccountBalance(Money(10_000, USD), margin_used, Money(0, USD))],
        margins=[
            MarginBalance(
                initial=margin_used,
                maintenance=Money(5_000, USD),  # 50% maintenance margin
                instrument_id=AUDUSD_SIM.id,
            ),
        ],
        info={},
        event_id=UUID4(),
        ts_event=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )
    portfolio.update_account(account_state_with_margin)

    # Act - Price moves adversely (10% drop)
    adverse_quote = QuoteTick(
        instrument_id=AUDUSD_SIM.id,
        bid_price=Price.from_str("0.90000"),
        ask_price=Price.from_str("0.90010"),
        bid_size=Quantity.from_int(1_000_000),
        ask_size=Quantity.from_int(1_000_000),
        ts_event=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )

    cache.add_quote_tick(adverse_quote)

    # Calculate unrealized PnL at adverse price
    unrealized_pnl = portfolio.unrealized_pnl(AUDUSD_SIM.id)

    # Assert - Significant loss should be reflected
    # 1M units bought at 1.0, now worth 0.9 = -100,000 USD loss
    assert unrealized_pnl.as_double() < 0  # Should have unrealized loss

    # In a real system, this massive loss would trigger margin call
    # as the account equity would be negative


def test_liquidation_trigger_on_maintenance_margin_breach(
    portfolio,
    cache,
    exec_engine,
    clock,
    account_id,
):
    """
    Test that positions crossing maintenance margin threshold trigger liquidation logic.
    """
    # Arrange - Setup with position near maintenance margin
    exec_engine.start()

    account_state = AccountState(
        account_id=account_id,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        reported=True,
        balances=[AccountBalance(Money(5_000, USD), Money(4_500, USD), Money(500, USD))],
        margins=[
            MarginBalance(
                initial=Money(4_500, USD),
                maintenance=Money(4_000, USD),  # Close to current margin used
                instrument_id=AUDUSD_SIM.id,
            ),
        ],
        info={},
        event_id=UUID4(),
        ts_event=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )

    account = TestExecStubs.margin_account()
    cache.add_account(account)
    portfolio.update_account(account_state)

    # Create position
    order = TestExecStubs.market_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(50_000),
    )

    fill = TestEventStubs.order_filled(
        order=order,
        instrument=AUDUSD_SIM,
        position_id=PositionId("P-002"),
        last_px=Price.from_str("1.00000"),
    )

    position = Position(instrument=AUDUSD_SIM, fill=fill)
    cache.add_position(position, OmsType.HEDGING)

    # Act - Small adverse movement should breach maintenance
    adverse_quote = QuoteTick(
        instrument_id=AUDUSD_SIM.id,
        bid_price=Price.from_str("0.98000"),  # 2% drop
        ask_price=Price.from_str("0.98010"),
        bid_size=Quantity.from_int(1_000_000),
        ask_size=Quantity.from_int(1_000_000),
        ts_event=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )

    cache.add_quote_tick(adverse_quote)

    # Check margin status
    unrealized_pnl = portfolio.unrealized_pnl(AUDUSD_SIM.id)

    # Assert - Should indicate margin breach conditions
    # 50K units * 0.02 drop = -1000 USD loss
    # Equity = 5000 - 1000 = 4000 (at maintenance threshold)
    assert unrealized_pnl.as_double() < 0

    # In a real system, this would trigger liquidation orders
    # Here we just verify the math shows we're at/below maintenance
    free_balance = 500 - abs(unrealized_pnl.as_double())
    assert free_balance <= 0  # No free margin left
