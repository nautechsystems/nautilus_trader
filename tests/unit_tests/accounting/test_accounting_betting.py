# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal

import pytest

from nautilus_trader.accounting.accounts.betting import BettingAccount
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.c_enums.order_side import OrderSideParser
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit.stubs import TestStubs


class TestBettingAccount:
    def setup(self):
        # Fixture Setup
        self.trader_id = TestStubs.trader_id()
        self.instrument = BetfairTestStubs.betting_instrument()
        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

    @staticmethod
    def _make_account_state(starting_balance: float):
        return AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.BETTING,
            base_currency=GBP,
            reported=True,
            balances=[
                AccountBalance(
                    GBP,
                    Money(starting_balance, GBP),
                    Money(0.00, GBP),
                    Money(starting_balance, GBP),
                ),
            ],
            info={},  # No default currency set
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

    def test_instantiated_accounts_basic_properties(self):
        # Arrange, Act
        account = TestStubs.betting_account()

        # Assert
        assert account == account
        assert not account != account
        assert account.id == AccountId("SIM", "000")
        assert str(account) == "BettingAccount(id=SIM-000, type=BETTING, base=GBP)"
        assert repr(account) == "BettingAccount(id=SIM-000, type=BETTING, base=GBP)"
        assert isinstance(hash(account), int)

    def test_instantiate_single_asset_cash_account(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "000"),
            account_type=AccountType.BETTING,
            base_currency=GBP,
            reported=True,
            balances=[
                AccountBalance(
                    GBP,
                    Money(1_000_000, GBP),
                    Money(0, GBP),
                    Money(1_000_000, GBP),
                ),
            ],
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
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.BETTING,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    GBP,
                    Money(10.00000000, GBP),
                    Money(0.00000000, GBP),
                    Money(10.00000000, GBP),
                ),
            ],
            info={},  # No default currency set
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act
        account = BettingAccount(event1)

        event2 = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.BETTING,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    GBP,
                    Money(9.00000000, GBP),
                    Money(0.50000000, GBP),
                    Money(8.50000000, GBP),
                ),
            ],
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
        "price, quantity, side, locked_balance",
        [
            ("0.80", 10, "BUY", "10"),
            ("0.50", 10, "BUY", "10"),
            ("0.10", 20, "BUY", "20"),
            ("0.80", 10, "SELL", "2.5"),
            ("0.5", 10, "SELL", "10"),
            ("0.1", 10, "SELL", "90"),
        ],
    )
    def test_calculate_balance_locked(self, price, quantity, side, locked_balance):
        # Arrange
        event = self._make_account_state(starting_balance=1000.0)
        account = BettingAccount(event)

        # Act
        result = account.calculate_balance_locked(
            instrument=self.instrument,
            side=OrderSideParser.from_str_py(side),
            quantity=Quantity.from_int(quantity),
            price=Price.from_str(price),
        )

        # Assert
        assert result == Money(Price.from_str(locked_balance), GBP)

    def test_calculate_pnls_for_single_currency_cash_account(self):
        # Arrange
        event = self._make_account_state(starting_balance=1000.0)
        account = BettingAccount(event)

        order = self.order_factory.market(
            self.instrument.id,
            OrderSide.BUY,
            Quantity.from_int(100),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=self.instrument,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("0.80000"),
        )

        position = Position(self.instrument, fill)

        # Act
        result = account.calculate_pnls(
            instrument=self.instrument,
            position=position,
            fill=fill,
        )

        # Assert
        assert result == [Money("-80.00", GBP)]

    def test_calculate_commission_when_given_liquidity_side_none_raises_value_error(
        self,
    ):
        # Arrange
        account = TestStubs.cash_account()

        # Act, Assert
        with pytest.raises(ValueError):
            account.calculate_commission(
                instrument=self.instrument,
                last_qty=Quantity.from_int(1),
                last_px=Decimal("1"),
                liquidity_side=LiquiditySide.NONE,
            )
