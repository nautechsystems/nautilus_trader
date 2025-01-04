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

from nautilus_trader.model.currencies import USD
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.risk.sizing import FixedRiskSizer
from nautilus_trader.risk.sizing import PositionSizer
from nautilus_trader.test_kit.providers import TestInstrumentProvider


USDJPY = TestInstrumentProvider.default_fx_ccy("GBP/USD")


class TestPositionSizer:
    def test_update_instrument(self):
        # Arrange
        sizer = PositionSizer(USDJPY)

        # Act
        sizer.update_instrument(USDJPY)

        # Assert
        assert True  # No exceptions raised


class TestFixedRiskSizer:
    def setup(self):
        # Fixture Setup
        self.sizer = FixedRiskSizer(USDJPY)

    def test_calculate_with_zero_equity_returns_quantity_zero(self):
        # Arrange
        equity = Money(0, USD)  # No equity

        # Act
        result = self.sizer.calculate(
            entry=Price.from_str("1.00100"),
            stop_loss=Price.from_str("1.00000"),
            equity=equity,
            risk=Decimal("0.001"),  # 0.1%
            unit_batch_size=Decimal(1000),
        )

        # Assert
        assert result == Quantity.zero()

    def test_calculate_with_zero_exchange_rate_returns_quantity_zero(self):
        # Arrange
        equity = Money(0, USD)  # No equity

        # Act
        result = self.sizer.calculate(
            entry=Price.from_str("1.00100"),
            stop_loss=Price.from_str("1.00000"),
            equity=equity,
            risk=Decimal("0.001"),  # 0.1%
            exchange_rate=Decimal("0"),
        )

        # Assert
        assert result == Quantity.zero()

    def test_calculate_with_zero_risk_returns_quantity_zero(self):
        # Arrange
        equity = Money(0, USD)  # No equity

        # Act
        result = self.sizer.calculate(
            entry=Price.from_str("1.00100"),
            stop_loss=Price.from_str("1.00100"),
            equity=equity,
            risk=Decimal("0.001"),  # 0.1%
            exchange_rate=Decimal("0"),
        )

        # Assert
        assert result == Quantity.zero()

    def test_calculate_single_unit_size(self):
        # Arrange
        equity = Money(1_000_000, USD)

        # Act
        result = self.sizer.calculate(
            entry=Price.from_str("1.00100"),
            stop_loss=Price.from_str("1.00000"),
            equity=equity,
            risk=Decimal("0.001"),  # 0.1%
            unit_batch_size=Decimal(1000),
        )

        # Assert
        assert result == Quantity.from_int(1_000_000)

    def test_calculate_single_unit_with_exchange_rate(self):
        # Arrange
        equity = Money(1_000_000, USD)

        # Act
        result = self.sizer.calculate(
            entry=Price.from_str("110.010"),
            stop_loss=Price.from_str("110.000"),
            equity=equity,
            risk=Decimal("0.001"),  # 1%
            exchange_rate=Decimal(str(1 / 110)),
        )

        # Assert
        assert result == Quantity.from_int(10_000_000)

    def test_calculate_single_unit_size_when_risk_too_high(self):
        # Arrange
        equity = Money(100000, USD)

        # Act
        result = self.sizer.calculate(
            entry=Price.from_str("3.00000"),
            stop_loss=Price.from_str("1.00000"),
            equity=equity,
            risk=Decimal("0.01"),  # 1%
            unit_batch_size=Decimal(1000),
        )

        # Assert
        assert result == Quantity.zero()

    def test_impose_hard_limit(self):
        # Arrange
        equity = Money(1_000_000, USD)

        # Act
        result = self.sizer.calculate(
            entry=Price.from_str("1.00010"),
            stop_loss=Price.from_str("1.00000"),
            equity=equity,
            risk=Decimal("0.01"),  # 1%
            hard_limit=Decimal(500000),
            unit_batch_size=Decimal(1000),
            units=1,
        )

        # Assert
        assert result == Quantity.from_int(500_000)

    def test_calculate_multiple_unit_size(self):
        # Arrange
        equity = Money(1_000_000, USD)

        # Act
        result = self.sizer.calculate(
            entry=Price.from_str("1.00010"),
            stop_loss=Price.from_str("1.00000"),
            equity=equity,
            risk=Decimal("0.001"),  # 0.1%
            unit_batch_size=Decimal(1000),
            units=3,
        )

        # Assert
        assert result == Quantity.from_int(3_333_000)

    def test_calculate_multiple_unit_size_larger_batches(self):
        # Arrange
        equity = Money(1_000_000, USD)

        # Act
        result = self.sizer.calculate(
            entry=Price.from_str("1.00087"),
            stop_loss=Price.from_str("1.00000"),
            equity=equity,
            risk=Decimal("0.001"),  # 0.1%
            unit_batch_size=Decimal(25000),
            units=4,
        )

        # Assert
        assert result == Quantity.from_int(275000)

    def test_calculate_for_usdjpy_with_commission(self):
        # Arrange
        sizer = FixedRiskSizer(TestInstrumentProvider.default_fx_ccy("USD/JPY"))
        equity = Money(1_000_000, USD)

        # Act
        result = sizer.calculate(
            entry=Price.from_str("107.703"),
            stop_loss=Price.from_str("107.403"),
            equity=equity,
            risk=Decimal("0.01"),  # 1%
            commission_rate=Decimal("0.0002"),
            exchange_rate=Decimal(str(1 / 107.403)),
            unit_batch_size=Decimal(1000),
            units=1,
        )

        # Assert
        assert result == Quantity.from_int(3_578_000)
