"""
The `analysis` subpackage groups components relating to trading performance statistics
and analysis.
"""

from nautilus_trader.analysis.analyzer import PortfolioAnalyzer
from nautilus_trader.analysis.config import GridLayout
from nautilus_trader.analysis.config import TearsheetBarsWithFillsChart
from nautilus_trader.analysis.config import TearsheetChart
from nautilus_trader.analysis.config import TearsheetConfig
from nautilus_trader.analysis.config import TearsheetCustomChart
from nautilus_trader.analysis.config import TearsheetDistributionChart
from nautilus_trader.analysis.config import TearsheetDrawdownChart
from nautilus_trader.analysis.config import TearsheetEquityChart
from nautilus_trader.analysis.config import TearsheetMonthlyReturnsChart
from nautilus_trader.analysis.config import TearsheetRollingSharpeChart
from nautilus_trader.analysis.config import TearsheetRunInfoChart
from nautilus_trader.analysis.config import TearsheetStatsTableChart
from nautilus_trader.analysis.config import TearsheetYearlyReturnsChart
from nautilus_trader.analysis.reporter import ReportProvider
from nautilus_trader.analysis.statistic import PortfolioStatistic
from nautilus_trader.analysis.tearsheet import create_drawdown_chart
from nautilus_trader.analysis.tearsheet import create_equity_curve
from nautilus_trader.analysis.tearsheet import create_monthly_returns_heatmap
from nautilus_trader.analysis.tearsheet import create_returns_distribution
from nautilus_trader.analysis.tearsheet import create_rolling_sharpe
from nautilus_trader.analysis.tearsheet import create_tearsheet
from nautilus_trader.analysis.tearsheet import create_tearsheet_from_stats
from nautilus_trader.analysis.tearsheet import create_yearly_returns
from nautilus_trader.analysis.tearsheet import get_chart
from nautilus_trader.analysis.tearsheet import list_charts
from nautilus_trader.analysis.tearsheet import register_chart
from nautilus_trader.analysis.themes import get_theme
from nautilus_trader.analysis.themes import list_themes
from nautilus_trader.analysis.themes import register_theme
from nautilus_trader.core.nautilus_pyo3 import CAGR
from nautilus_trader.core.nautilus_pyo3 import AvgLoser
from nautilus_trader.core.nautilus_pyo3 import AvgWinner
from nautilus_trader.core.nautilus_pyo3 import CalmarRatio
from nautilus_trader.core.nautilus_pyo3 import Expectancy
from nautilus_trader.core.nautilus_pyo3 import LongRatio
from nautilus_trader.core.nautilus_pyo3 import MaxDrawdown
from nautilus_trader.core.nautilus_pyo3 import MaxLoser
from nautilus_trader.core.nautilus_pyo3 import MaxWinner
from nautilus_trader.core.nautilus_pyo3 import MinLoser
from nautilus_trader.core.nautilus_pyo3 import MinWinner
from nautilus_trader.core.nautilus_pyo3 import ProfitFactor
from nautilus_trader.core.nautilus_pyo3 import ReturnsAverage
from nautilus_trader.core.nautilus_pyo3 import ReturnsAverageLoss
from nautilus_trader.core.nautilus_pyo3 import ReturnsAverageWin
from nautilus_trader.core.nautilus_pyo3 import ReturnsVolatility
from nautilus_trader.core.nautilus_pyo3 import RiskReturnRatio
from nautilus_trader.core.nautilus_pyo3 import SharpeRatio
from nautilus_trader.core.nautilus_pyo3 import SortinoRatio
from nautilus_trader.core.nautilus_pyo3 import WinRate


__all__ = [
    "CAGR",
    "AvgLoser",
    "AvgWinner",
    "CalmarRatio",
    "Expectancy",
    "GridLayout",
    "LongRatio",
    "MaxDrawdown",
    "MaxLoser",
    "MaxWinner",
    "MinLoser",
    "MinWinner",
    "PortfolioAnalyzer",
    "PortfolioStatistic",
    "ProfitFactor",
    "ReportProvider",
    "ReturnsAverage",
    "ReturnsAverageLoss",
    "ReturnsAverageWin",
    "ReturnsVolatility",
    "RiskReturnRatio",
    "SharpeRatio",
    "SortinoRatio",
    "TearsheetBarsWithFillsChart",
    "TearsheetChart",
    "TearsheetConfig",
    "TearsheetCustomChart",
    "TearsheetDistributionChart",
    "TearsheetDrawdownChart",
    "TearsheetEquityChart",
    "TearsheetMonthlyReturnsChart",
    "TearsheetRollingSharpeChart",
    "TearsheetRunInfoChart",
    "TearsheetStatsTableChart",
    "TearsheetYearlyReturnsChart",
    "WinRate",
    "create_drawdown_chart",
    "create_equity_curve",
    "create_monthly_returns_heatmap",
    "create_returns_distribution",
    "create_rolling_sharpe",
    "create_tearsheet",
    "create_tearsheet_from_stats",
    "create_yearly_returns",
    "get_chart",
    "get_theme",
    "list_charts",
    "list_themes",
    "register_chart",
    "register_theme",
]
