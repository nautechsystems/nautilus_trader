# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
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

from nautilus_trader.indicators.fuzzy_candlesticks import FuzzyCandlesticks
from nautilus_trader.indicators.fuzzy_candlesticks import FuzzyCandle
from nautilus_trader.indicators.fuzzy_candlesticks import CandleDirection
from nautilus_trader.indicators.fuzzy_candlesticks import CandleSize
from nautilus_trader.indicators.fuzzy_candlesticks import CandleBodySize
from nautilus_trader.indicators.fuzzy_candlesticks import CandleWickSize

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

    def test_name_returns_expected_name(self):
        # Act
        # Assert
        self.assertEqual('FuzzyCandlesticks', self.fc.name)

    def test_str_returns_expected_string(self):
        # Act
        result = str(self.fc)
        # Assert
        self.assertEqual('FuzzyCandlesticks(10, 0.5, 1.0, 2.0, 3.0)', result)

    def test_repr_returns_expected_string(self):
        # Act
        # Assert
        self.assertTrue(repr(self.fc).startswith('<FuzzyCandlesticks(10, 0.5, 1.0, 2.0, 3.0) object at'))
        self.assertTrue(repr(self.fc).endswith('>'))

    def test_period_returns_expected_value(self):
        # Act
        # Assert
        self.assertEqual(10, self.fc.period)

    def test_fuzzify_direction_returns_expected_values(self):
        # Arrange
        # Act
        result1 = FuzzyCandlesticks.fuzzify_direction(1.00000, 1.00010)
        result2 = FuzzyCandlesticks.fuzzify_direction(1.00000, 1.00000)
        result3 = FuzzyCandlesticks.fuzzify_direction(1.00000, 0.99990)

        # Assert
        self.assertEqual(CandleDirection.BULL, result1)
        self.assertEqual(CandleDirection.NONE, result2)
        self.assertEqual(CandleDirection.BEAR, result3)

    def test_price_comparison_returns_expected_results(self):
        # Arrange
        # Act
        result1 = self.fc.price_comparison(1.00000, 1.00010)
        result2 = self.fc.price_comparison(1.00000, 1.00000)
        result3 = self.fc.price_comparison(1.00000, 0.99990)

        # Assert
        self.assertEqual(-1, result1)
        self.assertEqual(0, result2)
        self.assertEqual(1, result3)

    def test_values_with_doji_bars_returns_expected_results(self):
        # arrange
        self.fc.update(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update(1.00000, 1.00000, 1.00000, 1.00000)
        self.fc.update(1.00000, 1.00000, 1.00000, 1.00000)

        # act
        result_candle = self.fc.value
        result_array = self.fc.value_array
        result_comparison = self.fc.value_price_comparisons

        # assert
        self.assertTrue(np.array_equal([0, 0, 0, 0, 0], result_array))
        self.assertTrue(np.array_equal([0, 0, 0, 0, 0], result_comparison))
        self.assertEqual(CandleDirection.NONE, result_candle.direction)
        self.assertEqual(CandleSize.NONE, result_candle.size)
        self.assertEqual(CandleBodySize.NONE, result_candle.body_size)
        self.assertEqual(CandleWickSize.NONE, result_candle.upper_wick_size)
        self.assertEqual(CandleWickSize.NONE, result_candle.lower_wick_size)

    def test_values_with_stub_bars_returns_expected_results(self):
        # Arrange
        self.fc.update(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update(1.00000, 1.00010, 0.99990, 1.00005)

        # Act
        result_candle = self.fc.value
        result_array = self.fc.value_array
        result_comparison = self.fc.value_price_comparisons

        # Assert
        self.assertTrue(np.array_equal([1, 1, 1, 1, 1], result_array))
        self.assertTrue(np.array_equal([0, 0, -1, 1, 0], result_comparison))
        self.assertEqual(CandleDirection.BULL, result_candle.direction)
        self.assertEqual(CandleSize.VERY_SMALL, result_candle.size)
        self.assertEqual(CandleBodySize.SMALL, result_candle.body_size)
        self.assertEqual(CandleWickSize.SMALL, result_candle.upper_wick_size)
        self.assertEqual(CandleWickSize.SMALL, result_candle.lower_wick_size)

    def test_values_with_down_market_returns_expected_results(self):
        # Arrange
        self.fc.update(1.00000, 1.00010, 0.99990, 1.00005)
        self.fc.update(1.00005, 1.00005, 0.99990, 0.99990)
        self.fc.update(0.99990, 0.99990, 0.99960, 0.99970)
        self.fc.update(0.99970, 0.99970, 0.99930, 0.99950)
        self.fc.update(0.99950, 0.99960, 0.99925, 0.99930)
        self.fc.update(0.99925, 0.99930, 0.99900, 0.99910)
        self.fc.update(0.99910, 0.99910, 0.99890, 0.99895)
        self.fc.update(0.99895, 0.99990, 0.99885, 0.99885)
        self.fc.update(0.99885, 0.99885, 0.99860, 0.99870)
        self.fc.update(0.99870, 0.99870, 0.99850, 0.99850)

        # Act
        result_candle = self.fc.value
        result_array = self.fc.value_array
        result_comparison = self.fc.value_price_comparisons

        # Assert
        print(result_candle)
        print(result_array)
        print(result_comparison)
        self.assertTrue(np.array_equal([-1, 2, 4, 2, 2], result_array))
        self.assertTrue(np.array_equal([-1, -1, -1, -1, -1], result_comparison))
        self.assertEqual(CandleDirection.BEAR, result_candle.direction)
        self.assertEqual(CandleSize.SMALL, result_candle.size)
        self.assertEqual(CandleBodySize.TREND, result_candle.body_size)
        self.assertEqual(CandleWickSize.MEDIUM, result_candle.upper_wick_size)
        self.assertEqual(CandleWickSize.MEDIUM, result_candle.lower_wick_size)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for i in range(1000):
            self.fc.update(1.00000, 1.00000, 1.00000, 1.00000)

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
                self.fc.update(point, point, point, point)
            except Exception as ex:
                print(ex)
            output.append(self.fc.value)

        # Assert
        self.assertEqual(len(battery_signal), len(output))
