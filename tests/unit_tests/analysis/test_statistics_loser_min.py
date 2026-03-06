import math

import pandas as pd
from numpy import float64

from nautilus_trader.analysis import MinLoser


class TestMinLoserPortfolioStatistic:
    def test_name_returns_expected_returns_expected(self):
        # Arrange
        stat = MinLoser()

        # Act
        result = stat.name

        # Assert
        assert result == "Min Loser"

    def test_calculate_given_empty_series_returns_nan(self):
        # Arrange
        stat = MinLoser()
        data = pd.Series(dtype=float64)

        # Act
        result = stat.calculate_from_realized_pnls(data)

        # Assert
        assert math.isnan(result)

    def test_calculate_given_mix_of_pnls_returns_expected(self):
        # Arrange
        stat = MinLoser()
        data = pd.Series([2.0, 1.0, -1.0, -2.0], dtype=float64)

        # Act
        result = stat.calculate_from_realized_pnls(data)

        # Assert
        assert result == -1.0
