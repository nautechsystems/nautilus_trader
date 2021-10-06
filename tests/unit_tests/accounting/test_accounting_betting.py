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
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.currencies import JPY
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit.providers import TestInstrumentProvider
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
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    USD,
                    Money(1_000_000, USD),
                    Money(0, USD),
                    Money(1_000_000, USD),
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
        assert account.base_currency == USD
        assert account.last_event == event
        assert account.events == [event]
        assert account.event_count == 1
        assert account.balance_total() == Money(1_000_000, USD)
        assert account.balance_free() == Money(1_000_000, USD)
        assert account.balance_locked() == Money(0, USD)
        assert account.balances_total() == {USD: Money(1_000_000, USD)}
        assert account.balances_free() == {USD: Money(1_000_000, USD)}
        assert account.balances_locked() == {USD: Money(0, USD)}

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

    def test_calculate_balance_locked_buy(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.BETTING,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    USD,
                    Money(1_000.00, USD),
                    Money(0.00, USD),
                    Money(1_000.00, USD),
                ),
            ],
            info={},  # No default currency set
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        account = BettingAccount(event)

        # Act
        result = account.calculate_balance_locked(
            instrument=self.instrument,
            side=OrderSide.BUY,
            quantity=Quantity.from_int(1),
            price=Price.from_str("0.80"),
        )

        # Assert
        assert result == Money(800_032.00, USD)  # Notional + expected commission

    def test_calculate_balance_locked_sell(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.BETTING,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    USD,
                    Money(1_000_000.00, USD),
                    Money(0.00, USD),
                    Money(1_000_000.00, USD),
                ),
            ],
            info={},  # No default currency set
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        account = BettingAccount(event)

        # Act
        result = account.calculate_balance_locked(
            instrument=self.instrument,
            side=OrderSide.SELL,
            quantity=Quantity.from_int(1_000_000),
            price=Price.from_str("0.80"),
        )

        # Assert
        assert result == Money(1_000_040.00, AUD)  # Notional + expected commission

    def test_calculate_pnls_for_single_currency_cash_account(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.BETTING,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    USD,
                    Money(1_000_000.00, USD),
                    Money(0.00, USD),
                    Money(1_000_000.00, USD),
                ),
            ],
            info={},  # No default currency set
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        account = BettingAccount(event)

        order = self.order_factory.market(
            self.instrument.id,
            OrderSide.BUY,
            Quantity.from_int(1_000_000),
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
        assert result == [Money(-800016.00, USD)]

    def test_calculate_commission_when_given_liquidity_side_none_raises_value_error(
        self,
    ):
        # Arrange
        account = TestStubs.cash_account()
        instrument = TestInstrumentProvider.xbtusd_bitmex()

        # Act, Assert
        with pytest.raises(ValueError):
            account.calculate_commission(
                instrument=instrument,
                last_qty=Quantity.from_int(100000),
                last_px=Decimal("11450.50"),
                liquidity_side=LiquiditySide.NONE,
            )

    def test_calculate_commission_for_taker_fx(self):
        # Arrange
        account = TestStubs.cash_account()
        instrument = self.instrument

        # Act
        result = account.calculate_commission(
            instrument=instrument,
            last_qty=Quantity.from_int(1500000),
            last_px=Decimal("0.80050"),
            liquidity_side=LiquiditySide.TAKER,
        )

        # Assert
        assert result == Money(24.02, USD)

    def test_calculate_commission_crypto_taker(self):
        # Arrange
        account = TestStubs.cash_account()
        instrument = TestInstrumentProvider.xbtusd_bitmex()

        # Act
        result = account.calculate_commission(
            instrument=instrument,
            last_qty=Quantity.from_int(100000),
            last_px=Decimal("11450.50"),
            liquidity_side=LiquiditySide.TAKER,
        )

        # Assert
        assert result == Money(0.00654993, GBP)

    def test_calculate_commission_fx_taker(self):
        # Arrange
        account = TestStubs.cash_account()
        instrument = TestInstrumentProvider.default_fx_ccy("USD/JPY", Venue("IDEALPRO"))

        # Act
        result = account.calculate_commission(
            instrument=instrument,
            last_qty=Quantity.from_int(2200000),
            last_px=Decimal("120.310"),
            liquidity_side=LiquiditySide.TAKER,
        )

        # Assert
        assert result == Money(5294, JPY)
