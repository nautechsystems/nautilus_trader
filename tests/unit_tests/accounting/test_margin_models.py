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

from nautilus_trader.backtest.models import LeveragedMarginModel
from nautilus_trader.backtest.models import StandardMarginModel
from nautilus_trader.core.rust.model import PositionSide
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider


USD = TestInstrumentProvider.default_fx_ccy("AUD/USD").quote_currency


class TestStandardMarginModel:
    def test_calculate_margin_init_ignores_leverage(self):
        # Arrange
        model = StandardMarginModel()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        quantity = Quantity.from_int(100_000)
        price = Price.from_str("0.80000")
        leverage = Decimal(50)  # High leverage should be ignored

        # Act
        result = model.calculate_margin_init(
            instrument=instrument,
            quantity=quantity,
            price=price,
            leverage=leverage,
        )

        # Assert - Should be 3% of notional (80,000 * 0.03 = 2,400)
        assert result == Money(2400.00, USD)

    def test_calculate_margin_maint_ignores_leverage(self):
        # Arrange
        model = StandardMarginModel()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        quantity = Quantity.from_int(1_000_000)
        price = Price.from_str("1.00000")
        leverage = Decimal(50)  # High leverage should be ignored

        # Act
        result = model.calculate_margin_maint(
            instrument=instrument,
            side=PositionSide.LONG,
            quantity=quantity,
            price=price,
            leverage=leverage,
        )

        # Assert - Should be 3% of notional (1,000,000 * 0.03 = 30,000)
        assert result == Money(30000.00, USD)


class TestLeveragedMarginModel:
    def test_calculate_margin_init_applies_leverage(self):
        # Arrange
        model = LeveragedMarginModel()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        quantity = Quantity.from_int(100_000)
        price = Price.from_str("0.80000")
        leverage = Decimal(50)

        # Act
        result = model.calculate_margin_init(
            instrument=instrument,
            quantity=quantity,
            price=price,
            leverage=leverage,
        )

        # Assert - Should be (80,000 / 50) * 0.03 = 48.00
        assert result == Money(48.00, USD)

    def test_calculate_margin_maint_applies_leverage(self):
        # Arrange
        model = LeveragedMarginModel()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        quantity = Quantity.from_int(1_000_000)
        price = Price.from_str("1.00000")
        leverage = Decimal(50)

        # Act
        result = model.calculate_margin_maint(
            instrument=instrument,
            side=PositionSide.LONG,
            quantity=quantity,
            price=price,
            leverage=leverage,
        )

        # Assert - Should be (1,000,000 / 50) * 0.03 = 600.00
        assert result == Money(600.00, USD)


class TestMarginAccountWithModels:
    def test_margin_account_with_standard_model(self):
        # Arrange
        from nautilus_trader.test_kit.stubs.execution import TestExecStubs

        account = TestExecStubs.margin_account()
        standard_model = StandardMarginModel()
        account.set_margin_model(standard_model)

        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        quantity = Quantity.from_int(100_000)
        price = Price.from_str("0.80000")

        # Act
        result = account.calculate_margin_init(
            instrument=instrument,
            quantity=quantity,
            price=price,
        )

        # Assert - Should use standard model (ignore leverage)
        assert result == Money(2400.00, USD)

    def test_margin_account_with_leveraged_model_default(self):
        # Arrange
        from nautilus_trader.test_kit.stubs.execution import TestExecStubs

        account = TestExecStubs.margin_account()
        # Default should be LeveragedMarginModel for backward compatibility

        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        account.set_leverage(instrument.id, Decimal(50))
        quantity = Quantity.from_int(100_000)
        price = Price.from_str("0.80000")

        # Act
        result = account.calculate_margin_init(
            instrument=instrument,
            quantity=quantity,
            price=price,
        )

        # Assert - Should use leveraged model (current behavior)
        assert result == Money(48.00, USD)

    def test_margin_account_with_standard_model_set(self):
        # Arrange
        from nautilus_trader.test_kit.stubs.execution import TestExecStubs

        account = TestExecStubs.margin_account()
        standard_model = StandardMarginModel()
        account.set_margin_model(standard_model)

        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        account.set_leverage(instrument.id, Decimal(50))  # High leverage
        quantity = Quantity.from_int(100_000)
        price = Price.from_str("0.80000")

        # Act
        result = account.calculate_margin_init(
            instrument=instrument,
            quantity=quantity,
            price=price,
        )

        # Assert - Should ignore leverage (standard behavior)
        assert result == Money(2400.00, USD)


