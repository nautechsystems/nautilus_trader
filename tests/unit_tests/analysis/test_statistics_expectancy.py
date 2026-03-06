import math

import pandas as pd
from numpy import float64

from nautilus_trader.analysis import Expectancy


class TestExpectancyPortfolioStatistic:
    def test_name_returns_expected_returns_expected(self):
        # Arrange
        stat = Expectancy()

        # Act
        result = stat.name

        # Assert
        assert result == "Expectancy"

    def test_calculate_given_empty_series_returns_nan(self):
        # Arrange
        stat = Expectancy()
        data = pd.Series(dtype=float64)

        # Act
        result = stat.calculate_from_realized_pnls(data)

        # Assert
        assert math.isnan(result)

    def test_calculate_given_insufficient_data_returns_zero(self):
        # Arrange
        stat = Expectancy()
        data = pd.Series([0.0, 0.0], dtype=float64)

        # Act
        result = stat.calculate_from_realized_pnls(data)

        # Assert
        assert result == 0.0

    def test_calculate_given_one_winner_one_loser_returns_zero(self):
        # Arrange
        stat = Expectancy()
        data = pd.Series([1.0, -1.0], dtype=float64)

        # Act
        result = stat.calculate_from_realized_pnls(data)

        # Assert
        assert result == 0.0

    def test_calculate_given_mix_of_pnls_returns_expected(self):
        # Arrange
        stat = Expectancy()
        data = pd.Series([2.0, 1.5, 1.0, 0.5, -1.0], dtype=float64)

        # Act
        result = stat.calculate_from_realized_pnls(data)

        # Assert
        assert result == 0.8
