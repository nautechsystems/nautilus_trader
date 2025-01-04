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

import pytest
from betfair_parser.spec.common import OrderSide as BetSide

from nautilus_trader.accounting.accounts.betting import BettingAccount
from nautilus_trader.adapters.betfair.parsing.common import bet_side_to_order_side
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.betfair.test_kit import betting_instrument


class TestBettingAccount:
    def setup(self):
        # Fixture Setup
        self.trader_id = TestIdStubs.trader_id()
        self.instrument = betting_instrument()
        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

    @staticmethod
    def _make_account_state(starting_balance: float):
        return AccountState(
            account_id=AccountId("SIM-001"),
            account_type=AccountType.BETTING,
            base_currency=GBP,
            reported=True,
            balances=[
                AccountBalance(
                    Money(starting_balance, GBP),
                    Money(0.00, GBP),
                    Money(starting_balance, GBP),
                ),
            ],
            margins=[],
            info={},  # No default currency set
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

    def _make_fill(self, price="0.5", volume=10, side="BUY", position_id="P-123456"):
        order = self.order_factory.market(
            self.instrument.id,
            getattr(OrderSide, side),
            Quantity.from_int(volume),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=self.instrument,
            position_id=PositionId(position_id),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str(price),
        )
        return fill

    def test_instantiated_accounts_basic_properties(self):
        # Arrange, Act
        account = TestExecStubs.betting_account()

        # Assert
        assert account == account
        assert account == account
        assert account.id == AccountId("SIM-000")
        assert str(account) == "BettingAccount(id=SIM-000, type=BETTING, base=GBP)"
        assert repr(account) == "BettingAccount(id=SIM-000, type=BETTING, base=GBP)"
        assert isinstance(hash(account), int)

    def test_instantiate_single_asset_cash_account(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM-000"),
            account_type=AccountType.BETTING,
            base_currency=GBP,
            reported=True,
            balances=[
                AccountBalance(
                    Money(1_000_000, GBP),
                    Money(0, GBP),
                    Money(1_000_000, GBP),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act
        account = BettingAccount(event)

        # Assert
        assert account.base_currency == GBP
        assert account.last_event == event
        assert account.events == [event]
        assert account.event_count == 1
        assert account.balance_total() == Money(1_000_000, GBP)
        assert account.balance_free() == Money(1_000_000, GBP)
        assert account.balance_locked() == Money(0, GBP)
        assert account.balances_total() == {GBP: Money(1_000_000, GBP)}
        assert account.balances_free() == {GBP: Money(1_000_000, GBP)}
        assert account.balances_locked() == {GBP: Money(0, GBP)}

    def test_apply_given_new_state_event_updates_correctly(self):
        # Arrange
        event1 = AccountState(
            account_id=AccountId("SIM-001"),
            account_type=AccountType.BETTING,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    Money(10.00000000, GBP),
                    Money(0.00000000, GBP),
                    Money(10.00000000, GBP),
                ),
            ],
            margins=[],
            info={},  # No default currency set
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act
        account = BettingAccount(event1)

        event2 = AccountState(
            account_id=AccountId("SIM-001"),
            account_type=AccountType.BETTING,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    Money(9.00000000, GBP),
                    Money(0.50000000, GBP),
                    Money(8.50000000, GBP),
                ),
            ],
            margins=[],
            info={},  # No default currency set
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act
        account.apply(event=event2)

        # Assert
        assert account.last_event == event2
        assert account.events == [event1, event2]
        assert account.event_count == 2
        assert account.balance_total(GBP) == Money(9.00000000, GBP)
        assert account.balance_free(GBP) == Money(8.50000000, GBP)
        assert account.balance_locked(GBP) == Money(0.50000000, GBP)

    @pytest.mark.parametrize(
        ("price", "quantity", "side", "locked_balance"),
        [
            ("1.60", 10, BetSide.BACK, "10"),
            ("2.00", 10, BetSide.BACK, "10"),
            ("10.0", 20, BetSide.BACK, "20"),
            ("1.25", 10, BetSide.LAY, "2.5"),
            ("2.0", 10, BetSide.LAY, "10"),
            ("10.0", 10, BetSide.LAY, "90"),
        ],
    )
    def test_calculate_balance_locked(self, price, quantity, side, locked_balance):
        # Arrange
        event = self._make_account_state(starting_balance=1000.0)
        account = BettingAccount(event)

        # Act
        result = account.calculate_balance_locked(
            instrument=self.instrument,
            side=bet_side_to_order_side(side),
            quantity=Quantity.from_int(quantity),
            price=Price.from_str(price),
        )

        # Assert
        assert result == Money(Price.from_str(locked_balance), GBP)

    def test_calculate_pnls_for_single_currency_cash_account(self):
        # Arrange
        event = self._make_account_state(starting_balance=1000.0)
        account = BettingAccount(event)
        fill = self._make_fill(price="0.8", volume=100)
        position = Position(self.instrument, fill)

        # Act
        result = account.calculate_pnls(
            instrument=self.instrument,
            fill=fill,
            position=position,
        )

        # Assert
        assert result == [Money("-80.00", GBP)]

    def test_calculate_pnls_partially_closed(self):
        # Arrange
        event = self._make_account_state(starting_balance=1000.0)
        account = BettingAccount(event)
        fill1 = self._make_fill(price="0.50", volume=100, side="BUY")
        fill2 = self._make_fill(price="0.80", volume=50, side="SELL")

        position = Position(self.instrument, fill1)

        # Act
        result = account.calculate_pnls(
            instrument=self.instrument,
            fill=fill2,
            position=position,
        )

        # Assert
        assert result == [Money("40.00", GBP)]

    def test_calculate_commission_when_given_liquidity_side_none_raises_value_error(
        self,
    ):
        # Arrange
        account = TestExecStubs.cash_account()

        # Act, Assert
        with pytest.raises(ValueError):
            account.calculate_commission(
                instrument=self.instrument,
                last_qty=Quantity.from_int(1),
                last_px=Price.from_str("1"),
                liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
            )

    @pytest.mark.parametrize(
        "side, price, quantity, expected",
        [
            (BetSide.BACK, 5.0, 100.0, -100),
            (BetSide.BACK, 1.50, 100.0, -100),
            (BetSide.LAY, 5.0, 100.0, -400),
            (BetSide.LAY, 1.5, 100.0, -50),
            (BetSide.LAY, 5.0, 300.0, -1200),
            (BetSide.LAY, 10.0, 100.0, -900),
        ],
    )
    def test_balance_impact(self, side, price, quantity, expected):
        # Arrange
        account = TestExecStubs.betting_account()
        instrument = self.instrument

        # Act
        impact = account.balance_impact(
            instrument=instrument,
            quantity=Quantity(quantity, instrument.size_precision),
            price=Price(price, instrument.price_precision),
            order_side=bet_side_to_order_side(side),
        )

        # Assert
        assert impact == Money(expected, impact.currency)
