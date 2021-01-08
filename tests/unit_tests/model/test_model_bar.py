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

import unittest

from parameterized import parameterized

from nautilus_trader.model.bar import Bar
from nautilus_trader.model.bar import BarData
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH


AUDUSD_SIM = TestStubs.symbol_audusd_fxcm()
GBPUSD_SIM = TestStubs.symbol_gbpusd_fxcm()


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
    def test_from_str_given_various_invalid_strings_raises_value_error(self, value):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, BarSpecification.from_str, value)

    @parameterized.expand([
        ["1-MINUTE-BID", BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)],
        ["15-MINUTE-MID", BarSpecification(15, BarAggregation.MINUTE, PriceType.MID)],
        ["100-TICK-LAST", BarSpecification(100, BarAggregation.TICK, PriceType.LAST)],
        ["10000-VALUE_IMBALANCE-MID", BarSpecification(10000, BarAggregation.VALUE_IMBALANCE, PriceType.MID)],

    ])
    def test_from_str_given_various_valid_string_returns_expected_specification(self, value, expected):
        # Arrange
        # Act
        spec = BarSpecification.from_str(value)

        # Assert
        self.assertEqual(spec, expected)

    @parameterized.expand([
        [BarSpecification(1, BarAggregation.MINUTE, PriceType.BID), True, False, False],
        [BarSpecification(1000, BarAggregation.TICK, PriceType.MID), False, True, False],
        [BarSpecification(10000, BarAggregation.VALUE_RUNS, PriceType.MID), False, False, True],
    ])
    def test_aggregation_queries(
            self,
            bar_spec,
            is_time_aggregated,
            is_threshold_aggregated,
            is_information_aggregated,
    ):
        # Arrange
        # Act
        # Assert
        self.assertEqual(is_time_aggregated, bar_spec.is_time_aggregated())
        self.assertEqual(is_threshold_aggregated, bar_spec.is_threshold_aggregated())
        self.assertEqual(is_information_aggregated, bar_spec.is_information_aggregated())


class BarTypeTests(unittest.TestCase):

    def test_bar_type_equality(self):
        # Arrange
        symbol1 = Symbol("AUD/USD", Venue("SIM"))
        symbol2 = Symbol("GBP/USD", Venue("SIM"))
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
        symbol = Symbol("AUD/USD", Venue("SIM"))
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_type = BarType(symbol, bar_spec)

        # Act
        # Assert
        self.assertEqual("AUD/USD.SIM-1-MINUTE-BID", str(bar_type))
        self.assertEqual("BarType(AUD/USD.SIM-1-MINUTE-BID, is_internal_aggregation=True)", repr(bar_type))

    @parameterized.expand([
        [""],
        ["AUD/USD"],
        ["AUD/USD.IDEALPRO-1-MILLISECOND-BID"],
    ])
    def test_from_str_given_various_invalid_strings_raises_value_error(self, value):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, BarType.from_str, value)

    @parameterized.expand([
        ["AUD/USD.IDEALPRO-1-MINUTE-BID", BarType(Symbol("AUD/USD", Venue("IDEALPRO")), BarSpecification(1, BarAggregation.MINUTE, PriceType.BID))],
        ["GBP/USD.SIM-1000-TICK-MID", BarType(Symbol("GBP/USD", Venue("SIM")), BarSpecification(1000, BarAggregation.TICK, PriceType.MID))],
        ["AAPL.NYSE-1-HOUR-MID", BarType(Symbol("AAPL", Venue("NYSE")), BarSpecification(1, BarAggregation.HOUR, PriceType.MID))],
        ["BTC/USDT.BINANCE-100-TICK-LAST", BarType(Symbol("BTC/USDT", Venue("BINANCE")), BarSpecification(100, BarAggregation.TICK, PriceType.LAST))],
    ])
    def test_from_str_given_various_valid_string_returns_expected_specification(self, value, expected):
        # Arrange
        # Act
        bar_type = BarType.from_str(value, is_internal_aggregation=True)

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
            UNIX_EPOCH,
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
            UNIX_EPOCH,
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
            UNIX_EPOCH,
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
            UNIX_EPOCH,
        )

        bar2 = Bar(
            Price("1.00000"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity(100000),
            UNIX_EPOCH,
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
            UNIX_EPOCH,
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
            UNIX_EPOCH,
        )

        # Act
        serializable = bar.to_serializable_string()

        # Assert
        self.assertEqual("1.00001,1.00004,1.00002,1.00003,100000,0", serializable)

    def test_from_serializable_string_given_malformed_string_raises_value_error(self):
        # Arrange
        bar = Bar(
            Price("1.00001"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity(100000),
            UNIX_EPOCH,
        )

        # Act
        # Assert
        self.assertRaises(ValueError, bar.from_serializable_string, "NOT_A_BAR")

    def test_from_serializable_string_given_valid_string_returns_expected_bar(self):
        # Arrange
        bar = TestStubs.bar_5decimal()

        # Act
        result = Bar.from_serializable_string(bar.to_serializable_string())

        # Assert
        self.assertEqual(bar, result)


class BarDataTests(unittest.TestCase):

    def test_str_repr(self):
        # Arrange
        symbol = Symbol("GBP/USD", Venue("SIM"))
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_type = BarType(symbol, bar_spec)
        bar = Bar(
            Price("1.00001"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity(100000),
            UNIX_EPOCH,
        )

        bar_data = BarData(bar_type, bar)

        # Act
        # Assert
        self.assertEqual("BarData(bar_type=GBP/USD.SIM-1-MINUTE-BID, bar=1.00001,1.00004,1.00002,1.00003,100000,1970-01-01T00:00:00.000Z)", str(bar_data))   # noqa
        self.assertEqual("BarData(bar_type=GBP/USD.SIM-1-MINUTE-BID, bar=1.00001,1.00004,1.00002,1.00003,100000,1970-01-01T00:00:00.000Z)", repr(bar_data))  # noqa
