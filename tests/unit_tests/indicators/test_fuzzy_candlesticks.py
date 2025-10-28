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

import numpy as np

from nautilus_trader.indicators import CandleBodySize
from nautilus_trader.indicators import CandleDirection
from nautilus_trader.indicators import CandleSize
from nautilus_trader.indicators import CandleWickSize
from nautilus_trader.indicators import FuzzyCandle
from nautilus_trader.indicators import FuzzyCandlesticks
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestFuzzyCandlesticks:

    def setup(self):
        # Fixture Setup
        self.fc = FuzzyCandlesticks(10, 0.5, 1.0, 2.0, 3.0)

    def test_fuzzy_candle_equality(self):
        # Arrange
        fuzzy_candle1 = FuzzyCandle(
            CandleDirection.DIRECTION_BULL,
            CandleSize.SIZE_MEDIUM,
            CandleBodySize.BODY_MEDIUM,
            CandleWickSize.WICK_SMALL,
            CandleWickSize.WICK_SMALL,
        )

        fuzzy_candle2 = FuzzyCandle(
            CandleDirection.DIRECTION_BULL,
            CandleSize.SIZE_MEDIUM,
            CandleBodySize.BODY_MEDIUM,
            CandleWickSize.WICK_SMALL,
            CandleWickSize.WICK_SMALL,
        )

        fuzzy_candle3 = FuzzyCandle(
            CandleDirection.DIRECTION_BEAR,
            CandleSize.SIZE_MEDIUM,
            CandleBodySize.BODY_MEDIUM,
            CandleWickSize.WICK_SMALL,
            CandleWickSize.WICK_SMALL,
        )

        # Act, Assert
        assert fuzzy_candle1 == fuzzy_candle1
        assert fuzzy_candle1 == fuzzy_candle2
        assert fuzzy_candle1 != fuzzy_candle3

    def test_fuzzy_str_and_repr(self):
        # Arrange
        fuzzy_candle = FuzzyCandle(
            CandleDirection.DIRECTION_BULL,
            CandleSize.SIZE_MEDIUM,
            CandleBodySize.BODY_MEDIUM,
            CandleWickSize.WICK_SMALL,
            CandleWickSize.WICK_SMALL,
        )

        # Act, Assert
        assert str(fuzzy_candle) == "(1, 3, 2, 1, 1)"
        assert repr(fuzzy_candle) == "FuzzyCandle(1, 3, 2, 1, 1)"

    def test_name_returns_expected_name(self):
        # Arrange, Act, Assert
        assert self.fc.name == "FuzzyCandlesticks"

    def test_str_returns_expected_string(self):
        # Arrange, Act, Assert
        assert str(self.fc) == "FuzzyCandlesticks(10, 0.5, 1.0, 2.0, 3.0)"
        assert repr(self.fc) == "FuzzyCandlesticks(10, 0.5, 1.0, 2.0, 3.0)"

    def test_period_returns_expected_value(self):
        # Arrange, Act, Assert
        assert self.fc.period == 10

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = FuzzyCandlesticks(10, 0.5, 1.0, 2.0, 3.0)

        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs

    def test_values_with_doji_bars_returns_expected_results(self):
        # Arrange
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

        # Act
        result_candle = self.fc.value
        result_vector = self.fc.vector

        # Assert
        assert np.array_equal([0, 0, 0, 0, 0], result_vector)
        assert result_candle.direction == CandleDirection.DIRECTION_NONE
        assert result_candle.size == CandleSize.SIZE_NONE
        assert result_candle.body_size == CandleBodySize.BODY_NONE
        assert result_candle.upper_wick_size == CandleWickSize.WICK_NONE
        assert result_candle.lower_wick_size == CandleWickSize.WICK_NONE

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
        assert np.array_equal([1, 1, 1, 1, 1], result_vector)
        assert result_candle.direction == CandleDirection.DIRECTION_BULL
        assert result_candle.size == CandleSize.SIZE_VERY_SMALL
        assert result_candle.body_size == CandleBodySize.BODY_SMALL
        assert result_candle.upper_wick_size == CandleWickSize.WICK_SMALL
        assert result_candle.lower_wick_size == CandleWickSize.WICK_SMALL

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
        assert [-1, 2, 4, 2, 2], result_vector
        assert result_candle.direction == CandleDirection.DIRECTION_BEAR
        assert result_candle.size == CandleSize.SIZE_SMALL
        assert result_candle.body_size == CandleBodySize.BODY_TREND

        # fix: last bar is WICK_NONE
        # open = 0.99870
        # high = 0.99870
        # low  = 0.99850
        # close = 0.99850
        # upper_wick = high - max(open, close)
        # lower_wick = min(open, close) - low
        # upper_wick = 0.99870 - 0.99870 = 0.0
        # lower_wick = 0.99850 - 0.99850 = 0.0
        assert result_candle.upper_wick_size == CandleWickSize.WICK_NONE
        assert result_candle.lower_wick_size == CandleWickSize.WICK_NONE

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(1000):
            self.fc.update_raw(1.00000, 1.00000, 1.00000, 1.00000)

        # Act
        self.fc.reset()

        # Assert
        assert self.fc.initialized is False  # No assertion errors.

    def test_body_percent_calculation_issue(self):
        """
        Exposes the operator precedence issue in the original body_percent calculation.

        The expression 'open - close / length' was incorrectly parsed as 'open - (close / length)',
        instead of the correct form '(open - close) / length'.

        """
        # Arrange: Small price fluctuation, but not a Doji
        self.fc.update_raw(1.00000, 1.00010, 0.99990, 1.00005)

        # Act
        result_candle = self.fc.value
        result_vector = self.fc.vector

        # Assert: If the code is incorrect, body_percent will be very small or even negative.
        # The correct logic should classify body_size as SMALL or MEDIUM.
        assert result_candle.body_size in (
            CandleBodySize.BODY_SMALL,
            CandleBodySize.BODY_MEDIUM,
        ), f"Body percent calculation error exposed, vector={result_vector}"

    def test_length_indexing_issue(self):
        """
        Reveals the bug where the original code used _lengths[0] instead of the latest
        candle length _lengths[-1].
        """
        # Arrange: First update a large candle
        self.fc.update_raw(1.0, 1.01, 0.99, 1.005)  # Large candle
        # Then update a small candle
        self.fc.update_raw(1.0, 1.0001, 0.9999, 1.00005)

        # Act
        result_candle = self.fc.value
        result_vector = self.fc.vector

        # Assert: If _lengths[0] is used, the small candle will be misclassified as large
        assert (
            result_candle.size != CandleSize.SIZE_LARGE
        ), f"Length indexing bug exposed, vector={result_vector}"

    def test_doji_bar_body_zero_issue(self):
        """
        Test whether a Doji candle is correctly recognized when body_percent == 0.
        """
        # Arrange: Perfect Doji candle
        self.fc.update_raw(1.0, 1.0, 1.0, 1.0)
        result_candle = self.fc.value
        result_vector = self.fc.vector

        # Assert: If body_percent != 0, it will be incorrectly classified as non-Doji
        assert (
            result_candle.body_size == CandleBodySize.BODY_NONE
        ), f"Doji body detection failed, vector={result_vector}"
