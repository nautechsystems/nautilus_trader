import math

import pandas as pd
from numpy import float64

from nautilus_trader.analysis import MinWinner


class TestMinWinnerPortfolioStatistic:
    def test_name_returns_expected_returns_expected(self):
        # Arrange
        stat = MinWinner()

        # Act
        result = stat.name

        # Assert
        assert result == "Min Winner"

    def test_calculate_given_empty_series_returns_nan(self):
        # Arrange
        stat = MinWinner()
        data = pd.Series([], dtype=float64)

        # Act
        result = stat.calculate_from_realized_pnls(data)

        # Assert
        assert math.isnan(result)

    def test_calculate_given_mix_of_pnls1_returns_expected(self):
        # Arrange
        stat = MinWinner()
        data = pd.Series([1.0, -1.0], dtype=float64)

        # Act
        result = stat.calculate_from_realized_pnls(data)

        # Assert
        assert result == 1.0

    def test_calculate_given_mix_of_pnls2_returns_expected(self):
        # Arrange
        stat = MinWinner()
        data = pd.Series([2.0, 2.0, 1.0, -1.0, -2.0], dtype=float64)

        # Act
        result = stat.calculate_from_realized_pnls(data)

        # Assert
        assert result == 1.0
