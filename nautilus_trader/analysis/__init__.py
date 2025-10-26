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
from nautilus_trader.analysis.reporter import ReportProvider
from nautilus_trader.analysis.statistic import PortfolioStatistic
from nautilus_trader.core.nautilus_pyo3 import AvgLoser
from nautilus_trader.core.nautilus_pyo3 import AvgWinner
from nautilus_trader.core.nautilus_pyo3 import Expectancy
from nautilus_trader.core.nautilus_pyo3 import LongRatio
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
    "AvgLoser",
    "AvgWinner",
    "Expectancy",
    "LongRatio",
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
    "WinRate",
]