class TestCustomMarginModelConfig:
    def test_custom_model_receives_config(self):
        # Arrange
        from nautilus_trader.backtest.config import MarginModelConfig
        from nautilus_trader.backtest.config import MarginModelFactory
        from nautilus_trader.backtest.models import MarginModel

        class TestCustomMarginModel(MarginModel):
            def __init__(self, config):
                self.multiplier = Decimal(str(config.config.get("multiplier", 1.0)))
                self.use_leverage = config.config.get("use_leverage", False)

            def calculate_margin_init(
                self,
                instrument,
                quantity,
                price,
                leverage,
                use_quote_for_inverse=False,
            ):
                notional = instrument.notional_value(quantity, price, use_quote_for_inverse)
                if self.use_leverage:
                    adjusted_notional = notional.as_decimal() / leverage
                else:
                    adjusted_notional = notional.as_decimal()
                margin = adjusted_notional * instrument.margin_init * self.multiplier
                return Money(margin, instrument.quote_currency)

            def calculate_margin_maint(
                self,
                instrument,
                side,
                quantity,
                price,
                leverage,
                use_quote_for_inverse=False,
            ):
                return self.calculate_margin_init(
                    instrument,
                    quantity,
                    price,
                    leverage,
                    use_quote_for_inverse,
                )

        # Temporarily add to globals for resolve_path to find it
        import sys

        sys.modules[__name__].TestCustomMarginModel = TestCustomMarginModel

        config = MarginModelConfig(
            model_type=f"{__name__}:TestCustomMarginModel",
            config={"multiplier": 2.0, "use_leverage": True},
        )

        # Act
        model = MarginModelFactory.create(config)

        # Assert
        assert isinstance(model, TestCustomMarginModel)
        assert model.multiplier == 2.0
        assert model.use_leverage

        # Test calculation
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        quantity = Quantity.from_int(100_000)
        price = Price.from_str("0.80000")
        leverage = Decimal(50)

        result = model.calculate_margin_init(instrument, quantity, price, leverage)

        # Should be (80,000 / 50) * 0.03 * 2.0 = 96.00
        assert result == Money(96.00, USD)


class TestLeveragedMarginModelEdgeCases:
    def test_calculate_margin_init_zero_leverage_raises_error(self):
        # Arrange
        model = LeveragedMarginModel()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        quantity = Quantity.from_int(100_000)
        price = Price.from_str("0.80000")
        leverage = Decimal(0)  # Zero leverage

        # Act & Assert
        import pytest

        with pytest.raises(Exception):  # Should raise validation error
            model.calculate_margin_init(
                instrument=instrument,
                quantity=quantity,
                price=price,
                leverage=leverage,
            )

    def test_calculate_margin_init_negative_leverage_raises_error(self):
        # Arrange
        model = LeveragedMarginModel()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        quantity = Quantity.from_int(100_000)
        price = Price.from_str("0.80000")
        leverage = Decimal(-10)  # Negative leverage

        # Act & Assert
        import pytest

        with pytest.raises(Exception):  # Should raise validation error
            model.calculate_margin_init(
                instrument=instrument,
                quantity=quantity,
                price=price,
                leverage=leverage,
            )

    def test_calculate_margin_maint_zero_leverage_raises_error(self):
        # Arrange
        model = LeveragedMarginModel()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        quantity = Quantity.from_int(100_000)
        price = Price.from_str("0.80000")
        leverage = Decimal(0)  # Zero leverage

        # Act & Assert
        import pytest

        with pytest.raises(Exception):  # Should raise validation error
            model.calculate_margin_maint(
                instrument=instrument,
                side=PositionSide.LONG,
                quantity=quantity,
                price=price,
                leverage=leverage,
            )

    def test_calculate_margin_maint_negative_leverage_raises_error(self):
        # Arrange
        model = LeveragedMarginModel()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        quantity = Quantity.from_int(100_000)
        price = Price.from_str("0.80000")
        leverage = Decimal(-10)  # Negative leverage

        # Act & Assert
        import pytest

        with pytest.raises(Exception):  # Should raise validation error
            model.calculate_margin_maint(
                instrument=instrument,
                side=PositionSide.LONG,
                quantity=quantity,
                price=price,
                leverage=leverage,
            )
