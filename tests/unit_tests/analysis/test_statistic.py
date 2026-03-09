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
