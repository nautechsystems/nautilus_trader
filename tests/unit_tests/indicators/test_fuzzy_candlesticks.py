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

import unittest

import numpy as np

from nautilus_trader.indicators.fuzzy_candlesticks import FuzzyCandle
from nautilus_trader.indicators.fuzzy_candlesticks import FuzzyCandlesticks
from nautilus_trader.indicators.fuzzy_enum import CandleBodySize
from nautilus_trader.indicators.fuzzy_enum import CandleDirection
from nautilus_trader.indicators.fuzzy_enum import CandleSize
from nautilus_trader.indicators.fuzzy_enum import CandleWickSize
from tests.test_kit.series import BatterySeries


class FuzzyCandlesticksTests(unittest.TestCase):

    # Test Fixture
    def setUp(self):
        # Arrange
        self.fc = FuzzyCandlesticks(10, 0.5, 1.0, 2.0, 3.0)

    def test_fuzzy_candle_equality(self):
        # Arrange
        fuzzy_candle1 = FuzzyCandle(
            CandleDirection.BULL,
            CandleSize.MEDIUM,
            CandleBodySize.MEDIUM,
            CandleWickSize.SMALL,
            CandleWickSize.SMALL)

        fuzzy_candle2 = FuzzyCandle(
            CandleDirection.BULL,
            CandleSize.MEDIUM,
            CandleBodySize.MEDIUM,
            CandleWickSize.SMALL,
            CandleWickSize.SMALL)

        fuzzy_candle3 = FuzzyCandle(
            CandleDirection.BEAR,
            CandleSize.MEDIUM,
            CandleBodySize.MEDIUM,
            CandleWickSize.SMALL,
            CandleWickSize.SMALL)

        # Act
        result1 = fuzzy_candle1.__eq__(fuzzy_candle2)
        result2 = fuzzy_candle1 == fuzzy_candle2
        result3 = fuzzy_candle1 == fuzzy_candle3

        # Assert
        self.assertTrue(result1)
        self.assertTrue(result2)
        self.assertFalse(result3)

    def test_fuzzy_candle_hashcode(self):
        # Arrange
        fuzzy_candle1 = FuzzyCandle(
            CandleDirection.BULL,
            CandleSize.MEDIUM,
            CandleBodySize.MEDIUM,
            CandleWickSize.SMALL,
            CandleWickSize.SMALL)

        fuzzy_candle2 = FuzzyCandle(
            CandleDirection.BULL,
            CandleSize.MEDIUM,
            CandleBodySize.MEDIUM,
            CandleWickSize.SMALL,
            CandleWickSize.SMALL)

        # Act
        hash1 = fuzzy_candle1.__hash__()
        hash2 = fuzzy_candle2.__hash__()

        # Assert
        self.assertEqual(hash1, hash2)

    def test_fuzzy_str_and_repr(self):
        # Arrange
        fuzzy_candle = FuzzyCandle(
            CandleDirection.BULL,
            CandleSize.MEDIUM,
            CandleBodySize.MEDIUM,
            CandleWickSize.SMALL,
            CandleWickSize.SMALL)

        # Act
        # Assert
        self.assertEqual("(1, 3, 2, 1, 1)", str(fuzzy_candle))
        self.assertTrue(repr(fuzzy_candle).startswith("<FuzzyCandle(1, 3, 2, 1, 1) object at"))

    def test_name_returns_expected_name(self):
        # Act
        # Assert
        self.assertEqual("FuzzyCandlesticks", self.fc.name)

    def test_str_returns_expected_string(self):
        # Act
        result = str(self.fc)
        # Assert
        self.assertEqual("FuzzyCandlesticks(10, 0.5, 1.0, 2.0, 3.0)", result)

    def test_repr_returns_expected_string(self):
        # Act
        # Assert
        self.assertTrue(repr(self.fc).startswith("<FuzzyCandlesticks(10, 0.5, 1.0, 2.0, 3.0) object at"))
        self.assertTrue(repr(self.fc).endswith(">"))

    def test_period_returns_expected_value(self):
        # Act
        # Assert
        self.assertEqual(10, self.fc.period)

    def test_values_with_doji_bars_returns_expected_results(self):
        # arrange
        self.fc.update_raw(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update_raw(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update_raw(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update_raw(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update_raw(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update_raw(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update_raw(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update_raw(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update_raw(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update_raw(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update_raw(1.00000, 1.00000, 1.00000, 1.00000)

        # act
        result_candle = self.fc.value
        result_vector = self.fc.vector

        # assert
        self.assertTrue(np.array_equal([0, 0, 0, 0, 0], result_vector))
        self.assertEqual(CandleDirection.NONE, result_candle.direction)
        self.assertEqual(CandleSize.NONE, result_candle.size)
        self.assertEqual(CandleBodySize.NONE, result_candle.body_size)
        self.assertEqual(CandleWickSize.NONE, result_candle.upper_wick_size)
        self.assertEqual(CandleWickSize.NONE, result_candle.lower_wick_size)
        self.assertEqual(0, self.fc.dir)
        self.assertEqual(0, self.fc.size)
        self.assertEqual(0, self.fc.b_size)
        self.assertEqual(0, self.fc.uw_size)
        self.assertEqual(0, self.fc.lw_size)

    def test_values_with_stub_bars_returns_expected_results(self):
        # Arrange
        self.fc.update_raw(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update_raw(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update_raw(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update_raw(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update_raw(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update_raw(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update_raw(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update_raw(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update_raw(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update_raw(1.00000, 1.00010, 0.99990, 1.00005)

        # Act
        result_candle = self.fc.value
        result_vector = self.fc.vector

        # Assert
        self.assertTrue(np.array_equal([1, 1, 1, 1, 1], result_vector))
        self.assertEqual(CandleDirection.BULL, result_candle.direction)
        self.assertEqual(CandleSize.VERY_SMALL, result_candle.size)
        self.assertEqual(CandleBodySize.SMALL, result_candle.body_size)
        self.assertEqual(CandleWickSize.SMALL, result_candle.upper_wick_size)
        self.assertEqual(CandleWickSize.SMALL, result_candle.lower_wick_size)
        self.assertEqual(1, self.fc.dir)
        self.assertEqual(1, self.fc.size)
        self.assertEqual(1, self.fc.b_size)
        self.assertEqual(1, self.fc.uw_size)
        self.assertEqual(1, self.fc.lw_size)

    def test_values_with_down_market_returns_expected_results(self):
        # Arrange
        self.fc.update_raw(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update_raw(1.00005, 1.00005, 0.99990, 0.99990)
        self.fc.update_raw(0.99990, 0.99990, 0.99960, 0.99970)
        self.fc.update_raw(0.99970, 0.99970, 0.99930, 0.99950)
        self.fc.update_raw(0.99950, 0.99960, 0.99925, 0.99930)
        self.fc.update_raw(0.99925, 0.99930, 0.99900, 0.99910)
        self.fc.update_raw(0.99910, 0.99910, 0.99890, 0.99895)
        self.fc.update_raw(0.99895, 0.99990, 0.99885, 0.99885)
        self.fc.update_raw(0.99885, 0.99885, 0.99860, 0.99870)
        self.fc.update_raw(0.99870, 0.99870, 0.99850, 0.99850)

        # Act
        result_candle = self.fc.value
        result_vector = self.fc.vector

        # Assert
        self.assertTrue([-1, 2, 4, 2, 2], result_vector)
        self.assertEqual(CandleDirection.BEAR, result_candle.direction)
        self.assertEqual(CandleSize.SMALL, result_candle.size)
        self.assertEqual(CandleBodySize.TREND, result_candle.body_size)
        self.assertEqual(CandleWickSize.MEDIUM, result_candle.upper_wick_size)
        self.assertEqual(CandleWickSize.MEDIUM, result_candle.lower_wick_size)
        self.assertEqual(-1, self.fc.dir)
        self.assertEqual(2, self.fc.size)
        self.assertEqual(4, self.fc.b_size)
        self.assertEqual(2, self.fc.uw_size)
        self.assertEqual(2, self.fc.lw_size)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(1000):
            self.fc.update_raw(1.00000, 1.00000, 1.00000, 1.00000)

        # Act
        self.fc.reset()

        # Assert
        self.assertEqual(False, self.fc.initialized)  # No assertion errors.

    def test_with_battery_signal_raises_no_exceptions(self):
        # Arrange
        battery_signal = BatterySeries.create()
        output = []

        # Act
        for point in battery_signal:
            try:
                self.fc.update_raw(point, point, point, point)
            except Exception as ex:
                print(ex)
            output.append(self.fc.vector)

        # Assert
        self.assertEqual(len(battery_signal), len(output))
