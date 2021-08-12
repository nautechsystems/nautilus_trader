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

from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit.stubs import TestStubs


class TestBettingAccount:
    def setup(self):
        self.account = TestStubs.margin_account()  # TODO(bm): Implement betting account
        self.instrument = BetfairTestStubs.betting_instrument()

    def test_betting_instrument_notional_value(self):
        notional = self.instrument.notional_value(
            quantity=Quantity.from_int(100),
            price=Price.from_str("0.5").as_decimal(),
            inverse_as_quote=False,
        ).as_decimal()
        # We are long 100 at 0.5 probability, aka 2.0 in odds terms
        assert notional == Decimal("200.0")

    def test_calculate_margin_initial(self):
        # Arrange
        result = self.account.calculate_margin_initial(
            instrument=self.instrument,
            quantity=Quantity.from_int(100),
            price=Price.from_str("0.5"),
        )

        # Assert
        assert result == Money("200.00", GBP)

    def test_calculate_maintenance_margin(self):
        # Arrange
        long = self.account.calculate_margin_maint(
            instrument=self.instrument,
            side=PositionSide.LONG,
            quantity=Quantity.from_int(100),
            last=Price.from_str("0.4"),
        )
        short = self.account.calculate_margin_maint(
            instrument=self.instrument,
            side=PositionSide.SHORT,
            quantity=Quantity.from_int(100),
            last=Price.from_str("0.8"),
        )
        very_short = self.account.calculate_margin_maint(
            instrument=self.instrument,
            side=PositionSide.SHORT,
            quantity=Quantity.from_int(100),
            last=Price.from_str("0.1"),
        )

        # Assert
        assert long == Money(250.00, GBP)
        assert short == Money(125.00, GBP)
        assert very_short == Money(1000.00, GBP)
