# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
"""
The `analysis` subpackage groups components relating to trading performance statistics
and analysis.
"""

from nautilus_trader.analysis.analyzer import PortfolioAnalyzer
from nautilus_trader.analysis.config import GridLayout
from nautilus_trader.analysis.config import TearsheetConfig
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
    "TearsheetConfig",
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
