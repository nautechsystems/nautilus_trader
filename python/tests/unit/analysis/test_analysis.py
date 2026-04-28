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

import pytest

from nautilus_trader.analysis import CAGR
from nautilus_trader.analysis import AvgLoser
from nautilus_trader.analysis import AvgWinner
from nautilus_trader.analysis import CalmarRatio
from nautilus_trader.analysis import Expectancy
from nautilus_trader.analysis import LongRatio
from nautilus_trader.analysis import MaxDrawdown
from nautilus_trader.analysis import MaxLoser
from nautilus_trader.analysis import MaxWinner
from nautilus_trader.analysis import MinLoser
from nautilus_trader.analysis import MinWinner
from nautilus_trader.analysis import PortfolioAnalyzer
from nautilus_trader.analysis import ProfitFactor
from nautilus_trader.analysis import ReturnsAverage
from nautilus_trader.analysis import ReturnsAverageLoss
from nautilus_trader.analysis import ReturnsAverageWin
from nautilus_trader.analysis import ReturnsVolatility
from nautilus_trader.analysis import RiskReturnRatio
from nautilus_trader.analysis import SharpeRatio
from nautilus_trader.analysis import SortinoRatio
from nautilus_trader.analysis import WinRate


NO_ARG_STATISTICS = [
    (AvgLoser, "Avg Loser"),
    (AvgWinner, "Avg Winner"),
    (Expectancy, "Expectancy"),
    (LongRatio, "Long Ratio"),
    (MaxDrawdown, "Max Drawdown"),
    (MaxLoser, "Max Loser"),
    (MaxWinner, "Max Winner"),
    (MinLoser, "Min Loser"),
    (MinWinner, "Min Winner"),
    (ProfitFactor, "Profit Factor"),
    (ReturnsAverage, "Average (Return"),
    (ReturnsAverageLoss, "Average Loss (Return"),
    (ReturnsAverageWin, "Average Win (Return"),
    (RiskReturnRatio, "Risk Return Ratio"),
    (WinRate, "Win Rate"),
]

PERIOD_STATISTICS = [
    (CAGR, "CAGR"),
    (CalmarRatio, "Calmar Ratio"),
    (ReturnsVolatility, "Returns Volatility"),
    (SharpeRatio, "Sharpe Ratio"),
    (SortinoRatio, "Sortino Ratio"),
]


@pytest.mark.parametrize(("cls", "expected_prefix"), NO_ARG_STATISTICS)
def test_statistic_construction_and_name(cls, expected_prefix):
    stat = cls()

    assert stat.name.startswith(expected_prefix)


@pytest.mark.parametrize(("cls", "expected_prefix"), PERIOD_STATISTICS)
def test_period_statistic_default_construction_and_name(cls, expected_prefix):
    stat = cls()

    assert stat.name.startswith(expected_prefix)


@pytest.mark.parametrize(("cls", "expected_prefix"), PERIOD_STATISTICS)
def test_period_statistic_custom_period(cls, expected_prefix):
    stat = cls(period=30)

    assert "30" in stat.name


@pytest.mark.parametrize(
    ("cls", "_expected_prefix"),
    NO_ARG_STATISTICS + PERIOD_STATISTICS,
)
def test_pyo3_statistic_exposes_full_calculate_surface(cls, _expected_prefix):
    stat = cls()

    # Every pyo3 statistic must expose all three calculate_from_* methods so the
    # Python PortfolioAnalyzer can iterate registered stats without AttributeError.
    # Methods return None for inputs that do not apply to the underlying calculation.
    assert callable(stat.calculate_from_returns)
    assert callable(stat.calculate_from_realized_pnls)
    assert callable(stat.calculate_from_positions)


def test_long_ratio_custom_precision():
    stat = LongRatio(precision=4)

    assert stat.name.startswith("Long Ratio")


def test_portfolio_analyzer_construction():
    analyzer = PortfolioAnalyzer()

    assert analyzer.currencies() == []
    assert analyzer.returns() == {}
    assert analyzer.position_returns() == {}
    assert analyzer.portfolio_returns() == {}


def test_portfolio_analyzer_register_and_deregister_statistic():
    analyzer = PortfolioAnalyzer()
    stat = SharpeRatio()

    analyzer.register_statistic(stat)

    assert analyzer.statistic(stat.name) is not None

    analyzer.deregister_statistic(stat)

    assert analyzer.statistic(stat.name) is None


def test_portfolio_analyzer_deregister_all_statistics():
    analyzer = PortfolioAnalyzer()
    analyzer.register_statistic(SharpeRatio())
    analyzer.register_statistic(WinRate())

    analyzer.deregister_statistics()

    assert analyzer.get_performance_stats_returns() == {}


def test_portfolio_analyzer_add_return_and_stats():
    analyzer = PortfolioAnalyzer()
    analyzer.register_statistic(ReturnsAverage())

    analyzer.add_return(1_000_000_000, 0.01)
    analyzer.add_return(2_000_000_000, -0.005)

    stats = analyzer.get_performance_stats_returns()

    assert len(stats) > 0


def test_portfolio_analyzer_add_position_return():
    analyzer = PortfolioAnalyzer()

    analyzer.add_position_return(1_000_000_000, 0.02)

    assert analyzer.position_returns() != {}


def test_portfolio_analyzer_reset():
    analyzer = PortfolioAnalyzer()
    analyzer.add_return(1_000_000_000, 0.01)

    analyzer.reset()

    assert analyzer.returns() == {}
    assert analyzer.position_returns() == {}


def test_portfolio_analyzer_formatted_stats_empty():
    analyzer = PortfolioAnalyzer()

    assert analyzer.get_stats_returns_formatted() == []
    assert analyzer.get_stats_position_returns_formatted() == []
    assert analyzer.get_stats_portfolio_returns_formatted() == []
    assert analyzer.get_stats_general_formatted() == []
