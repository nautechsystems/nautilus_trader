import pandas as pd
from numpy import float64

from nautilus_trader.analysis import ReturnsAverage
from tests.unit_tests.analysis.conftest import convert_series_to_dict


class TestReturnsAveragePortfolioStatistic:
    def test_name_returns_expected_returns_expected(self):
        # Arrange
        stat = ReturnsAverage()

        # Act
        result = stat.name

        # Assert
        assert result == "Average (Return)"

    def test_calculate_given_empty_series_returns_nan(self):
        # Arrange
        stat = ReturnsAverage()
        data = pd.Series([], dtype=float64)

        # Act
        result = stat.calculate_from_returns(convert_series_to_dict(data))

        # Assert
        assert pd.isna(result)

    def test_calculate_given_mix_of_pnls1_returns_expected(self):
        # Arrange
        stat = ReturnsAverage()
        data = pd.Series([1.0, -1.0], dtype=float64)

        # Act
        result = stat.calculate_from_returns(convert_series_to_dict(data))

        # Assert
        assert result == 0.0

    def test_calculate_given_mix_of_pnls2_returns_expected(self):
        # Arrange
        stat = ReturnsAverage()
        data = pd.Series([2.0, 2.0, 1.0, -1.0, -2.0], dtype=float64)

        # Act
        result = stat.calculate_from_returns(convert_series_to_dict(data))

        # Assert
        assert result == 0.4
