import math

import pandas as pd
from numpy import float64
from numpy import linspace
from numpy import nan

from nautilus_trader.analysis import SharpeRatio
from tests.unit_tests.analysis.conftest import convert_series_to_dict


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
        result = stat.calculate_from_returns(convert_series_to_dict(data))

        # Assert
        assert result
        assert pd.isna(result)

    def test_calculate_given_nan_series_returns_nan(self):
        # Arrange
        index = pd.date_range("1/1/2000", periods=10, freq="1D")
        data = pd.Series([nan] * 10, index=index, dtype=float64)

        stat = SharpeRatio()

        # Act
        result = stat.calculate_from_returns(convert_series_to_dict(data))

        # Assert
        assert result
        assert pd.isna(result)

    def test_calculate_given_mix_of_pnls1_returns_expected(self):
        # Arrange
        index = pd.date_range("1/1/2000", periods=2, freq="1D")
        data = pd.Series([1.0, -1.0], index=index, dtype=float64)

        stat = SharpeRatio()

        # Act
        result = stat.calculate_from_returns(convert_series_to_dict(data))

        # Assert
        assert result == 0.0

    def test_calculate_given_mix_of_pnls2_returns_expected(self):
        # Arrange
        index = pd.date_range("1/1/2000", periods=10, freq="12h")
        data = pd.Series(linspace(0.1, 1, 10), index=index, dtype=float64)

        stat = SharpeRatio()

        # Act
        result = stat.calculate_from_returns(convert_series_to_dict(data))

        # Assert
        assert result
        assert math.isclose(result, 27.6097808756245, rel_tol=1e-9)
