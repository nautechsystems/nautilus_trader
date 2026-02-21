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
"""
Configuration for tearsheet generation and visualization.
"""

from __future__ import annotations

from typing import Any

import msgspec

from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.common.config import PositiveInt


def _default_heights() -> list[float]:
    return [0.50, 0.22, 0.16, 0.12]


class TearsheetChart(msgspec.Struct, frozen=True, kw_only=True):
    """
    Base class for tearsheet chart configuration.

    Concrete chart classes define which chart to render (via `name`) and can expose
    additional arguments (via `kwargs`) that are passed into the chart renderer.

    """

    title: str | None = None

    @property
    def name(self) -> str:  # pragma: no cover (implemented by subclasses)
        raise NotImplementedError

    def kwargs(self) -> dict[str, Any]:
        return {}


class TearsheetRunInfoChart(TearsheetChart, frozen=True, kw_only=True):
    @property
    def name(self) -> str:
        return "run_info"


class TearsheetStatsTableChart(TearsheetChart, frozen=True, kw_only=True):
    @property
    def name(self) -> str:
        return "stats_table"


class TearsheetEquityChart(TearsheetChart, frozen=True, kw_only=True):
    @property
    def name(self) -> str:
        return "equity"


class TearsheetDrawdownChart(TearsheetChart, frozen=True, kw_only=True):
    @property
    def name(self) -> str:
        return "drawdown"


class TearsheetMonthlyReturnsChart(TearsheetChart, frozen=True, kw_only=True):
    @property
    def name(self) -> str:
        return "monthly_returns"


class TearsheetDistributionChart(TearsheetChart, frozen=True, kw_only=True):
    @property
    def name(self) -> str:
        return "distribution"


class TearsheetRollingSharpeChart(TearsheetChart, frozen=True, kw_only=True):
    @property
    def name(self) -> str:
        return "rolling_sharpe"


class TearsheetYearlyReturnsChart(TearsheetChart, frozen=True, kw_only=True):
    @property
    def name(self) -> str:
        return "yearly_returns"


class TearsheetBarsWithFillsChart(TearsheetChart, frozen=True, kw_only=True):
    """
    Render `bars_with_fills` for a specific bar type (string form accepted).
    """

    bar_type: str

    @property
    def name(self) -> str:
        return "bars_with_fills"

    def kwargs(self) -> dict[str, Any]:
        return {"bar_type": self.bar_type}


class TearsheetCustomChart(TearsheetChart, frozen=True, kw_only=True):
    """
    Configure a tearsheet chart by its registered name.

    This is intended for charts registered for tearsheet integration (i.e. present in
    the tearsheet chart spec registry).

    """

    chart: str
    args: dict[str, Any] = msgspec.field(default_factory=dict)

    @property
    def name(self) -> str:
        return self.chart

    def kwargs(self) -> dict[str, Any]:
        return self.args


def _default_charts() -> list[TearsheetChart]:
    return [
        TearsheetRunInfoChart(),
        TearsheetStatsTableChart(),
        TearsheetEquityChart(),
        TearsheetDrawdownChart(),
        TearsheetMonthlyReturnsChart(),
        TearsheetDistributionChart(),
        TearsheetRollingSharpeChart(),
        TearsheetYearlyReturnsChart(),
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
    charts : list[TearsheetChart], default built-ins
        Charts to include in the tearsheet, in order. Example:
        `charts=[TearsheetRunInfoChart(title="Run Info")]`.
    theme : str, default "plotly_white"
        Theme name for visualization styling.
        Built-in themes: "plotly_white", "plotly_dark", "nautilus", "nautilus_dark".
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

    charts: list[TearsheetChart] = msgspec.field(default_factory=_default_charts)
    theme: str = "plotly_white"
    layout: GridLayout | None = None
    title: str = "NautilusTrader Backtest Results"
    include_benchmark: bool = True
    benchmark_name: str = "Benchmark"
    height: PositiveInt = 1500
    show_logo: bool = True

    @property
    def chart_names(self) -> list[str]:
        return [c.name for c in self.charts]
