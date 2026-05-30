# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
import pytest

from nautilus_trader.analysis.statistic import PortfolioStatistic


class TestPortfolioStatistic:
    def test_fully_qualified_name_returns_expected(self):
        # Arrange, Act
        result = PortfolioStatistic.fully_qualified_name()

        # Assert
        assert result == "nautilus_trader.analysis.statistic:PortfolioStatistic"

    def test_name_returns_expected_returns_expected(self):
        # Arrange
        stat = PortfolioStatistic()

        # Act
        result = stat.name

        # Assert
        assert result == "Portfolio Statistic"

    def test_downsample_to_daily_bins_compounds_intraday_returns(self):
        # Two intraday returns in the same UTC day: +5% then -5%.
        #   arithmetic sum:  0.05 + (-0.05) = 0.00      (incorrect)
        #   geometric chain: (1.05)(0.95) - 1 = -0.0025 (correct)
        stat = PortfolioStatistic()
        day = pd.Timestamp("2024-01-02", tz="UTC")
        returns = pd.Series(
            [0.05, -0.05],
            index=[day, day + pd.Timedelta(hours=1)],
        )

        daily = stat._downsample_to_daily_bins(returns)

        assert len(daily) == 1
        assert daily.iloc[0] == pytest.approx(-0.0025)

    def test_downsample_to_daily_bins_daily_inputs_unchanged(self):
        # For one-return-per-day inputs the bin value equals the input return,
        # so existing callers already operating on daily returns see no change.
        stat = PortfolioStatistic()
        index = pd.date_range("2024-01-02", periods=3, freq="D", tz="UTC")
        returns = pd.Series([0.01, -0.02, 0.015], index=index)

        daily = stat._downsample_to_daily_bins(returns)

        assert len(daily) == 3
        assert daily.iloc[0] == pytest.approx(0.01)
        assert daily.iloc[1] == pytest.approx(-0.02)
        assert daily.iloc[2] == pytest.approx(0.015)
