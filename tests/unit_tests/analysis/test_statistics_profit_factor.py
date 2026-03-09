import pandas as pd
from numpy import float64

from nautilus_trader.analysis import ProfitFactor
from tests.unit_tests.analysis.conftest import convert_series_to_dict


class TestProfitFactorPortfolioStatistic:
    def test_name_returns_expected_returns_expected(self):
        # Arrange
        stat = ProfitFactor()

        # Act
        result = stat.name

        # Assert
        assert result == "Profit Factor"

    def test_calculate_given_empty_series_returns_nan(self):
        # Arrange
        stat = ProfitFactor()
        data = pd.Series([0.0], dtype=float64)

        # Act
        result = stat.calculate_from_returns(convert_series_to_dict(data))

        # Assert
        assert pd.isna(result)

    def test_calculate_given_mix_of_pnls_returns_expected(self):
        # Arrange
        stat = ProfitFactor()
        data = pd.Series([3.0, 2.0, 1.0, -1.0, -2.0], dtype=float64)

        # Act
        result = stat.calculate_from_returns(convert_series_to_dict(data))

        # Assert
        assert result == 2.0
