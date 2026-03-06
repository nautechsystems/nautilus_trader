import pandas as pd

from nautilus_trader.analysis import RiskReturnRatio


def convert_to_daily_returns(values: list[float]) -> dict[int, float]:
    """
    Convert values to dict with daily-spaced timestamps.
    """
    one_day_ns = 86_400_000_000_000
    start_time = 1_600_000_000_000_000_000
    return {start_time + i * one_day_ns: v for i, v in enumerate(values)}


class TestRiskReturnRatioPortfolioStatistic:
    def test_name_returns_expected_returns_expected(self):
        # Arrange
        stat = RiskReturnRatio()

        # Act
        result = stat.name

        # Assert
        assert result == "Risk Return Ratio"

    def test_calculate_given_empty_series_returns_nan(self):
        # Arrange
        stat = RiskReturnRatio()
        data = convert_to_daily_returns([])

        # Act
        result = stat.calculate_from_returns(data)

        # Assert
        assert pd.isna(result)

    def test_calculate_given_mix_of_pnls1_returns_expected(self):
        # Arrange
        stat = RiskReturnRatio()
        data = convert_to_daily_returns([1.0, -1.0])

        # Act
        result = stat.calculate_from_returns(data)

        # Assert
        assert result == 0.0

    def test_calculate_given_mix_of_pnls2_returns_expected(self):
        # Arrange
        stat = RiskReturnRatio()
        data = convert_to_daily_returns([2.0, 2.0, 1.0, -1.0, -2.0])

        # Act
        result = stat.calculate_from_returns(data)

        # Assert
        assert result == 0.2201927530252721
