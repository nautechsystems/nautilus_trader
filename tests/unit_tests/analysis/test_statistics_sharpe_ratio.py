# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import pandas as pd
from numpy import float64
from numpy import linspace
from numpy import nan

from nautilus_trader.analysis.statistics.sharpe_ratio import SharpeRatio


class TestSharpeRatioPortfolioStatistic:
    def test_name_returns_expected_returns_expected(self):
        # Arrange
        stat = SharpeRatio()

        # Act
        result = stat.name

        # Assert
        assert result == "Sharpe Ratio (252 days)"

    def test_calculate_given_empty_series_returns_nan(self):
        # Arrange
        data = pd.Series([], dtype=float64)

        stat = SharpeRatio()

        # Act
        result = stat.calculate_from_returns(data)

        # Assert
        assert pd.isna(result)

    def test_calculate_given_nan_series_returns_nan(self):
        # Arrange
        index = pd.date_range("1/1/2000", periods=10, freq="1D")
        data = pd.Series([nan] * 10, index=index, dtype=float64)

        stat = SharpeRatio()

        # Act
        result = stat.calculate_from_returns(data)

        # Assert
        assert pd.isna(result)

    def test_calculate_given_mix_of_pnls1_returns_expected(self):
        # Arrange
        index = pd.date_range("1/1/2000", periods=2, freq="1D")
        data = pd.Series([1.0, -1.0], index=index, dtype=float64)

        stat = SharpeRatio()

        # Act
        result = stat.calculate_from_returns(data)

        # Assert
        assert result == 0.0

    def test_calculate_given_mix_of_pnls2_returns_expected(self):
        # Arrange
        index = pd.date_range("1/1/2000", periods=10, freq="12H")
        data = pd.Series(linspace(0.1, 1, 10), index=index, dtype=float64)

        stat = SharpeRatio()

        # Act
        result = stat.calculate_from_returns(data)

        # Assert
        assert result == 27.6097808756245
