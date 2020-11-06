# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from datetime import datetime
import unittest

from parameterized import parameterized
import pytz

from nautilus_trader.model.bar import Bar
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.stubs import TestStubs


AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
GBPUSD_FXCM = TestStubs.symbol_gbpusd_fxcm()


class BarSpecificationTests(unittest.TestCase):

    def test_bar_spec_equality(self):
        # Arrange
        bar_spec1 = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_spec2 = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_spec3 = BarSpecification(1, BarAggregation.MINUTE, PriceType.ASK)

        # Act
        # Assert
        self.assertTrue(bar_spec1 == bar_spec1)
        self.assertTrue(bar_spec1 == bar_spec2)
        self.assertTrue(bar_spec1 != bar_spec3)

    def test_bar_spec_str_and_repr(self):
        # Arrange
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)

        # Act
        # Assert
        self.assertEqual("1-MINUTE-BID", str(bar_spec))
        self.assertEqual("BarSpecification(1-MINUTE-BID)", repr(bar_spec))

    @parameterized.expand([
        [""],
        ["1"],
        ["-1-TICK-MID"],
        ["1-TICK_MID"],
    ])
    def test_from_string_given_various_invalid_strings_raises_value_error(self, value):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, BarSpecification.from_string, value)

    @parameterized.expand([
        ["1-MINUTE-BID", BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)],
        ["15-MINUTE-MID", BarSpecification(15, BarAggregation.MINUTE, PriceType.MID)],
        ["100-TICK-LAST", BarSpecification(100, BarAggregation.TICK, PriceType.LAST)],
        ["10000-NOTIONAL_IMBALANCE-MID", BarSpecification(10000, BarAggregation.NOTIONAL_IMBALANCE, PriceType.MID)],

    ])
    def test_from_string_given_various_valid_string_returns_expected_specification(self, value, expected):
        # Arrange
        # Act
        spec = BarSpecification.from_string(value)

        # Assert
        self.assertEqual(spec, expected)


class BarTypeTests(unittest.TestCase):

    def test_bar_type_equality(self):
        # Arrange
        symbol1 = Symbol("AUD/USD", Venue('FXCM'))
        symbol2 = Symbol("GBP/USD", Venue('FXCM'))
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_type1 = BarType(symbol1, bar_spec)
        bar_type2 = BarType(symbol1, bar_spec)
        bar_type3 = BarType(symbol2, bar_spec)

        # Act
        # Assert
        self.assertTrue(bar_type1 == bar_type1)
        self.assertTrue(bar_type1 == bar_type2)
        self.assertTrue(bar_type1 != bar_type3)

    def test_bar_type_str_and_repr(self):
        # Arrange
        symbol = Symbol("AUD/USD", Venue('FXCM'))
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_type = BarType(symbol, bar_spec)

        # Act
        # Assert
        self.assertEqual("AUD/USD.FXCM-1-MINUTE-BID", str(bar_type))
        self.assertEqual("BarType(AUD/USD.FXCM-1-MINUTE-BID)", repr(bar_type))

    @parameterized.expand([
        [""],
        ["AUD/USD"],
        ["AUD/USD.IDEALPRO-1-MILLISECOND-BID"],
    ])
    def test_from_string_given_various_invalid_strings_raises_value_error(self, value):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, BarType.from_string, value)

    @parameterized.expand([
        ["AUD/USD.IDEALPRO-1-MINUTE-BID", BarType(Symbol("AUD/USD", Venue("IDEALPRO")), BarSpecification(1, BarAggregation.MINUTE, PriceType.BID))],
        ["GBP/USD.FXCM-1000-TICK-MID", BarType(Symbol("GBP/USD", Venue("FXCM")), BarSpecification(1000, BarAggregation.TICK, PriceType.MID))],
        ["AAPL.NYSE-1-HOUR-MID", BarType(Symbol("AAPL", Venue("NYSE")), BarSpecification(1, BarAggregation.HOUR, PriceType.MID))],
        ["BTC/USDT.BINANCE-100-TICK-LAST", BarType(Symbol("BTC/USDT", Venue("BINANCE")), BarSpecification(100, BarAggregation.TICK, PriceType.LAST))],
    ])
    def test_from_string_given_various_valid_string_returns_expected_specification(self, value, expected):
        # Arrange
        # Act
        bar_type = BarType.from_string(value)

        # Assert
        self.assertEqual(bar_type, expected)


class BarTests(unittest.TestCase):

    def test_check_when_high_below_low_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            ValueError,
            Bar,
            Price("1.00001"),
            Price("1.00000"),  # High below low
            Price("1.00002"),
            Price("1.00003"),
            Quantity(100000),
            datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc),
            True,
        )

    def test_check_when_high_below_close_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            ValueError,
            Bar,
            Price("1.00000"),
            Price("1.00000"),  # High below close
            Price("1.00000"),
            Price("1.00005"),
            Quantity(100000),
            datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc),
            True,
        )

    def test_check_when_low_above_close_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            ValueError,
            Bar,
            Price("1.00000"),
            Price("1.00005"),
            Price("1.00000"),
            Price("0.99999"),  # Close below low
            Quantity(100000),
            datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc),
            True,
        )

    def test_equality(self):
        # Arrange
        bar1 = Bar(
            Price("1.00001"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity(100000),
            datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc),
        )

        bar2 = Bar(
            Price("1.00000"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity(100000),
            datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc),
        )

        # Act
        # Assert
        self.assertEqual(bar1, bar1)
        self.assertNotEqual(bar1, bar2)

    def test_str_repr(self):
        # Arrange
        bar = Bar(
            Price("1.00001"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity(100000),
            datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc),
        )

        # Act
        # Assert
        self.assertEqual("1.00001,1.00004,1.00002,1.00003,100000,1970-01-01T00:00:00.000Z", str(bar))
        self.assertEqual("Bar(1.00001,1.00004,1.00002,1.00003,100000,1970-01-01T00:00:00.000Z)", repr(bar))

    def test_to_serializable_string(self):
        # Arrange
        bar = Bar(
            Price("1.00001"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity(100000),
            datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc),
        )

        # Act
        serializable = bar.to_serializable_string()

        # Assert
        self.assertEqual("1.00001,1.00004,1.00002,1.00003,100000,0", serializable)

    def test_from_serializable_string(self):
        # Arrange
        bar = TestStubs.bar_5decimal()

        # Act
        result = Bar.from_serializable_string(bar.to_serializable_string())

        # Assert
        self.assertEqual(bar, result)
