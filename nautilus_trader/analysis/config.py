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
Configuration for tearsheet generation and visualization.
"""

from __future__ import annotations

import msgspec

from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.common.config import PositiveInt


def _default_heights() -> list[float]:
    return [0.50, 0.22, 0.16, 0.12]


def _default_charts() -> list[str]:
    return [
        "run_info",
        "stats_table",
        "equity",
        "drawdown",
        "monthly_returns",
        "distribution",
        "rolling_sharpe",
        "yearly_returns",
    ]


class GridLayout(msgspec.Struct, frozen=True, kw_only=True):
    """
    Grid layout specification for tearsheet subplots.

    Parameters
    ----------
    rows : int, default 4
        Number of rows in the grid.
    cols : int, default 2
        Number of columns in the grid.
    heights : list[float], default [0.50, 0.22, 0.16, 0.12]
        Relative heights for each row (must sum to 1.0 or be proportional).
    vertical_spacing : float, default 0.10
        Vertical spacing between subplots (0.0 to 1.0).
    horizontal_spacing : float, default 0.10
        Horizontal spacing between subplots (0.0 to 1.0).

    """

    rows: int = 4
    cols: int = 2
    heights: list[float] = msgspec.field(default_factory=_default_heights)
    vertical_spacing: float = 0.10
    horizontal_spacing: float = 0.10


class TearsheetConfig(NautilusConfig, frozen=True, kw_only=True):
    """
    Configuration for tearsheet generation.

    Parameters
    ----------
    charts : list[str], default ["stats_table", "equity", "drawdown", "monthly_returns", "distribution", "rolling_sharpe", "yearly_returns"]
        List of chart names to include in the tearsheet.
        Available charts: "stats_table", "equity", "drawdown", "monthly_returns",
        "distribution", "rolling_sharpe", "yearly_returns".
    theme : str, default "plotly_white"
        Theme name for visualization styling.
        Built-in themes: "plotly_white", "plotly_dark", "nautilus".
    layout : GridLayout | None, default None
        Custom grid layout specification. If None, auto-calculated based on charts.
    title : str, default "NautilusTrader Backtest Results"
        Title for the tearsheet.
    include_benchmark : bool, default True
        Whether to include benchmark comparison in visualizations.
        Only applies when benchmark_returns data is provided.
    benchmark_name : str, default "Benchmark"
        Display name for the benchmark in visualizations.
    height : PositiveInt, default 1500
        Total height of the tearsheet in pixels.
    show_logo : bool, default True
        Whether to display NautilusTrader logo in the tearsheet.

    """

    charts: list[str] = msgspec.field(default_factory=_default_charts)
    theme: str = "plotly_white"
    layout: GridLayout | None = None
    title: str = "NautilusTrader Backtest Results"
    include_benchmark: bool = True
    benchmark_name: str = "Benchmark"
    height: PositiveInt = 1500
    show_logo: bool = True
