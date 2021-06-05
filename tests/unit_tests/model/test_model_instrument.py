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

from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import JPY
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.providers import TestDataProvider
from tests.test_kit.providers import TestInstrumentProvider


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
XBTUSD_BITMEX = TestInstrumentProvider.xbtusd_bitmex()
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
BTCUSDT_BINANCE_INSTRUMENT = TestDataProvider.binance_btcusdt_instrument()
ETHUSD_BITMEX = TestInstrumentProvider.ethusd_bitmex()


class TestInstrument:
    @pytest.mark.parametrize(
        "instrument1, instrument2, expected1, expected2",
        [
            [AUDUSD_SIM, AUDUSD_SIM, True, False],
            [AUDUSD_SIM, USDJPY_SIM, False, True],
        ],
    )
    def test_equality(self, instrument1, instrument2, expected1, expected2):
        # Arrange
        # Act
        result1 = instrument1 == instrument2
        result2 = instrument1 != instrument2

        # Assert
        assert result1 == expected1
        assert result2 == expected2

    # TODO: WIP - TBC
    # def test_str_repr_returns_expected(self):
    #     # Arrange
    #     # Act
    #     # Assert
    #     assert str(BTCUSDT_BINANCE) == BTCUSDT_BINANCE_INSTRUMENT
    #     assert repr(BTCUSDT_BINANCE) == BTCUSDT_BINANCE_INSTRUMENT

    def test_hash(self):
        # Arrange
        # Act
        # Assert
        assert isinstance(hash(BTCUSDT_BINANCE), int)
        assert hash(BTCUSDT_BINANCE), hash(BTCUSDT_BINANCE)

    @pytest.mark.parametrize(
        "value, expected_str",
        [
            [0, "0.00000"],
            [1, "1.00000"],
            [1.23456, "1.23456"],
            [1.234567, "1.23457"],  # <-- rounds to precision
            ["0", "0.00000"],
            ["1.00", "1.00000"],
            [Decimal(), "0.00000"],
            [Decimal(1), "1.00000"],
            [Decimal("0.85"), "0.85000"],
        ],
    )
    def test_make_price_with_various_values_returns_expected(
        self,
        value,
        expected_str,
    ):
        # Arrange, Act
        price = AUDUSD_SIM.make_price(value)

        # Assert
        assert str(price) == expected_str

    @pytest.mark.parametrize(
        "value, expected_str",
        [
            [0, "0.000000"],
            [1, "1.000000"],
            [1.23456, "1.234560"],
            [1.2345678, "1.234568"],  # <-- rounds to precision
            ["0", "0.000000"],
            ["1.00", "1.000000"],
            [Decimal(), "0.000000"],
            [Decimal(1), "1.000000"],
            [Decimal("0.85"), "0.850000"],
        ],
    )
    def test_make_qty_with_various_values_returns_expected(
        self,
        value,
        expected_str,
    ):
        # Arrange, Act
        qty = BTCUSDT_BINANCE.make_qty(value)

        # Assert
        assert str(qty) == expected_str

    @pytest.mark.parametrize(
        "instrument, expected",
        [
            [AUDUSD_SIM, USD],
            [BTCUSDT_BINANCE, USDT],
            [XBTUSD_BITMEX, BTC],
            [ETHUSD_BITMEX, BTC],
        ],
    )
    def test_cost_currency_for_various_instruments(self, instrument, expected):
        # Arrange, Act, Asset
        assert instrument.cost_currency() == expected

    def test_calculate_notional_value(self):
        # Arrange
        instrument = TestInstrumentProvider.btcusdt_binance()

        # Act
        result = instrument.notional_value(
            Quantity.from_int(10),
            Price.from_str("11493.60"),
        )

        # Assert
        assert result == Money(114936.00000000, USDT)

    @pytest.mark.parametrize(
        "inverse_as_quote, expected",
        [
            [False, Money(8.70049419, BTC)],
            [True, Money(100000.00, USD)],
        ],
    )
    def test_calculate_notional_value_for_inverse(self, inverse_as_quote, expected):
        # Arrange
        instrument = TestInstrumentProvider.xbtusd_bitmex()

        # Act
        result = instrument.notional_value(
            Quantity.from_int(100000),
            Price.from_str("11493.60"),
            inverse_as_quote=inverse_as_quote,
        )

        # Assert
        assert result == expected

    def test_calculate_initial_margin_with_leverage(self):
        # Arrange
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")

        result = instrument.calculate_initial_margin(
            Quantity.from_int(100000),
            Price.from_str("0.80000"),
            leverage=Decimal(50),
        )

        # Assert
        assert result == Money(48.06, USD)

    @pytest.mark.parametrize(
        "inverse_as_quote, expected",
        [
            [False, Money(0.10005568, BTC)],
            [True, Money(1150.00, USD)],
        ],
    )
    def test_calculate_initial_margin_with_no_leverage_for_inverse(
        self, inverse_as_quote, expected
    ):
        # Arrange
        instrument = TestInstrumentProvider.xbtusd_bitmex()

        result = instrument.calculate_initial_margin(
            Quantity.from_int(100000),
            Price.from_str("11493.60"),
            inverse_as_quote=inverse_as_quote,
        )

        # Assert
        assert result == expected

    def test_calculate_position_maint_with_no_leverage(self):
        # Arrange
        instrument = TestInstrumentProvider.xbtusd_bitmex()

        # Act
        result = instrument.calculate_maint_margin(
            PositionSide.LONG,
            Quantity.from_int(100000),
            Price.from_str("11493.60"),
        )

        # Assert
        assert result == Money(0.03697710, BTC)

    @pytest.mark.parametrize(
        "inverse_as_quote, expected",
        [
            [False, Money(-0.00218331, BTC)],  # Negative commission = credit
            [True, Money(-25.00, USD)],  # Negative commission = credit
        ],
    )
    def test_calculate_commission_for_inverse_maker_crypto(
        self, inverse_as_quote, expected
    ):
        # Arrange
        instrument = TestInstrumentProvider.xbtusd_bitmex()

        # Act
        result = instrument.calculate_commission(
            Quantity.from_int(100000),
            Decimal("11450.50"),
            LiquiditySide.MAKER,
            inverse_as_quote=inverse_as_quote,
        )

        # Assert
        assert result == expected

    def test_calculate_commission_for_taker_fx(self):
        # Arrange
        instrument = AUDUSD_SIM

        # Act
        result = instrument.calculate_commission(
            Quantity.from_int(1500000),
            Decimal("0.80050"),
            LiquiditySide.TAKER,
        )

        # Assert
        assert result == Money(24.02, USD)

    def test_calculate_commission_crypto_taker(self):
        # Arrange
        instrument = TestInstrumentProvider.xbtusd_bitmex()

        # Act
        result = instrument.calculate_commission(
            Quantity.from_int(100000),
            Decimal("11450.50"),
            LiquiditySide.TAKER,
        )

        # Assert
        assert result == Money(0.00654993, BTC)

    def test_calculate_commission_fx_taker(self):
        # Arrange
        instrument = TestInstrumentProvider.default_fx_ccy("USD/JPY", Venue("IDEALPRO"))

        # Act
        result = instrument.calculate_commission(
            Quantity.from_int(2200000),
            Decimal("120.310"),
            LiquiditySide.TAKER,
        )

        # Assert
        assert result == Money(5294, JPY)
