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

from decimal import Decimal

import pytest

from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
ADABTC_BINANCE = TestInstrumentProvider.adabtc_binance()
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()


class TestMarginAccount:
    def setup(self):
        # Fixture Setup
        self.trader_id = TestIdStubs.trader_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

    def test_instantiated_accounts_basic_properties(self):
        # Arrange, Act
        account = TestExecStubs.margin_account()

        # Assert
        assert account.id == AccountId("SIM-000")
        assert str(account) == "MarginAccount(id=SIM-000, type=MARGIN, base=USD)"
        assert repr(account) == "MarginAccount(id=SIM-000, type=MARGIN, base=USD)"
        assert isinstance(hash(account), int)
        assert account == account
        assert account == account
        assert account.default_leverage == Decimal(1)

    def test_set_default_leverage(self):
        # Arrange
        account = TestExecStubs.margin_account()

        # Act
        account.set_default_leverage(Decimal(100))

        # Assert
        assert account.default_leverage == Decimal(100)
        assert account.leverages() == {}

    def test_set_leverage(self):
        # Arrange
        account = TestExecStubs.margin_account()

        # Act
        account.set_leverage(AUDUSD_SIM.id, Decimal(100))

        # Assert
        assert account.leverage(AUDUSD_SIM.id) == Decimal(100)
        assert account.leverages() == {AUDUSD_SIM.id: Decimal(100)}

    def test_is_unleveraged_with_leverage_returns_false(self):
        # Arrange
        account = TestExecStubs.margin_account()

        # Act
        account.set_leverage(AUDUSD_SIM.id, Decimal(100))

        # Assert
        assert not account.is_unleveraged(AUDUSD_SIM.id)

    def test_is_unleveraged_with_no_leverage_returns_true(self):
        # Arrange
        account = TestExecStubs.margin_account()

        # Act
        account.set_leverage(AUDUSD_SIM.id, Decimal(1))

        # Assert
        assert account.is_unleveraged(AUDUSD_SIM.id)

    def test_is_unleveraged_with_default_leverage_of_1_returns_true(self):
        # Arrange
        account = TestExecStubs.margin_account()

        # Act, Assert
        assert account.is_unleveraged(AUDUSD_SIM.id)

    def test_update_margin_init(self):
        # Arrange
        account = TestExecStubs.margin_account()
        margin = Money(1_000.00, USD)

        # Act
        account.update_margin_init(AUDUSD_SIM.id, margin)

        # Assert
        assert account.margin_init(AUDUSD_SIM.id) == margin
        assert account.margins_init() == {AUDUSD_SIM.id: margin}

    def test_update_margin_maint(self):
        # Arrange
        account = TestExecStubs.margin_account()
        margin = Money(1_000.00, USD)

        # Act
        account.update_margin_maint(AUDUSD_SIM.id, margin)

        # Assert
        assert account.margin_maint(AUDUSD_SIM.id) == margin
        assert account.margins_maint() == {AUDUSD_SIM.id: margin}

    def test_calculate_margin_init_with_leverage(self):
        # Arrange
        account = TestExecStubs.margin_account()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        account.set_leverage(instrument.id, Decimal(50))

        result = account.calculate_margin_init(
            instrument=instrument,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("0.80000"),
        )

        # Assert
        assert result == Money(48.00, USD)

    def test_calculate_margin_init_with_default_leverage(self):
        # Arrange
        account = TestExecStubs.margin_account()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        account.set_default_leverage(Decimal(10))

        result = account.calculate_margin_init(
            instrument=instrument,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("0.80000"),
        )

        # Assert
        assert result == Money(240.00, USD)

    @pytest.mark.parametrize(
        ("use_quote_for_inverse", "expected"),
        [
            [False, Money(0.08700494, BTC)],
            [True, Money(1000.00, USD)],
        ],
    )
    def test_calculate_margin_init_with_no_leverage_for_inverse(
        self,
        use_quote_for_inverse,
        expected,
    ):
        # Arrange
        account = TestExecStubs.margin_account()
        instrument = TestInstrumentProvider.xbtusd_bitmex()

        result = account.calculate_margin_init(
            instrument=instrument,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("11493.60"),
            use_quote_for_inverse=use_quote_for_inverse,
        )

        # Assert
        assert result == expected

    def test_calculate_margin_maint_with_no_leverage(self):
        # Arrange
        account = TestExecStubs.margin_account()
        instrument = TestInstrumentProvider.xbtusd_bitmex()

        # Act
        result = account.calculate_margin_maint(
            instrument=instrument,
            side=PositionSide.LONG,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("11493.60"),
        )

        # Assert
        assert result == Money(0.03045173, BTC)

    def test_calculate_margin_maint_with_leverage_fx_instrument(self):
        # Arrange
        account = TestExecStubs.margin_account()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        account.set_default_leverage(Decimal(50))

        # Act
        result = account.calculate_margin_maint(
            instrument=instrument,
            side=PositionSide.LONG,
            quantity=Quantity.from_int(1_000_000),
            price=Price.from_str("1.00000"),
        )

        # Assert
        assert result == Money(600.00, USD)

    def test_calculate_margin_maint_with_leverage_inverse_instrument(self):
        # Arrange
        account = TestExecStubs.margin_account()
        instrument = TestInstrumentProvider.xbtusd_bitmex()
        account.set_default_leverage(Decimal(10))

        # Act
        result = account.calculate_margin_maint(
            instrument=instrument,
            side=PositionSide.LONG,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("100000.00"),
        )

        # Assert
        assert result == Money(0.00035000, BTC)
