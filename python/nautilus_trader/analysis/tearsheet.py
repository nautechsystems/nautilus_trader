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
Backtest visualization and tearsheet generation using Plotly.

This module provides functions to create interactive tearsheets and plots from backtest
results, using backtest result statistics and report DataFrames.

"""

from __future__ import annotations

import math
import numbers
import re
from collections.abc import Mapping
from difflib import get_close_matches
from pathlib import Path
from typing import TYPE_CHECKING

from nautilus_trader.analysis.config import GridLayout
from nautilus_trader.analysis.config import TearsheetConfig
from nautilus_trader.analysis.themes import get_theme
from nautilus_trader.core import NAUTILUS_VERSION
from nautilus_trader.core import unix_nanos_to_iso8601
from nautilus_trader.model import AccountId
from nautilus_trader.model import AggregationSource
from nautilus_trader.model import BarType


if TYPE_CHECKING:
    from collections.abc import Callable
    from types import ModuleType
    from typing import Any

    import pandas as pd

    from nautilus_trader.analysis.config import TearsheetChart
    from nautilus_trader.backtest import BacktestEngine
    from nautilus_trader.backtest import BacktestNode
    from nautilus_trader.backtest import BacktestResult
    from nautilus_trader.common import Cache
    from nautilus_trader.model import Currency
    from nautilus_trader.model import PortfolioSnapshot
    from nautilus_trader.model import Venue
    from nautilus_trader.portfolio import Portfolio


def _require_pandas() -> ModuleType:
    try:
        import pandas as pd
    except ImportError as e:
        raise ImportError(
            "pandas is required for visualization; install it with "
            "`pip install nautilus_trader[visualization]`",
        ) from e

    return pd


if not TYPE_CHECKING:

    class _PandasProxy:
        def __getattr__(self, name: str) -> Any:
            return getattr(_require_pandas(), name)

    pd = _PandasProxy()


try:
    import plotly.graph_objects as go
    import plotly.io as pio
    from plotly.subplots import make_subplots

    PLOTLY_AVAILABLE = True
except ImportError:
    PLOTLY_AVAILABLE = False

    if not TYPE_CHECKING:
        go = None  # type: ignore[assignment]


TRADING_DAYS_PER_YEAR = 252

_GRID_ONE_ROW_MAX_CHARTS = 2
_GRID_TWO_ROWS_MAX_CHARTS = 4
_GRID_THREE_ROWS_MAX_CHARTS = 6
_GRID_DEFAULT_ROWS = 4
_GRID_DEFAULT_SPACING = 0.08

# Sourced from GridLayout defaults (the default 8-chart grid).
_GRID_DEFAULT_HEIGHTS = list(GridLayout().heights)

_STATIC_IMAGE_SUFFIXES = frozenset({".png", ".jpg", ".jpeg", ".webp", ".svg", ".pdf"})

# Inter for titles, IBM Plex Mono for data; both degrade to system fonts when absent.
_FONT_HEADING = "Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif"
_FONT_MONO = "'IBM Plex Mono', 'SF Mono', Consolas, Menlo, monospace"

_EMPTY_DATA_MESSAGE = "No data available"

_HEX_SHORT_LENGTH = 3
_HEX_FULL_LENGTH = 6

_CHART_REGISTRY: dict[str, Callable] = {}


def _require_not_none(value: object, name: str) -> None:
    if value is None:
        raise ValueError(f"{name} must not be None")


def _format_optional_iso8601(timestamp_ns: int | None) -> str:
    if timestamp_ns is None:
        return "N/A"
    return unix_nanos_to_iso8601(timestamp_ns, nanos_precision=False)


def _format_optional_duration(start_ns: int | None, end_ns: int | None) -> str:
    if start_ns is None or end_ns is None:
        return "N/A"
    return str(pd.Timedelta(end_ns - start_ns, unit="ns"))


def _to_returns_series(returns: pd.Series | Mapping | list | None) -> pd.Series:
    pandas: Any = _require_pandas()
    if returns is None:
        return pandas.Series(dtype=float)

    if isinstance(returns, pandas.Series):
        series = returns.copy()
    elif isinstance(returns, Mapping):
        series = pandas.Series(dict(returns), dtype=float)
    else:
        series = pandas.Series(returns, dtype=float)

    if series.empty:
        return pandas.Series(dtype=float)

    if not isinstance(series.index, pandas.DatetimeIndex):
        is_epoch_ns = all(
            isinstance(value, numbers.Real) and not isinstance(value, bool)
            for value in series.index
        )
        series.index = pandas.to_datetime(
            series.index,
            unit="ns" if is_epoch_ns else None,
            utc=True,
        )

    return series.sort_index()


def _write_figure(fig: go.Figure, output_path: str) -> None:
    # Static image export uses Kaleido (install via `nautilus_trader[visualization]`).
    path = Path(output_path)
    path.parent.mkdir(parents=True, exist_ok=True)
    suffix = path.suffix.lower()
    if suffix in _STATIC_IMAGE_SUFFIXES:
        # Kaleido serializes via orjson, which cannot encode the pandas Timestamp
        # values on datetime axes; a round-trip through plotly's JSON encoder
        # converts them so the resulting trace data is serializable.
        export = pio.from_json(pio.to_json(fig))
        if export.layout.width is None:
            export.update_layout(width=1200)
        export.write_image(output_path)
    else:
        fig.write_html(output_path)


def _hex_to_rgb(hex_color: str) -> tuple[int, int, int]:
    """
    Parse a hex color code into an RGB triple.

    Parameters
    ----------
    hex_color : str
        Hex color code (e.g., "#d62728" or "#fff").

    Returns
    -------
    tuple[int, int, int]
        The red, green, and blue components.

    Raises
    ------
    ValueError
        If the value is not a 3- or 6-digit hex code.

    """
    code = hex_color.lstrip("#")
    if len(code) == _HEX_SHORT_LENGTH:
        code = "".join(char * 2 for char in code)
    if len(code) != _HEX_FULL_LENGTH:
        raise ValueError(f"Color must be a hex code like '#rrggbb', was {hex_color!r}")
    try:
        return int(code[0:2], 16), int(code[2:4], 16), int(code[4:6], 16)
    except ValueError as e:
        raise ValueError(f"Color must be a hex code like '#rrggbb', was {hex_color!r}") from e


def _hex_to_rgba(hex_color: str, alpha: float = 1.0) -> str:
    """
    Convert hex color to rgba format.

    Parameters
    ----------
    hex_color : str
        Hex color code (e.g., "#d62728").
    alpha : float, default 1.0
        Alpha/opacity value, clamped to 0.0 to 1.0.

    Returns
    -------
    str
        RGBA color string (e.g., "rgba(214, 39, 40, 0.3)").

    """
    r, g, b = _hex_to_rgb(hex_color)
    return f"rgba({r}, {g}, {b}, {min(max(alpha, 0.0), 1.0)})"


_LIGHT_LUMINANCE_THRESHOLD = 0.5


def _is_light_background(bg: str) -> bool:
    """
    Judge whether a theme background needs dark foreground colors.
    """
    try:
        r, g, b = _hex_to_rgb(bg)
    except ValueError:
        return False
    luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255
    return luminance > _LIGHT_LUMINANCE_THRESHOLD


def _padded_range(values_min: float, values_max: float, pad: float = 0.05) -> list[float]:
    """
    Build a padded axis range covering the data without forcing zero in view.

    Filled traces with ``fill="tozeroy"`` make Plotly's autorange include zero,
    which squashes series that live far from it (like an equity index starting
    near 1.0). An explicit range keeps the fill and the detail.

    """
    span = values_max - values_min
    if span <= 0:
        delta = abs(values_max) * pad if values_max != 0 else pad
        return [values_min - delta, values_max + delta]
    return [values_min - span * pad, values_max + span * pad]


def _mix_hex(first: str, second: str, weight: float) -> str:
    """
    Blend two hex colors; a weight of 1.0 returns the first color.
    """
    r1, g1, b1 = _hex_to_rgb(first)
    r2, g2, b2 = _hex_to_rgb(second)
    w = min(max(weight, 0.0), 1.0)
    r = round(r1 * w + r2 * (1.0 - w))
    g = round(g1 * w + g2 * (1.0 - w))
    b = round(b1 * w + b2 * (1.0 - w))
    return f"#{r:02x}{g:02x}{b:02x}"


def _group_thousands(text: str) -> str:
    """
    Add comma thousand separators to a plain decimal string.

    Values that are not plain decimals (identifiers, timestamps, durations) are returned
    unchanged, and decimal places are preserved exactly.

    """
    match = re.match(r"^([+-]?)(\d+)(\.\d+)?$", text.strip())
    if match is None:
        return text
    sign, int_part, frac_part = match.groups()
    if len(int_part) > 1 and int_part.startswith("0"):
        return text
    return f"{sign}{int(int_part):,}{frac_part or ''}"


def _format_info_value(value: Any) -> str:
    """
    Format a run/account info value, grouping thousands where numeric.
    """
    if isinstance(value, bool):
        return str(value)
    if isinstance(value, int):
        return f"{value:,}"
    if isinstance(value, float):
        return _group_thousands(str(value))
    if isinstance(value, str):
        return _group_thousands(value)
    return str(value)


def _heatmap_colorscale(theme_config: dict[str, Any]) -> list[list[float | str]]:
    """
    Build a muted diverging colorscale from the theme's semantic colors.
    """
    colors = theme_config["colors"]
    midpoint = colors.get("table_section", colors["background"])
    low = _mix_hex(colors["negative"], midpoint, 0.72)
    high = _mix_hex(colors["positive"], midpoint, 0.72)
    return [[0.0, low], [0.5, midpoint], [1.0, high]]


_MONTH_ABBR = [
    "Jan",
    "Feb",
    "Mar",
    "Apr",
    "May",
    "Jun",
    "Jul",
    "Aug",
    "Sep",
    "Oct",
    "Nov",
    "Dec",
]


def _monthly_pivot(returns: pd.Series, *, compounding: bool) -> Any:
    """
    Pivot period returns into a year-by-month table for heatmaps.
    """
    monthly = _aggregate_period_returns(returns, "ME", compounding=compounding)
    pivot = pd.DataFrame(
        {
            "Year": monthly.index.year,
            "Month": monthly.index.month,
            "Return": monthly.to_numpy(),
        },
    )
    return pivot.pivot_table(index="Year", columns="Month", values="Return")


def _symmetric_heatmap_range(values: Any) -> tuple[float, float]:
    """
    Return a symmetric (min, max) range around zero for diverging heatmaps.
    """
    magnitudes = [abs(value) for row in values for value in row if not math.isnan(value)]
    vmax = max(magnitudes, default=0.0) or 1.0
    return -vmax, vmax


def _heatmap_trace(
    heatmap_data: Any,
    theme_config: dict[str, Any],
    *,
    showscale: bool,
    hovertemplate: str | None = "<b>%{y} %{x}</b><br>Return: %{z:.2f}%<extra></extra>",
) -> go.Heatmap:
    """
    Build a monthly returns heatmap trace with symmetric diverging colors.
    """
    colors = theme_config["colors"]
    zmin, zmax = _symmetric_heatmap_range(heatmap_data.values)
    colorbar: dict[str, Any] | None = None
    if showscale:
        colorbar = {
            "thickness": 12,
            "len": 0.85,
            "outlinewidth": 0,
            "tickfont": {"family": _FONT_MONO, "size": 10, "color": colors["muted"]},
        }
    return go.Heatmap(
        z=heatmap_data.values,
        x=[_MONTH_ABBR[int(m) - 1] for m in heatmap_data.columns],
        y=heatmap_data.index.astype(str),
        colorscale=_heatmap_colorscale(theme_config),
        zmid=0,
        zmin=zmin,
        zmax=zmax,
        text=heatmap_data.values,
        texttemplate="%{text:.1f}%",
        textfont={"family": _FONT_MONO, "size": 9, "color": colors["text"]},
        xgap=1,
        ygap=1,
        showscale=showscale,
        colorbar=colorbar,
        hovertemplate=hovertemplate,
    )


def _normalize_theme_config(theme_config: dict[str, Any]) -> dict[str, Any]:
    """
    Ensure theme config has all required color keys with sensible defaults.

    This provides backward compatibility for themes registered before
    text- and table-specific colors were added.

    Parameters
    ----------
    theme_config : dict[str, Any]
        Theme configuration from get_theme().

    Returns
    -------
    dict[str, Any]
        Normalized theme configuration with all required keys.

    """
    colors = theme_config.get("colors", {})
    bg = colors.get("background", "#ffffff")
    is_light = _is_light_background(bg)

    if "text" not in colors:
        colors["text"] = "#333333" if is_light else "#e5e7eb"
    if "muted" not in colors:
        colors["muted"] = "#737373" if is_light else "#9ca3af"
    if "table_header" not in colors:
        colors["table_header"] = colors.get("table_section", colors.get("grid", "#e0e0e0"))
    if "table_section" not in colors:
        colors["table_section"] = colors.get("grid", "#e0e0e0")
    if "table_row_odd" not in colors:
        colors["table_row_odd"] = colors.get("grid", "#f0f0f0")
    if "table_row_even" not in colors:
        colors["table_row_even"] = colors.get("background", "#ffffff")
    if "table_text" not in colors:
        colors["table_text"] = "#000000" if is_light else "#eeeeee"

    theme_config["colors"] = colors
    return theme_config


def _style_axes(
    fig: go.Figure,
    theme_config: dict[str, Any],
    row: int | None = None,
    col: int | None = None,
) -> None:
    """
    Apply theme grid, tick, and title fonts to axes (all, or one subplot cell).
    """
    colors = theme_config["colors"]
    axis_line = _hex_to_rgba(colors["neutral"], 0.45)
    tickfont = {"family": _FONT_MONO, "size": 10, "color": colors["muted"]}
    title_font = {"family": _FONT_HEADING, "size": 11, "color": colors["text"]}
    kwargs: dict[str, Any] = {} if row is None else {"row": row, "col": col}
    fig.update_xaxes(
        gridcolor=colors["grid"],
        zeroline=False,
        showline=True,
        linecolor=axis_line,
        tickfont=tickfont,
        title_font=title_font,
        **kwargs,
    )
    fig.update_yaxes(
        gridcolor=colors["grid"],
        zeroline=False,
        showline=True,
        linecolor=axis_line,
        tickfont=tickfont,
        title_font=title_font,
        **kwargs,
    )


def _base_layout(theme_config: dict[str, Any]) -> dict[str, Any]:
    """
    Shared theme template, fonts, legend, and hover styling for all figures.
    """
    colors = theme_config["colors"]
    return {
        "template": theme_config["template"],
        "paper_bgcolor": colors["background"],
        "plot_bgcolor": colors["background"],
        "font": {"family": _FONT_MONO, "size": 11, "color": colors["text"]},
        "hoverlabel": {
            "font": {"family": _FONT_MONO, "size": 11, "color": colors["text"]},
            "bgcolor": colors["background"],
            "bordercolor": colors["grid"],
        },
        "legend": {
            "font": {"family": _FONT_MONO, "size": 11, "color": colors["text"]},
            "orientation": "h",
            "yanchor": "bottom",
            "y": 1.02,
            "xanchor": "right",
            "x": 1.0,
        },
    }


def _title_block(title: str | None, color: str, size: int) -> dict[str, Any]:
    """
    Left-aligned figure title block shared by all figures.
    """
    return {
        "text": title,
        "x": 0.02,
        "xanchor": "left",
        "font": {"family": _FONT_HEADING, "size": size, "color": color},
    }


def _style_figure(
    fig: go.Figure,
    theme_config: dict[str, Any],
    title: str | None = None,
    height: int | None = None,
    title_size: int = 16,
) -> None:
    """
    Apply theme fonts, colors, legend, and margins shared by standalone figures.
    """
    layout = _base_layout(theme_config)
    layout.update(
        {
            "title": _title_block(title, theme_config["colors"]["text"], title_size),
            "margin": {"l": 60, "r": 30, "t": 70, "b": 50},
        },
    )

    if height is not None:
        layout["height"] = height
    fig.update_layout(**layout)
    _style_axes(fig, theme_config)


def _add_paper_note(fig: go.Figure, theme_config: dict[str, Any], message: str) -> None:
    """
    Add a centered paper-space note, used for empty standalone figures.
    """
    fig.add_annotation(
        x=0.5,
        y=0.5,
        xref="paper",
        yref="paper",
        text=message,
        showarrow=False,
        font={"family": _FONT_MONO, "size": 12, "color": theme_config["colors"]["muted"]},
    )


def _empty_figure(
    theme_config: dict[str, Any],
    title: str,
    message: str = _EMPTY_DATA_MESSAGE,
    height: int = 400,
) -> go.Figure:
    """
    Build a styled standalone figure stating that no data is available.
    """
    fig = go.Figure()
    _style_figure(fig, theme_config, title, height=height)
    fig.update_xaxes(visible=False)
    fig.update_yaxes(visible=False)
    _add_paper_note(fig, theme_config, message)
    return fig


def _render_empty_cell(
    fig: go.Figure,
    row: int,
    col: int,
    theme_config: dict[str, Any],
    message: str = _EMPTY_DATA_MESSAGE,
) -> None:
    """
    Render a centered note into an empty cartesian subplot cell.
    """
    fig.add_trace(
        go.Scatter(
            x=[0.5],
            y=[0.5],
            text=[message],
            mode="text",
            textfont={
                "family": _FONT_MONO,
                "size": 11,
                "color": theme_config["colors"]["muted"],
            },
            hoverinfo="skip",
            showlegend=False,
        ),
        row=row,
        col=col,
    )
    fig.update_xaxes(
        showticklabels=False,
        showgrid=False,
        zeroline=False,
        showline=False,
        row=row,
        col=col,
    )
    fig.update_yaxes(
        showticklabels=False,
        showgrid=False,
        zeroline=False,
        showline=False,
        row=row,
        col=col,
    )


def _build_table(
    metrics: list[str],
    values: list[str],
    fill_colors: list[str],
    theme_config: dict[str, Any],
) -> go.Table:
    """
    Build a styled two-column table with right-aligned values.
    """
    colors = theme_config["colors"]

    # Row heights stay automatic so long statistic lists always fit their cell.
    return go.Table(
        columnwidth=[0.50, 0.50],
        header={
            "values": ["<b>METRIC</b>", "<b>VALUE</b>"],
            "fill_color": colors["table_header"],
            "font": {"family": _FONT_MONO, "color": colors["table_text"], "size": 11},
            "align": ["left", "right"],
            "line": {"color": colors["grid"], "width": 1},
        },
        cells={
            "values": [metrics, values],
            "fill_color": [fill_colors, fill_colors],
            "align": ["left", "right"],
            "font": {"family": _FONT_MONO, "size": 11, "color": colors["table_text"]},
            "line": {"color": colors["grid"], "width": 1},
        },
    )


def _calculate_drawdown(returns: pd.Series) -> pd.Series:
    """
    Calculate drawdown series from returns.

    Parameters
    ----------
    returns : pd.Series
        Returns series with datetime index.

    Returns
    -------
    pd.Series
        Drawdown series as percentage (negative values).

    """
    if returns.empty:
        return pd.Series(dtype=float)

    cumulative = (1 + returns).cumprod()

    # Prepend baseline 1.0 at time before first return to establish starting equity
    baseline_time = returns.index.min() - pd.Timedelta(seconds=1)
    baseline = pd.Series([1.0], index=[baseline_time])
    cumulative = pd.concat([baseline, cumulative])

    running_max = cumulative.cummax()

    return (cumulative - running_max) / running_max * 100


def register_chart(name: str, func: Callable | None = None) -> Callable | None:
    """
    Register a custom chart function for standalone use.

    Registered charts are retrievable via ``get_chart`` and ``list_charts``.
    Placing a custom chart in a tearsheet grid uses the separate
    ``register_tearsheet_chart`` path (see the visualization guide); a name
    registered here is not rendered by ``TearsheetCustomChart``.

    Can be used as a decorator or called directly.

    Parameters
    ----------
    name : str
        The chart name for later lookup via ``get_chart`` / ``list_charts``.
    func : Callable, optional
        Chart function that returns a plotly Figure. Should accept
        (returns: pd.Series, **kwargs) as parameters. If None, returns
        a decorator.

    Returns
    -------
    Callable or None
        The decorated function if used as a decorator, otherwise None.

    Raises
    ------
    ValueError
        If name is empty or func is not callable.

    Examples
    --------
    >>> # As a decorator
    >>> @register_chart("my_custom_chart")
    ... def create_custom_chart(returns: pd.Series, **kwargs) -> go.Figure:
    ...     fig = go.Figure()
    ...     # ... custom visualization logic
    ...     return fig
    >>>
    >>> # Or called directly
    >>> register_chart("another_chart", create_custom_chart)

    """
    _require_not_none(name, "name")

    if not name.strip():
        raise ValueError("Chart name cannot be empty")

    if func is None:

        def decorator(f: Callable) -> Callable:
            if not callable(f):
                raise ValueError(f"Chart function must be callable, was {type(f)}")
            _CHART_REGISTRY[name] = f
            return f

        return decorator

    if not callable(func):
        raise ValueError(f"Chart function must be callable, was {type(func)}")

    _CHART_REGISTRY[name] = func
    return None


def get_chart(name: str) -> Callable:
    """
    Get registered chart function by name.

    Parameters
    ----------
    name : str
        The chart name.

    Returns
    -------
    Callable
        The chart function.

    Raises
    ------
    KeyError
        If the chart name is not registered.

    """
    _require_not_none(name, "name")

    if name not in _CHART_REGISTRY:
        available = ", ".join(_CHART_REGISTRY.keys())

        suggestions = get_close_matches(name, _CHART_REGISTRY.keys(), n=3, cutoff=0.6)
        suggestion_text = f" Did you mean: {', '.join(suggestions)}?" if suggestions else ""

        raise KeyError(
            f"Chart '{name}' not found.{suggestion_text} "
            f"Available charts: {available}. "
            f"Register custom charts with register_chart().",
        )

    return _CHART_REGISTRY[name]


def list_charts() -> list[str]:
    """
    List all registered chart names.

    Returns
    -------
    list[str]
        List of available chart names.

    """
    return list(_CHART_REGISTRY.keys())


def create_tearsheet(
    engine: BacktestEngine | BacktestResult,
    output_path: str | None = "tearsheet.html",
    title: str = "NautilusTrader Backtest Results",
    currency: Currency | None = None,
    config: TearsheetConfig | None = None,
    benchmark_returns: pd.Series | None = None,
    benchmark_name: str = "Benchmark",
    node: BacktestNode | None = None,
    run_config_id: str | None = None,
) -> str | None:
    """
    Generate an interactive HTML tearsheet from backtest results.

    Parameters
    ----------
    engine : BacktestEngine or BacktestResult
        The completed backtest engine or result.
    output_path : str, optional
        Path to save the tearsheet. File extension selects the format:
        ``.html`` (interactive), or ``.png``, ``.jpg``, ``.webp``, ``.svg``, ``.pdf``
        (static, via Kaleido). If None, returns HTML string.
    title : str, default "NautilusTrader Backtest Results"
        Title for the tearsheet.
    currency : Currency, optional
        Currency filter for PnL statistics, account balances, and engine-derived returns.
        For ``BacktestResult`` input, the stored return series remains unchanged. If
        None, includes all available currencies.
    config : TearsheetConfig, optional
        Configuration for tearsheet customization. If None, uses default configuration.
    benchmark_returns : pd.Series, optional
        Benchmark returns series for comparison. If provided, benchmark will be overlaid
        on visualizations.
    benchmark_name : str, default "Benchmark"
        Display name for the benchmark.
    node : BacktestNode, optional
        The node which produced a ``BacktestResult``. Provide it for starting balances
        or charts that read cached data, such as ``bars_with_fills``. The matching run
        configuration must set ``dispose_on_completion=False``.
    run_config_id : str, optional
        The run configuration ID. Defaults to ``engine.run_config_id``.

    Returns
    -------
    str or None
        HTML string if output_path is None, otherwise None.

    Raises
    ------
    ImportError
        If plotly is not installed.
    ValueError
        If ``bars_with_fills`` is configured without a node, a supplied node has no
        run configuration ID, or the matching run configuration sets
        ``dispose_on_completion=True``.

    """
    if not PLOTLY_AVAILABLE:
        raise ImportError(
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]",
        )

    _require_not_none(engine, "engine")

    if not hasattr(engine, "get_result"):
        return _create_tearsheet_from_result(
            result=engine,
            node=node,
            run_config_id=run_config_id,
            currency=currency,
            output_path=output_path,
            title=title,
            config=config,
            benchmark_returns=benchmark_returns,
            benchmark_name=benchmark_name,
        )

    result = engine.get_result()
    stats_returns = dict(result.stats_returns)
    stats_general = dict(result.stats_general)
    stats_pnls = dict(result.stats_pnls)
    returns = _resolve_tearsheet_returns(engine=engine, currency=currency)

    stats_pnls = _filter_stats_pnls(stats_pnls, currency)

    if title == "NautilusTrader Backtest Results":
        strategies = engine.cache.strategy_ids()
        strategy_names = ", ".join(str(s) for s in strategies) if strategies else "None"
        run_started = _format_optional_iso8601(engine.run_started)

        title = f"<b>NautilusTrader</b> v{NAUTILUS_VERSION} - Backtest Results"
        title += f"<br><sub>Strategies: {strategy_names} | Run started: {run_started}</sub>"

    run_info = {
        "Run ID": str(engine.run_id),
        "Run started": _format_optional_iso8601(engine.run_started),
        "Run finished": _format_optional_iso8601(engine.run_finished),
        "Elapsed time": _format_optional_duration(engine.run_started, engine.run_finished),
        "Backtest start": _format_optional_iso8601(engine.backtest_start),
        "Backtest end": _format_optional_iso8601(engine.backtest_end),
        "Backtest range": _format_optional_duration(engine.backtest_start, engine.backtest_end),
        "Iterations": f"{result.iterations:,}",
        "Total events": f"{result.total_events:,}",
        "Total orders": f"{result.total_orders:,}",
        "Total positions": f"{result.total_positions:,}",
    }

    return create_tearsheet_from_stats(
        run_info=run_info,
        account_info=_collect_account_info(engine=engine, currency=currency),
        stats_pnls=stats_pnls,
        stats_returns=stats_returns,
        stats_general=stats_general,
        returns=returns,
        output_path=output_path,
        title=title,
        config=config,
        benchmark_returns=benchmark_returns,
        benchmark_name=benchmark_name,
        engine=engine,
    )


class _BacktestNodeEngineView:
    def __init__(self, node: BacktestNode, run_config_id: str) -> None:
        self._node = node
        self._run_config_id = run_config_id
        self.cache = node.get_engine_cache(run_config_id)

    def generate_fills_report(self) -> pd.DataFrame:
        return self._node.generate_fills_report(self._run_config_id)


def _create_tearsheet_from_result(
    result: BacktestResult,
    node: BacktestNode | None,
    run_config_id: str | None,
    currency: Currency | None,
    output_path: str | None,
    title: str,
    config: TearsheetConfig | None,
    benchmark_returns: pd.Series | None,
    benchmark_name: str,
) -> str | None:
    resolved_run_config_id = run_config_id or result.run_config_id
    needs_engine = config is not None and "bars_with_fills" in config.chart_names
    if needs_engine and node is None:
        raise ValueError("A BacktestNode is required for the bars_with_fills chart")
    if node is not None and resolved_run_config_id is None:
        raise ValueError("run_config_id is required when a BacktestNode is provided")
    if node is not None and resolved_run_config_id is not None:
        _validate_result_node_state(node, resolved_run_config_id)

    engine_view = (
        _BacktestNodeEngineView(node, resolved_run_config_id)
        if node is not None and resolved_run_config_id is not None
        else None
    )
    returns = _result_returns_series(result)
    run_info = _result_run_info(result)
    account_info = _result_account_info(result, node, resolved_run_config_id, currency)

    if title == "NautilusTrader Backtest Results":
        run_started = _format_optional_iso8601(result.run_started)
        title = f"<b>NautilusTrader</b> v{NAUTILUS_VERSION} - Backtest Results"
        title += f"<br><sub>Run started: {run_started}</sub>"

    return create_tearsheet_from_stats(
        run_info=run_info,
        account_info=account_info,
        stats_pnls=_filter_stats_pnls(result.stats_pnls, currency),
        stats_returns=result.stats_returns,
        stats_general=result.stats_general,
        returns=returns,
        output_path=output_path,
        title=title,
        config=config,
        benchmark_returns=benchmark_returns,
        benchmark_name=benchmark_name,
        engine=engine_view,
    )


def _validate_result_node_state(node: BacktestNode, run_config_id: str) -> None:
    for run_config in node.configs:
        if run_config.id != run_config_id:
            continue
        if run_config.dispose_on_completion:
            raise ValueError(
                "BacktestNode state is unavailable when dispose_on_completion=True; "
                "set dispose_on_completion=False before running the backtest",
            )
        return


def _filter_stats_pnls(
    stats_pnls: Mapping[str, Any],
    currency: Currency | None,
) -> dict[str, Any]:
    stats_pnls = dict(stats_pnls)
    if currency is None:
        return stats_pnls

    currency_code = getattr(currency, "code", str(currency))
    return {currency_code: stats_pnls[currency_code]} if currency_code in stats_pnls else {}


def _result_returns_series(result: BacktestResult) -> pd.Series:
    returns = pd.Series(dict(result.returns_series), dtype="float64")
    returns.index = pd.to_datetime(returns.index, unit="ns", utc=True)
    return returns.sort_index()


def _result_run_info(result: BacktestResult) -> dict[str, str]:
    return {
        "Run ID": str(result.run_id) if result.run_id is not None else "N/A",
        "Run started": _format_optional_iso8601(result.run_started),
        "Run finished": _format_optional_iso8601(result.run_finished),
        "Elapsed time": _format_optional_duration(result.run_started, result.run_finished),
        "Backtest start": _format_optional_iso8601(result.backtest_start),
        "Backtest end": _format_optional_iso8601(result.backtest_end),
        "Backtest range": _format_optional_duration(result.backtest_start, result.backtest_end),
        "Iterations": f"{result.iterations:,}",
        "Total events": f"{result.total_events:,}",
        "Total orders": f"{result.total_orders:,}",
        "Total positions": f"{result.total_positions:,}",
    }


def _result_account_info(
    result: BacktestResult,
    node: BacktestNode | None,
    run_config_id: str | None,
    currency: Currency | None = None,
) -> dict[str, str]:
    currency_code = getattr(currency, "code", str(currency)) if currency is not None else None
    summary = result.summary
    account_ids = {
        key.removeprefix("account.").removesuffix(".id"): value
        for key, value in summary.items()
        if key.startswith("account.") and key.endswith(".id")
    }
    account_info = {}

    if node is not None and run_config_id is not None:
        for venue, account_id in sorted(account_ids.items()):
            report = node.generate_account_report(
                run_config_id,
                account_id=AccountId.from_str(account_id),
            )

            if report.empty:
                continue

            for report_currency, balances in report.groupby("currency", sort=True):
                if currency_code is not None and str(report_currency) != currency_code:
                    continue
                totals = pd.to_numeric(balances["total"], errors="coerce").dropna()
                if totals.empty:
                    continue
                account_info[f"Starting balance ({venue}, {report_currency})"] = str(totals.iloc[0])
                account_info[f"Ending balance ({venue}, {report_currency})"] = str(totals.iloc[-1])

        return account_info

    for key, value in sorted(summary.items()):
        if ".balance." not in key or not key.endswith(".total"):
            continue
        prefix, currency_and_field = key.rsplit(".balance.", maxsplit=1)
        venue = prefix.removeprefix("account.")
        report_currency = currency_and_field.removesuffix(".total")
        if currency_code is not None and report_currency != currency_code:
            continue
        account_info[f"Ending balance ({venue}, {report_currency})"] = value

    return account_info


def _resolve_tearsheet_returns(
    engine: BacktestEngine,
    currency: Currency | None = None,
) -> pd.Series:
    """
    Pick the best available returns series for the tearsheet.

    v2 exposes statistics snapshots rather than the mutable analyzer behind the
    portfolio, so this uses complete, currency-compatible mark-to-market portfolio
    snapshots when available and falls back to account reports otherwise.

    """
    snapshot_returns = _calculate_snapshot_returns(engine=engine, currency=currency)
    if snapshot_returns is not None and not snapshot_returns.empty:
        return snapshot_returns

    account_returns = _calculate_account_returns(engine=engine, currency=currency)
    if account_returns is not None and not account_returns.empty:
        return account_returns

    return pd.Series(dtype=float)


def _calculate_snapshot_returns(
    engine: BacktestEngine,
    currency: Currency | None = None,
) -> pd.Series | None:
    """
    Compute daily returns from complete, currency-compatible portfolio snapshots.

    Returns ``None`` when an account lacks usable snapshots or the account currencies
    cannot be aggregated.

    """
    if engine is None:
        return None

    portfolio = getattr(engine, "portfolio", None)
    cache = getattr(engine, "cache", None)

    if portfolio is None or cache is None:
        return None

    venues = engine.list_venues()

    if not venues:
        return None

    target_currency = getattr(currency, "code", str(currency)) if currency is not None else None
    one_day_ns = pd.Timedelta(days=1).value
    result = _collect_snapshot_equity(
        venues=venues,
        cache=cache,
        portfolio=portfolio,
        target_currency=target_currency,
        one_day_ns=one_day_ns,
    )

    if result is None:
        return None

    equity_by_account, observed_currencies = result

    if target_currency is None and len(observed_currencies) != 1:
        return None

    combined = pd.concat(equity_by_account.values(), axis=1).sort_index().ffill().dropna()

    return None if combined.empty else _calculate_daily_balance_returns(combined.sum(axis=1))


def _collect_snapshot_equity(
    venues: list[Venue],
    cache: Cache,
    portfolio: Portfolio,
    target_currency: str | None,
    one_day_ns: int,
) -> tuple[dict[str, pd.Series], set[str]] | None:
    observed_currencies = set()
    equity_by_account: dict[str, pd.Series] = {}

    for venue in venues:
        account = cache.account_for_venue(venue)
        if account is None:
            return None

        account_id = str(account.id)
        if account_id in equity_by_account:
            continue

        snapshots = portfolio.snapshots(account.id)
        if not snapshots:
            return None

        result = _snapshot_equity_series(
            snapshots=snapshots,
            target_currency=target_currency,
            account_id=account_id,
            one_day_ns=one_day_ns,
        )

        if result is None:
            return None

        account_equity, account_currencies = result
        equity_by_account[account_id] = account_equity
        observed_currencies.update(account_currencies)

    if not equity_by_account:
        return None

    return equity_by_account, observed_currencies


def _snapshot_equity_series(
    snapshots: list[PortfolioSnapshot],
    target_currency: str | None,
    account_id: str,
    one_day_ns: int,
) -> tuple[pd.Series, set[str]] | None:
    values = []
    timestamps = []
    observed_currencies = set()

    for snapshot in snapshots:
        if getattr(snapshot, "unpriced_instruments", []):
            continue

        resolved = _resolve_snapshot_equity(snapshot, target_currency)
        if resolved is None:
            return None

        value, observed_currency = resolved
        values.append(value)
        ts_event = snapshot.ts_event
        if len(values) == 1:
            ts_event = max(ts_event - (ts_event % one_day_ns) - 1, 0)
        elif ts_event > 0 and ts_event % one_day_ns == 0:
            ts_event -= 1
        timestamps.append(ts_event)
        observed_currencies.add(observed_currency)

    if not values:
        return None

    series = pd.Series(
        values,
        index=pd.to_datetime(timestamps, unit="ns", utc=True),
        dtype=float,
        name=account_id,
    )
    return series.sort_index().groupby(level=0).last(), observed_currencies


def _resolve_snapshot_equity(
    snapshot: PortfolioSnapshot,
    target_currency: str | None,
) -> tuple[float, str] | None:
    base_equity = snapshot.base_currency_equity
    total_equity = snapshot.total_equity

    if target_currency is not None:
        if base_equity is not None and base_equity.currency.code == target_currency:
            return base_equity.as_double(), target_currency

        matching = [money for money in total_equity if money.currency.code == target_currency]
        if len(matching) != 1:
            return None
        return matching[0].as_double(), target_currency

    if len(total_equity) != 1:
        return None

    if base_equity is not None:
        return base_equity.as_double(), base_equity.currency.code

    return total_equity[0].as_double(), total_equity[0].currency.code


def _calculate_account_returns(
    engine: BacktestEngine,
    currency: Currency | None = None,
) -> pd.Series | None:
    """
    Compute daily portfolio returns by aggregating v2 account reports.

    Returns ``None`` when accounts use mixed currencies without an explicit ``currency``
    filter.

    """
    if engine is None:
        return None

    venues = engine.list_venues()
    if not venues:
        return None

    target_currency = getattr(currency, "code", str(currency)) if currency is not None else None
    observed_currencies = set()
    balance_series: list[pd.Series] = []

    for venue in venues:
        report = engine.generate_account_report(venue=venue)
        try:
            totals, observed_currency = _extract_account_balance_series(
                report=report,
                target_currency=target_currency,
            )
        except ValueError:
            return None

        if observed_currency is not None:
            observed_currencies.add(observed_currency)

        if totals is None:
            continue

        balance_series.append(totals.rename(str(venue)))

    if not balance_series:
        return None

    if target_currency is None and len(observed_currencies) != 1:
        return None

    combined = pd.concat(balance_series, axis=1).sort_index().ffill().dropna()

    return None if combined.empty else _calculate_daily_balance_returns(combined.sum(axis=1))


def _collect_account_info(
    engine: BacktestEngine,
    currency: Currency | None = None,
) -> dict[str, str]:
    target_currency = getattr(currency, "code", str(currency)) if currency is not None else None
    account_info: dict[str, str] = {}

    for venue in engine.list_venues():
        report = engine.generate_account_report(venue=venue)
        if report.empty or "currency" not in report or "total" not in report:
            continue

        currencies = (
            [target_currency]
            if target_currency is not None
            else sorted(report["currency"].dropna().unique())
        )

        for code in currencies:
            currency_report = report[report["currency"] == code].sort_index()
            if currency_report.empty:
                continue

            account_info[f"Starting balance ({venue} {code})"] = str(
                currency_report["total"].iloc[0],
            )
            account_info[f"Ending balance ({venue} {code})"] = str(
                currency_report["total"].iloc[-1],
            )

    return account_info


def _extract_account_balance_series(
    report: pd.DataFrame,
    target_currency: str | None,
) -> tuple[pd.Series | None, str | None]:
    if report.empty or "currency" not in report or "total" not in report:
        return None, None

    observed_currency: str | None = None

    if target_currency is None:
        account_currencies = set(report["currency"].dropna())
        if len(account_currencies) != 1:
            raise ValueError("account report contains multiple currencies")
        observed_currency = next(iter(account_currencies))
    else:
        report = report[report["currency"] == target_currency]

    if report.empty:
        return None, observed_currency

    totals = pd.to_numeric(report["total"], errors="coerce")
    totals.index = report.index
    totals = totals.dropna().sort_index()
    if totals.empty:
        return None, observed_currency

    return totals.groupby(level=0).last(), observed_currency


def _calculate_daily_balance_returns(total_balance: pd.Series) -> pd.Series | None:
    account_returns = (
        total_balance.resample("D")
        .last()
        .ffill()
        .pct_change()
        .replace(
            [float("inf"), float("-inf")],
            float("nan"),
        )
    ).dropna()

    if account_returns.empty:
        return None

    return account_returns


def _aggregate_period_returns(
    returns: pd.Series,
    freq: str,
    *,
    compounding: bool = True,
) -> pd.Series:
    if compounding:
        return returns.resample(freq).apply(lambda x: (1 + x).prod() - 1) * 100

    # cumprod is row-order dependent, so sort by time before building the equity index
    equity = (1 + returns.sort_index()).cumprod()
    period_end = equity.resample(freq).last().ffill()
    period_return = period_end.diff()

    # First period has no prior balance; measure it against the initial unit base
    period_return.iloc[0] = period_end.iloc[0] - 1.0

    return period_return * 100


def create_tearsheet_from_stats(
    stats_pnls: dict[str, Any] | dict[str, dict[str, Any]],
    stats_returns: dict[str, Any],
    stats_general: dict[str, Any],
    returns: pd.Series,
    output_path: str | None = "tearsheet.html",
    title: str = "NautilusTrader Backtest Results",
    config: TearsheetConfig | None = None,
    benchmark_returns: pd.Series | None = None,
    benchmark_name: str = "Benchmark",
    run_info: dict[str, Any] | None = None,
    account_info: dict[str, Any] | None = None,
    engine: BacktestEngine | _BacktestNodeEngineView | None = None,
) -> str | None:
    """
    Generate an interactive HTML tearsheet from precomputed statistics.

    This lower-level API is useful for offline analysis when you have
    precomputed statistics and don't want to pass an engine.

    Parameters
    ----------
    stats_pnls : dict[str, Any]
        PnL-based statistics.
    stats_returns : dict[str, Any]
        Returns-based statistics.
    stats_general : dict[str, Any]
        General statistics.
    returns : pd.Series
        Returns series.
    output_path : str, optional
        Path to save the tearsheet. File extension selects the format:
        ``.html`` (interactive), or ``.png``, ``.jpg``, ``.webp``, ``.svg``, ``.pdf``
        (static, via Kaleido). If None, returns HTML string.
    title : str, default "NautilusTrader Backtest Results"
        Title for the tearsheet.
    config : TearsheetConfig, optional
        Configuration for tearsheet customization. If None, uses default configuration.
    benchmark_returns : pd.Series, optional
        Benchmark returns series for comparison. If provided, benchmark will be overlaid
        on visualizations.
    benchmark_name : str, default "Benchmark"
        Display name for the benchmark.
    run_info : dict[str, Any], optional
        Run metadata (run ID, timestamps, backtest period, event counts).
    account_info : dict[str, Any], optional
        Account information (starting/ending balances per currency).
    engine : BacktestEngine, optional
        The backtest engine. Required for charts that need engine access (e.g., bars_with_fills).

    Returns
    -------
    str or None
        HTML string if output_path is None, otherwise None.

    Raises
    ------
    ImportError
        If plotly is not installed.

    Examples
    --------
    >>> # Offline analysis with precomputed stats
    >>> stats_returns = {"Sharpe Ratio (252 days)": 1.5}
    >>> stats_general = {"Win Rate": 0.55}
    >>> stats_pnls = {"PnL (total)": 10000.0}
    >>> returns = pd.Series([0.01, -0.02])
    >>> html = create_tearsheet_from_stats(  # doctest: +SKIP
    ...     stats_pnls,
    ...     stats_returns,
    ...     stats_general,
    ...     returns,
    ...     output_path=None,  # Return HTML instead of saving
    ... )

    """
    if not PLOTLY_AVAILABLE:
        raise ImportError(
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]",
        )

    returns = _to_returns_series(returns)
    benchmark_returns = (
        _to_returns_series(benchmark_returns) if benchmark_returns is not None else None
    )

    if config is None:
        config = TearsheetConfig()

    # Filter out run_info chart if no metadata is available.
    # This prevents an empty subplot from wasting grid space.
    if not run_info and not account_info and "run_info" in config.chart_names:
        config = TearsheetConfig(
            charts=[c for c in config.charts if c.name != "run_info"],
            theme=config.theme,
            layout=config.layout,
            title=config.title,
            include_benchmark=config.include_benchmark,
            benchmark_name=config.benchmark_name,
            height=config.height,
            show_logo=config.show_logo,
        )

    fig = _create_tearsheet_figure(
        stats_returns=stats_returns,
        stats_general=stats_general,
        stats_pnls=stats_pnls,
        returns=returns,
        title=title,
        config=config,
        benchmark_returns=benchmark_returns,
        benchmark_name=benchmark_name,
        run_info=run_info or {},
        account_info=account_info or {},
        engine=engine,
    )

    if output_path:
        _write_figure(fig, output_path)
        return None
    return fig.to_html()


def create_equity_curve(
    returns: pd.Series,
    output_path: str | None = None,
    title: str = "Equity Curve",
    benchmark_returns: pd.Series | None = None,
    benchmark_name: str = "Benchmark",
    theme: str = "plotly_white",
) -> go.Figure:
    """
    Create an interactive equity curve plot with optional benchmark overlay.

    Parameters
    ----------
    returns : pd.Series
        Returns series.
    output_path : str, optional
        Path to save HTML plot. If None, plot is not saved.
    title : str, default "Equity Curve"
        Plot title.
    benchmark_returns : pd.Series, optional
        Benchmark returns series for comparison. If provided, benchmark equity
        curve will be overlaid on the chart.
    benchmark_name : str, default "Benchmark"
        Display name for the benchmark in the legend.
    theme : str, default "plotly_white"
        Theme name for styling.

    Returns
    -------
    go.Figure
        Plotly figure object.

    Raises
    ------
    ImportError
        If plotly is not installed.

    """
    if not PLOTLY_AVAILABLE:
        raise ImportError(
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]",
        )

    returns = _to_returns_series(returns)
    benchmark_returns = (
        _to_returns_series(benchmark_returns) if benchmark_returns is not None else None
    )
    theme_config = _normalize_theme_config(get_theme(theme))
    colors = theme_config["colors"]

    benchmark = (
        benchmark_returns if benchmark_returns is not None and not benchmark_returns.empty else None
    )

    if returns.empty and benchmark is None:
        fig = _empty_figure(theme_config, title, height=500)
        if output_path:
            _write_figure(fig, output_path)
        return fig

    fig = go.Figure()
    bounds: list[tuple[float, float]] = []

    if not returns.empty:
        equity = (1 + returns).cumprod()
        fig.add_trace(
            go.Scatter(
                x=equity.index,
                y=equity.values,
                mode="lines",
                name="Strategy",
                line={"color": colors["primary"], "width": 2},
                fill="tozeroy",
                fillcolor=_hex_to_rgba(colors["primary"], 0.07),
                hovertemplate="<b>%{x}</b><br>Strategy: %{y:.4f}<extra></extra>",
            ),
        )
        bounds.append((float(equity.min()), float(equity.max())))

    if benchmark is not None:
        benchmark_equity = (1 + benchmark).cumprod()
        fig.add_trace(
            go.Scatter(
                x=benchmark_equity.index,
                y=benchmark_equity.values,
                mode="lines",
                name=benchmark_name,
                line={"color": colors["neutral"], "width": 1.5, "dash": "dash"},
                hovertemplate=f"<b>%{{x}}</b><br>{benchmark_name}: %{{y:.4f}}<extra></extra>",
            ),
        )
        bounds.append((float(benchmark_equity.min()), float(benchmark_equity.max())))

    ymin = min(lo for lo, _ in bounds)
    ymax = max(hi for _, hi in bounds)
    fig.update_yaxes(range=_padded_range(ymin, ymax))

    _style_figure(fig, theme_config, title, height=500)
    fig.update_layout(hovermode="x unified", showlegend=benchmark is not None)
    fig.update_xaxes(title_text="Date")
    fig.update_yaxes(title_text="Equity")

    if output_path:
        _write_figure(fig, output_path)

    return fig


def create_drawdown_chart(
    returns: pd.Series,
    output_path: str | None = None,
    title: str = "Drawdown",
    theme: str = "plotly_white",
) -> go.Figure:
    """
    Create an interactive drawdown chart.

    Parameters
    ----------
    returns : pd.Series
        Returns series.
    output_path : str, optional
        Path to save HTML plot. If None, plot is not saved.
    title : str, default "Drawdown"
        Plot title.
    theme : str, default "plotly_white"
        Theme name for styling.

    Returns
    -------
    go.Figure
        Plotly figure object.

    Raises
    ------
    ImportError
        If plotly is not installed.

    """
    if not PLOTLY_AVAILABLE:
        raise ImportError(
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]",
        )

    returns = _to_returns_series(returns)
    theme_config = _normalize_theme_config(get_theme(theme))
    colors = theme_config["colors"]

    if returns.empty:
        fig = _empty_figure(theme_config, title)
        if output_path:
            _write_figure(fig, output_path)
        return fig

    drawdown = _calculate_drawdown(returns)
    neg_color = colors["negative"]
    fig = go.Figure()
    fig.add_trace(
        go.Scatter(
            x=drawdown.index,
            y=drawdown.values,
            mode="lines",
            name="Drawdown",
            fill="tozeroy",
            line={"color": neg_color, "width": 1.5},
            fillcolor=_hex_to_rgba(neg_color, 0.18),
            hovertemplate="<b>%{x}</b><br>Drawdown: %{y:.2f}%<extra></extra>",
        ),
    )

    _style_figure(fig, theme_config, title, height=400)
    fig.update_layout(hovermode="x unified", showlegend=False)
    fig.update_xaxes(title_text="Date")
    fig.update_yaxes(title_text="Drawdown (%)")

    if output_path:
        _write_figure(fig, output_path)

    return fig


def create_monthly_returns_heatmap(
    returns: pd.Series,
    output_path: str | None = None,
    title: str | None = None,
    *,
    compounding: bool = True,
    theme: str = "plotly_white",
) -> go.Figure:
    """
    Create an interactive monthly returns heatmap.

    Parameters
    ----------
    returns : pd.Series
        Returns series.
    output_path : str, optional
        Path to save HTML plot. If None, plot is not saved.
    title : str, optional
        Plot title. Defaults to a basis-aware title derived from `compounding`.
    compounding : bool, default True
        If True, cells compound against the running start-of-month balance. If False,
        cells are simple returns on fixed initial capital that sum to the total return
        (the nominal rate of return).
    theme : str, default "plotly_white"
        Theme name for styling.

    Returns
    -------
    go.Figure
        Plotly figure object.

    Raises
    ------
    ImportError
        If plotly is not installed.

    """
    if not PLOTLY_AVAILABLE:
        raise ImportError(
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]",
        )

    if title is None:
        title = "Monthly Returns (%)" if compounding else "Monthly Returns (% of initial capital)"

    returns = _to_returns_series(returns)
    theme_config = _normalize_theme_config(get_theme(theme))

    if returns.empty:
        fig = _empty_figure(theme_config, title)
        if output_path:
            _write_figure(fig, output_path)
        return fig

    heatmap_data = _monthly_pivot(returns, compounding=compounding)
    fig = go.Figure(data=_heatmap_trace(heatmap_data, theme_config, showscale=True))

    _style_figure(fig, theme_config, title, height=400)
    fig.update_xaxes(title_text="Month")
    fig.update_yaxes(title_text="Year")

    if output_path:
        _write_figure(fig, output_path)

    return fig


def create_returns_distribution(
    returns: pd.Series,
    output_path: str | None = None,
    title: str = "Returns Distribution",
    theme: str = "plotly_white",
) -> go.Figure:
    """
    Create an interactive returns distribution histogram.

    Parameters
    ----------
    returns : pd.Series
        Returns series.
    output_path : str, optional
        Path to save HTML plot. If None, plot is not saved.
    title : str, default "Returns Distribution"
        Plot title.
    theme : str, default "plotly_white"
        Theme name for styling.

    Returns
    -------
    go.Figure
        Plotly figure object.

    Raises
    ------
    ImportError
        If plotly is not installed.

    """
    if not PLOTLY_AVAILABLE:
        raise ImportError(
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]",
        )

    returns = _to_returns_series(returns)
    theme_config = _normalize_theme_config(get_theme(theme))
    colors = theme_config["colors"]

    if returns.empty:
        fig = _empty_figure(theme_config, title)
        if output_path:
            _write_figure(fig, output_path)
        return fig

    fig = go.Figure()
    fig.add_trace(
        go.Histogram(
            x=returns.to_numpy() * 100,
            nbinsx=50,
            name="Returns",
            marker={
                "color": colors["neutral"],
                "line": {"color": colors["background"], "width": 0.5},
            },
            hovertemplate="Return: %{x:.2f}%<br>Count: %{y}<extra></extra>",
        ),
    )
    fig.add_vline(
        x=float(returns.mean()) * 100,
        line={"color": colors["primary"], "dash": "dash", "width": 1.5},
        annotation_text="mean",
        annotation_position="top right",
        annotation_font={"family": _FONT_MONO, "size": 10, "color": colors["muted"]},
    )

    _style_figure(fig, theme_config, title, height=400)
    fig.update_layout(bargap=0.1, showlegend=False)
    fig.update_xaxes(title_text="Return (%)")
    fig.update_yaxes(title_text="Frequency")

    if output_path:
        _write_figure(fig, output_path)

    return fig


def _rolling_sharpe_series(returns: pd.Series, window: int) -> pd.Series:
    """
    Compute the annualized rolling Sharpe ratio series.
    """
    rolling_mean = returns.rolling(window=window).mean()
    rolling_std = returns.rolling(window=window).std()
    return (rolling_mean / rolling_std.replace(0, float("nan"))) * (TRADING_DAYS_PER_YEAR**0.5)


def create_rolling_sharpe(
    returns: pd.Series,
    window: int = 60,
    output_path: str | None = None,
    title: str = "Rolling Sharpe Ratio (60-day)",
    theme: str = "plotly_white",
) -> go.Figure:
    """
    Create an interactive rolling Sharpe ratio chart.

    Parameters
    ----------
    returns : pd.Series
        Returns series.
    window : int, default 60
        Rolling window size in days.
    output_path : str, optional
        Path to save HTML plot. If None, plot is not saved.
    title : str, default "Rolling Sharpe Ratio (60-day)"
        Plot title.
    theme : str, default "plotly_white"
        Theme name for styling.

    Returns
    -------
    go.Figure
        Plotly figure object.

    Raises
    ------
    ImportError
        If plotly is not installed.

    """
    if not PLOTLY_AVAILABLE:
        raise ImportError(
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]",
        )

    returns = _to_returns_series(returns)
    theme_config = _normalize_theme_config(get_theme(theme))
    colors = theme_config["colors"]

    if returns.empty or len(returns) < window:
        fig = _empty_figure(
            theme_config,
            title,
            message=f"Not enough data (need {window} observations)",
        )

        if output_path:
            _write_figure(fig, output_path)
        return fig

    rolling_sharpe = _rolling_sharpe_series(returns, window)

    fig = go.Figure()
    fig.add_trace(
        go.Scatter(
            x=rolling_sharpe.index,
            y=rolling_sharpe.values,
            mode="lines",
            name=f"Rolling Sharpe ({window}d)",
            line={"color": colors["primary"], "width": 2},
            hovertemplate="<b>%{x}</b><br>Sharpe: %{y:.3f}<extra></extra>",
        ),
    )

    fig.add_hline(y=0, line={"color": colors["neutral"], "dash": "dash", "width": 1})

    _style_figure(fig, theme_config, title, height=400)
    fig.update_layout(hovermode="x unified", showlegend=False)
    fig.update_xaxes(title_text="Date")
    fig.update_yaxes(title_text="Sharpe Ratio")

    if output_path:
        _write_figure(fig, output_path)

    return fig


def create_yearly_returns(
    returns: pd.Series,
    output_path: str | None = None,
    title: str | None = None,
    *,
    compounding: bool = True,
    theme: str = "plotly_white",
) -> go.Figure:
    """
    Create an interactive yearly returns bar chart.

    Parameters
    ----------
    returns : pd.Series
        Returns series.
    output_path : str, optional
        Path to save HTML plot. If None, plot is not saved.
    title : str, optional
        Plot title. Defaults to a basis-aware title derived from `compounding`.
    compounding : bool, default True
        If True, bars compound against the running start-of-year balance. If False,
        bars are simple returns on fixed initial capital that sum to the total return
        (the nominal rate of return).
    theme : str, default "plotly_white"
        Theme name for styling.

    Returns
    -------
    go.Figure
        Plotly figure object.

    Raises
    ------
    ImportError
        If plotly is not installed.

    """
    if not PLOTLY_AVAILABLE:
        raise ImportError(
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]",
        )

    if title is None:
        title = "Yearly Returns" if compounding else "Yearly Returns (% of initial capital)"

    returns = _to_returns_series(returns)
    theme_config = _normalize_theme_config(get_theme(theme))
    colors = theme_config["colors"]

    if returns.empty:
        fig = _empty_figure(theme_config, title)
        if output_path:
            _write_figure(fig, output_path)
        return fig

    yearly = _aggregate_period_returns(returns, "YE", compounding=compounding)
    bar_colors = [colors["positive"] if r >= 0 else colors["negative"] for r in yearly.to_numpy()]

    fig = go.Figure()
    fig.add_trace(
        go.Bar(
            x=yearly.index.year,
            y=yearly.values,
            marker={
                "color": bar_colors,
                "line": {"color": colors["background"], "width": 1},
            },
            texttemplate="%{y:,.1f}%",
            textposition="outside",
            textfont={"family": _FONT_MONO, "size": 10, "color": colors["text"]},
            cliponaxis=False,
            hovertemplate="<b>%{x}</b><br>Return: %{y:.2f}%<extra></extra>",
        ),
    )

    fig.add_hline(y=0, line={"color": colors["neutral"], "width": 1})

    _style_figure(fig, theme_config, title, height=400)
    fig.update_layout(bargap=0.35, showlegend=False)
    fig.update_xaxes(title_text="Year", type="category")
    fig.update_yaxes(title_text="Return (%)")

    if output_path:
        _write_figure(fig, output_path)

    return fig


def _create_tearsheet_figure(
    stats_returns: dict[str, Any],
    stats_general: dict[str, Any],
    stats_pnls: dict[str, Any] | dict[str, dict[str, Any]],
    returns: pd.Series,
    title: str,
    config: TearsheetConfig | None = None,
    benchmark_returns: pd.Series | None = None,
    benchmark_name: str = "Benchmark",
    run_info: dict[str, Any] | None = None,
    account_info: dict[str, Any] | None = None,
    engine: BacktestEngine | _BacktestNodeEngineView | None = None,
) -> go.Figure:
    """
    Create the complete tearsheet figure with subplots using dynamic chart registry.

    Parameters
    ----------
    stats_returns : dict[str, Any]
        Returns-based statistics.
    stats_general : dict[str, Any]
        General statistics.
    stats_pnls : dict[str, Any]
        PnL-based statistics.
    returns : pd.Series
        Returns series.
    title : str
        Title for the tearsheet.
    config : TearsheetConfig, optional
        Configuration for tearsheet customization.
    benchmark_returns : pd.Series, optional
        Benchmark returns for comparison.
    benchmark_name : str, default "Benchmark"
        Display name for benchmark. If not provided, uses config.benchmark_name.
    run_info : dict[str, Any], optional
        Run metadata (run ID, timestamps, backtest period, event counts).
    account_info : dict[str, Any], optional
        Account information (starting/ending balances per currency).
    engine : BacktestEngine, optional
        The backtest engine. Required for charts that need engine access (e.g., bars_with_fills).

    Returns
    -------
    go.Figure
        Complete tearsheet figure.

    Raises
    ------
    ImportError
        If plotly is not installed.

    """
    if not PLOTLY_AVAILABLE:
        raise ImportError(
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]",
        )

    if config is None:
        config = TearsheetConfig()

    returns = _to_returns_series(returns)
    benchmark_returns = (
        _to_returns_series(benchmark_returns) if benchmark_returns is not None else None
    )

    theme_config = _normalize_theme_config(get_theme(config.theme))

    if not config.include_benchmark and benchmark_returns is not None:
        benchmark_returns = None

    if benchmark_name == "Benchmark":
        benchmark_name = config.benchmark_name

    rows, cols, specs, subplot_titles, heights, v_spacing, h_spacing = _calculate_grid_layout(
        config.charts,
        config.layout,
    )

    fig = make_subplots(
        rows=rows,
        cols=cols,
        subplot_titles=subplot_titles,
        specs=specs,
        vertical_spacing=v_spacing,
        horizontal_spacing=h_spacing,
        row_heights=heights,
    )

    _apply_tearsheet_chrome(
        fig,
        theme_config,
        title,
        config,
        show_legend=benchmark_returns is not None,
    )

    # Style subplot titles before renderers run so annotations added by
    # custom charts keep their own fonts.
    fig.update_annotations(
        font={"family": _FONT_HEADING, "size": 13, "color": theme_config["colors"]["text"]},
    )

    chart_idx = 0

    for row in range(1, rows + 1):
        for col in range(1, cols + 1):
            if chart_idx >= len(config.charts):
                break

            chart = config.charts[chart_idx]
            chart_idx += 1

            chart_name = chart.name

            if chart_name not in _TEARSHEET_CHART_SPECS:
                available = ", ".join(sorted(_TEARSHEET_CHART_SPECS))
                suggestions = get_close_matches(chart_name, _TEARSHEET_CHART_SPECS, n=3, cutoff=0.6)
                hint = f" Did you mean: {', '.join(suggestions)}?" if suggestions else ""
                raise KeyError(
                    f"No tearsheet chart registered under '{chart_name}'.{hint} "
                    f"Register one with `register_tearsheet_chart`. Available: {available}",
                )

            renderer = _TEARSHEET_CHART_SPECS[chart_name]["renderer"]

            chart_kwargs = chart.kwargs()

            renderer(
                fig=fig,
                row=row,
                col=col,
                returns=returns,
                stats_pnls=stats_pnls,
                stats_returns=stats_returns,
                stats_general=stats_general,
                theme_config=theme_config,
                benchmark_returns=benchmark_returns,
                benchmark_name=benchmark_name,
                run_info=run_info or {},
                account_info=account_info or {},
                engine=engine,
                **chart_kwargs,
            )

    if config.show_logo:
        fig.add_annotation(
            x=1.0,
            y=0.004,
            xref="paper",
            yref="paper",
            text=f"NautilusTrader v{NAUTILUS_VERSION}",
            showarrow=False,
            xanchor="right",
            yanchor="bottom",
            font={
                "family": _FONT_MONO,
                "size": 9,
                "color": theme_config["colors"]["muted"],
            },
        )

    return fig


def _apply_tearsheet_chrome(
    fig: go.Figure,
    theme_config: dict[str, Any],
    title: str,
    config: TearsheetConfig,
    *,
    show_legend: bool,
) -> None:
    """
    Apply title, legend, axes, and margin styling before renderers draw traces.
    """
    colors = theme_config["colors"]
    title_text = config.title if config.title != "NautilusTrader Backtest Results" else title
    layout = _base_layout(theme_config)
    layout.update(
        {
            "title": _title_block(title_text, colors["text"], 18),
            "height": config.height,
            "showlegend": show_legend,
            "hovermode": "closest",
            "bargap": 0.35,
            "margin": {"t": 180, "b": 60, "l": 60, "r": 40},
        },
    )
    fig.update_layout(**layout)
    _style_axes(fig, theme_config)


def _create_stats_table(  # noqa: C901
    stats_pnls: dict[str, Any] | dict[str, dict[str, Any]],
    stats_returns: dict[str, Any],
    stats_general: dict[str, Any],
    theme_config: dict[str, Any] | None = None,
    run_info: dict[str, Any] | None = None,
    account_info: dict[str, Any] | None = None,
) -> go.Table:
    """
    Create performance statistics table with section headers.

    Parameters
    ----------
    stats_pnls : dict[str, Any]
        PnL-based statistics.
    stats_returns : dict[str, Any]
        Returns-based statistics.
    stats_general : dict[str, Any]
        General statistics.
    theme_config : dict[str, Any], optional
        Theme configuration for styling.
    run_info : dict[str, Any], optional
        Run metadata (run ID, timestamps, backtest period, event counts).
    account_info : dict[str, Any], optional
        Account information (starting/ending balances per currency).

    Returns
    -------
    go.Table
        Plotly table object with section headers.

    """
    if theme_config is None:
        theme_config = _normalize_theme_config(get_theme("plotly_white"))
    metrics = []
    values = []
    fill_colors = []

    def add_section(title: str, section_stats: dict[str, Any]) -> None:
        if not section_stats:
            return

        metrics.append(f"<b>{title}</b>")
        values.append("")
        fill_colors.append(theme_config["colors"]["table_section"])

        for metric, value in section_stats.items():
            metrics.append(metric)

            if isinstance(value, numbers.Real) and not isinstance(value, bool):
                formatted_value = "N/A" if math.isnan(value) else f"{value:,.4f}"
            elif isinstance(value, str):
                formatted_value = _group_thousands(value)
            else:
                formatted_value = str(value)
            values.append(formatted_value)
            if len(fill_colors) % 2 == 0:
                fill_colors.append(theme_config["colors"]["table_row_odd"])
            else:
                fill_colors.append(theme_config["colors"]["table_row_even"])

    if run_info:
        add_section("Run Information", run_info)

    if account_info:
        add_section("Account Summary", account_info)

    if stats_pnls:
        first_value = next(iter(stats_pnls.values()), None) if stats_pnls else None
        if first_value is not None and isinstance(first_value, dict):
            for currency, curr_stats in stats_pnls.items():
                add_section(f"PnL Statistics ({currency})", curr_stats)
        else:
            add_section("PnL Statistics", stats_pnls)

    add_section("Returns Statistics", stats_returns)

    add_section("General Statistics", stats_general)

    if not metrics:
        metrics = [_EMPTY_DATA_MESSAGE]
        values = [""]
        fill_colors = [theme_config["colors"]["table_section"]]

    return _build_table(metrics, values, fill_colors, theme_config)


def _render_run_info(
    fig: go.Figure,
    row: int,
    col: int,
    theme_config: dict[str, Any],
    run_info: dict[str, Any] | None = None,
    account_info: dict[str, Any] | None = None,
    **_kwargs: Any,
) -> None:
    """
    Render run information and account summary table.
    """
    if theme_config is None:
        theme_config = _normalize_theme_config(get_theme("plotly_white"))

    metrics = []
    values = []
    fill_colors = []

    def add_section(title: str, section_data: dict[str, Any]) -> None:
        if not section_data:
            return

        metrics.append(f"<b>{title}</b>")
        values.append("")
        fill_colors.append(theme_config["colors"]["table_section"])

        for key, value in section_data.items():
            metrics.append(key)
            values.append(_format_info_value(value))

            if len(fill_colors) % 2 == 0:
                fill_colors.append(theme_config["colors"]["table_row_odd"])
            else:
                fill_colors.append(theme_config["colors"]["table_row_even"])

    if run_info:
        add_section("Run Information", run_info)
    if account_info:
        add_section("Account Summary", account_info)

    if not metrics:
        metrics = [_EMPTY_DATA_MESSAGE]
        values = [""]
        fill_colors = [theme_config["colors"]["table_section"]]

    run_info_table = _build_table(metrics, values, fill_colors, theme_config)
    fig.add_trace(run_info_table, row=row, col=col)


def _render_stats_table(
    fig: go.Figure,
    row: int,
    col: int,
    stats_pnls: dict[str, dict[str, Any]],
    stats_returns: dict[str, Any],
    stats_general: dict[str, Any],
    theme_config: dict[str, Any],
    **_kwargs: Any,
) -> None:
    """
    Render performance statistics table (PnL, Returns, General).
    """
    stats_table = _create_stats_table(
        stats_pnls,
        stats_returns,
        stats_general,
        theme_config,
        run_info=None,
        account_info=None,
    )
    fig.add_trace(stats_table, row=row, col=col)


def _render_equity(
    fig: go.Figure,
    row: int,
    col: int,
    returns: pd.Series,
    theme_config: dict[str, Any],
    benchmark_returns: pd.Series | None = None,
    benchmark_name: str = "Benchmark",
    **_kwargs: Any,
) -> None:
    """
    Render equity curve with optional benchmark.
    """
    benchmark = (
        benchmark_returns if benchmark_returns is not None and not benchmark_returns.empty else None
    )

    if returns.empty and benchmark is None:
        _render_empty_cell(fig, row, col, theme_config)
        return

    colors = theme_config["colors"]
    bounds: list[tuple[float, float]] = []

    if not returns.empty:
        equity = (1 + returns).cumprod()
        fig.add_trace(
            go.Scatter(
                x=equity.index,
                y=equity.values,
                mode="lines",
                name="Strategy",
                line={"color": colors["primary"], "width": 2},
                fill="tozeroy",
                fillcolor=_hex_to_rgba(colors["primary"], 0.07),
                showlegend=benchmark is not None,
            ),
            row=row,
            col=col,
        )
        bounds.append((float(equity.min()), float(equity.max())))

    if benchmark is not None:
        benchmark_equity = (1 + benchmark).cumprod()
        fig.add_trace(
            go.Scatter(
                x=benchmark_equity.index,
                y=benchmark_equity.values,
                mode="lines",
                name=benchmark_name,
                line={"color": colors["neutral"], "width": 1.5, "dash": "dash"},
                showlegend=True,
            ),
            row=row,
            col=col,
        )
        bounds.append((float(benchmark_equity.min()), float(benchmark_equity.max())))

    ymin = min(lo for lo, _ in bounds)
    ymax = max(hi for _, hi in bounds)

    fig.update_xaxes(title_text="Date", row=row, col=col)
    fig.update_yaxes(title_text="Equity", range=_padded_range(ymin, ymax), row=row, col=col)


def _render_drawdown(
    fig: go.Figure,
    row: int,
    col: int,
    returns: pd.Series,
    theme_config: dict[str, Any],
    **_kwargs: Any,
) -> None:
    """
    Render drawdown chart.
    """
    if returns.empty:
        _render_empty_cell(fig, row, col, theme_config)
        return

    drawdown = _calculate_drawdown(returns)
    neg_color = theme_config["colors"]["negative"]

    fig.add_trace(
        go.Scatter(
            x=drawdown.index,
            y=drawdown.values,
            mode="lines",
            name="Drawdown",
            fill="tozeroy",
            line={"color": neg_color, "width": 1.5},
            fillcolor=_hex_to_rgba(neg_color, 0.18),
            showlegend=False,
        ),
        row=row,
        col=col,
    )

    fig.update_xaxes(title_text="Date", row=row, col=col)
    fig.update_yaxes(title_text="Drawdown (%)", row=row, col=col)


def _render_monthly_returns(
    fig: go.Figure,
    row: int,
    col: int,
    returns: pd.Series,
    theme_config: dict[str, Any] | None = None,
    **kwargs: Any,
) -> None:
    """
    Render monthly returns heatmap.
    """
    if theme_config is None:
        theme_config = _normalize_theme_config(get_theme("plotly_white"))

    if returns.empty:
        _render_empty_cell(fig, row, col, theme_config)
        return

    heatmap_data = _monthly_pivot(returns, compounding=kwargs.get("compounding", True))
    fig.add_trace(
        _heatmap_trace(heatmap_data, theme_config, showscale=False, hovertemplate=None),
        row=row,
        col=col,
    )

    fig.update_xaxes(title_text="Month", row=row, col=col)
    fig.update_yaxes(title_text="Year", row=row, col=col)


def _render_distribution(
    fig: go.Figure,
    row: int,
    col: int,
    returns: pd.Series,
    theme_config: dict[str, Any],
    **_kwargs: Any,
) -> None:
    """
    Render returns distribution histogram.
    """
    if returns.empty:
        _render_empty_cell(fig, row, col, theme_config)
        return

    colors = theme_config["colors"]
    fig.add_trace(
        go.Histogram(
            x=returns.to_numpy() * 100,
            nbinsx=50,
            name="Returns",
            marker={
                "color": colors["neutral"],
                "line": {"color": colors["background"], "width": 0.5},
            },
            showlegend=False,
        ),
        row=row,
        col=col,
    )

    fig.update_xaxes(title_text="Return (%)", row=row, col=col)
    fig.update_yaxes(title_text="Frequency", row=row, col=col)


def _render_rolling_sharpe(
    fig: go.Figure,
    row: int,
    col: int,
    returns: pd.Series,
    theme_config: dict[str, Any],
    window: int = 60,
    **_kwargs: Any,
) -> None:
    """
    Render rolling Sharpe ratio.
    """
    if returns.empty or len(returns) < window:
        _render_empty_cell(
            fig,
            row,
            col,
            theme_config,
            message=f"Not enough data (need {window} observations)",
        )
        return

    colors = theme_config["colors"]
    rolling_sharpe = _rolling_sharpe_series(returns, window)

    fig.add_trace(
        go.Scatter(
            x=rolling_sharpe.index,
            y=rolling_sharpe.values,
            mode="lines",
            name="Rolling Sharpe",
            line={"color": colors["primary"], "width": 2},
            showlegend=False,
        ),
        row=row,
        col=col,
    )

    # A zero line (add_hline with row/col cannot scan grids containing tables).
    fig.update_xaxes(title_text="Date", row=row, col=col)
    fig.update_yaxes(
        title_text="Sharpe Ratio",
        zeroline=True,
        zerolinecolor=colors["neutral"],
        zerolinewidth=1,
        row=row,
        col=col,
    )


def _render_yearly_returns(
    fig: go.Figure,
    row: int,
    col: int,
    returns: pd.Series,
    theme_config: dict[str, Any],
    **kwargs: Any,
) -> None:
    """
    Render yearly returns bar chart.
    """
    if returns.empty:
        _render_empty_cell(fig, row, col, theme_config)
        return

    colors = theme_config["colors"]
    yearly = _aggregate_period_returns(returns, "YE", compounding=kwargs.get("compounding", True))
    bar_colors = [colors["positive"] if r >= 0 else colors["negative"] for r in yearly.to_numpy()]

    fig.add_trace(
        go.Bar(
            x=yearly.index.year,
            y=yearly.to_numpy(),
            marker={
                "color": bar_colors,
                "line": {"color": colors["background"], "width": 1},
            },
            texttemplate="%{y:,.1f}%",
            textposition="outside",
            textfont={"family": _FONT_MONO, "size": 9, "color": colors["text"]},
            cliponaxis=False,
            showlegend=False,
        ),
        row=row,
        col=col,
    )

    fig.update_xaxes(title_text="Year", type="category", row=row, col=col)
    fig.update_yaxes(
        title_text="Return (%)",
        zeroline=True,
        zerolinecolor=colors["neutral"],
        zerolinewidth=1,
        row=row,
        col=col,
    )


def create_bars_with_fills(
    engine: BacktestEngine,
    bar_type: BarType,
    title: str | None = None,
    theme: str = "plotly_white",
    output_path: str | None = None,
) -> go.Figure:
    """
    Create a candlestick chart with order fills overlaid as bar charts.

    This visualization shows price bars (OHLC) as candlesticks and order fills
    as vertical bars colored by side (buy/sell).

    Parameters
    ----------
    engine
        The backtest engine with completed run.
    bar_type : BarType
        The bar type to visualize.
    title : str, optional
        Plot title. If None, uses bar_type string.
    theme : str, default "plotly_white"
        Theme name for styling.
    output_path : str, optional
        Path to save HTML plot. If None, plot is not saved.

    Returns
    -------
    go.Figure
        Plotly figure object with candlestick and fill bars.

    Raises
    ------
    ImportError
        If plotly is not installed.

    """
    if not PLOTLY_AVAILABLE:
        raise ImportError(
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]",
        )

    _require_not_none(engine, "engine")
    _require_not_none(bar_type, "bar_type")

    theme_config = _normalize_theme_config(get_theme(theme))

    bars = engine.cache.bars(bar_type)
    if not bars:
        fig = _empty_figure(
            theme_config,
            title or f"{bar_type}",
            message=f"No bars available for {bar_type}",
            height=800,
        )

        if output_path:
            _write_figure(fig, output_path)
        return fig

    fig = make_subplots(
        rows=1,
        cols=1,
        shared_xaxes=True,
        vertical_spacing=0.03,
        row_heights=[1.0],
    )

    _render_bars_with_fills(
        fig=fig,
        row=1,
        col=1,
        engine=engine,
        bar_type=bar_type,
        theme_config=theme_config,
        rangeslider=True,
    )

    _style_figure(fig, theme_config, title or f"{bar_type} - Bars with Order Fills", height=800)
    fig.update_layout(showlegend=True)

    if output_path:
        _write_figure(fig, output_path)

    return fig


def _render_bars_with_fills(  # noqa: C901
    fig: go.Figure,
    row: int,
    col: int,
    engine: BacktestEngine | None = None,
    bar_type: BarType | str | None = None,
    theme_config: dict[str, Any] | None = None,
    *,
    rangeslider: bool = False,
    **_kwargs: Any,
) -> None:
    """
    Render bars with order fills chart.

    Parameters
    ----------
    fig : go.Figure
        The figure to add traces to.
    row : int
        Row position in subplot grid.
    col : int
        Column position in subplot grid.
    engine : BacktestEngine, optional
        The backtest engine. Required.
    bar_type : str | BarType, optional
        The bar type to visualize. Can be a string or BarType object.
    theme_config : dict[str, Any], optional
        Theme configuration dictionary. If None, defaults to plotly_white theme.
    rangeslider : bool, default False
        Whether to show the x-axis range slider (standalone use; hidden in grids).
    **kwargs : Any
        Additional keyword arguments (ignored).

    """
    if theme_config is None:
        theme_config = _normalize_theme_config(get_theme("plotly_white"))

    if engine is None:
        _render_empty_cell(fig, row, col, theme_config)
        return

    if bar_type is None:
        bar_types = [
            *engine.cache.bar_types(aggregation_source=AggregationSource.EXTERNAL),
            *engine.cache.bar_types(aggregation_source=AggregationSource.INTERNAL),
        ]

        if not bar_types:
            _render_empty_cell(fig, row, col, theme_config)
            return

        bar_type = bar_types[0]

    if isinstance(bar_type, str):
        bar_type = BarType.from_str(bar_type)

    _require_not_none(engine, "engine")
    _require_not_none(bar_type, "bar_type")

    bars = engine.cache.bars(bar_type)
    if not bars:
        _render_empty_cell(
            fig,
            row,
            col,
            theme_config,
            message=f"No bars available for {bar_type}",
        )
        return

    colors = theme_config["colors"]
    bars_df = pd.DataFrame(bar.to_dict() for bar in bars)
    bars_df["ts_init"] = pd.to_datetime(bars_df["ts_init"])

    for column in ("open", "high", "low", "close"):
        bars_df[column] = bars_df[column].astype(float)

    fills_df = engine.generate_fills_report()

    if not fills_df.empty:
        instrument_id = bar_type.instrument_id
        fills_df = fills_df[fills_df["instrument_id"] == str(instrument_id)].copy()

        if not fills_df.empty:
            fills_df["ts_init"] = pd.to_datetime(fills_df["ts_init"])
            fills_df["last_qty"] = pd.to_numeric(fills_df["last_qty"], errors="coerce")
            fills_df["last_px"] = pd.to_numeric(fills_df["last_px"], errors="coerce")

    fig.add_trace(
        go.Candlestick(
            x=bars_df["ts_init"],
            open=bars_df["open"],
            high=bars_df["high"],
            low=bars_df["low"],
            close=bars_df["close"],
            increasing_line_color=colors["positive"],
            increasing_fillcolor=colors["positive"],
            decreasing_line_color=colors["negative"],
            decreasing_fillcolor=colors["negative"],
            name="OHLC",
            showlegend=False,
        ),
        row=row,
        col=col,
    )

    if not fills_df.empty and "ts_init" in fills_df.columns:
        buy_fills = (
            fills_df[fills_df["order_side"] == "BUY"]
            if "order_side" in fills_df.columns
            else pd.DataFrame()
        )
        sell_fills = (
            fills_df[fills_df["order_side"] == "SELL"]
            if "order_side" in fills_df.columns
            else pd.DataFrame()
        )

        is_dark = "dark" in theme_config.get("template", "")
        marker_line = "rgba(255,255,255,0.85)" if is_dark else "rgba(0,0,0,0.55)"

        _add_fill_scatter_trace(
            fig=fig,
            fills_df=buy_fills,
            row=row,
            col=col,
            marker_symbol="triangle-up",
            marker_color=_hex_to_rgba(colors["positive"], 0.85),
            marker_line_color=marker_line,
            name="Buy Fills",
        )

        _add_fill_scatter_trace(
            fig=fig,
            fills_df=sell_fills,
            row=row,
            col=col,
            marker_symbol="triangle-down",
            marker_color=_hex_to_rgba(colors["negative"], 0.85),
            marker_line_color=marker_line,
            name="Sell Fills",
        )

    fig.update_xaxes(
        title_text="Time",
        row=row,
        col=col,
        rangeslider={"visible": rangeslider},
    )
    fig.update_yaxes(title_text="Price", row=row, col=col)
    fig.update_yaxes(fixedrange=False, row=row, col=col)


def _add_fill_scatter_trace(
    fig: go.Figure,
    fills_df: pd.DataFrame,
    row: int,
    col: int,
    marker_symbol: str,
    marker_color: str,
    name: str,
    marker_line_color: str = "rgba(0,0,0,0.55)",
) -> None:
    if fills_df.empty or "last_px" not in fills_df.columns or "last_qty" not in fills_df.columns:
        return

    required_cols = [
        "strategy_id",
        "instrument_id",
        "order_side",
        "last_qty",
    ]
    has_all_cols = all(col in fills_df.columns for col in required_cols)

    fig.add_trace(
        go.Scatter(
            x=fills_df["ts_init"],
            y=fills_df["last_px"],
            mode="markers",
            customdata=fills_df[required_cols].to_numpy() if has_all_cols else fills_df.to_numpy(),
            marker_symbol=marker_symbol,
            marker_size=10,
            marker_line_width=1.2,
            marker_line_color=marker_line_color,
            marker_color=marker_color,
            name=name,
            hovertemplate=(
                "Time: %{x}<br>"
                "Price: %{y:.2f}<br>"
                "Strategy: %{customdata[0]}<br>"
                "Instrument: %{customdata[1]}<br>"
                "Side: %{customdata[2]}<br>"
                "Quantity: %{customdata[3]:.2f}<br>"
                "<extra></extra>"
            )
            if has_all_cols
            else ("<b>%{x}</b><br>Price: %{y:.2f}<br>Quantity: %{customdata}<br><extra></extra>"),
            showlegend=True,
        ),
        row=row,
        col=col,
    )


_TEARSHEET_CHART_SPECS: dict[str, dict[str, Any]] = {}


def register_tearsheet_chart(
    name: str,
    subplot_type: str,
    title: str,
    renderer: Callable,
) -> None:
    """
    Register a custom chart renderer for tearsheet integration.

    The registered ``name`` can then be placed in a tearsheet via
    ``TearsheetConfig(charts=[TearsheetCustomChart(chart=name)])``. This differs
    from ``register_chart``, which registers standalone chart functions that
    return their own figure; tearsheet renderers instead draw onto a shared
    subplot grid cell.

    Parameters
    ----------
    name : str
        Chart name referenced by ``TearsheetCustomChart(chart=name)``.
    subplot_type : str
        Plotly subplot type ('scatter', 'bar', 'table', 'heatmap', 'histogram').
    title : str
        Display title for the subplot.
    renderer : Callable
        Function that adds traces to the figure. Signature:
        renderer(fig, row, col, returns, stats_pnls, stats_returns, stats_general,
        theme_config, benchmark_returns, benchmark_name, run_info, account_info, engine, **kwargs)

    Raises
    ------
    ValueError
        If name is empty or renderer is not callable.

    """
    _require_not_none(name, "name")

    if not name.strip():
        raise ValueError("Chart name cannot be empty")

    if not callable(renderer):
        raise ValueError(f"Chart renderer must be callable, was {type(renderer)}")

    _TEARSHEET_CHART_SPECS[name] = {
        "type": subplot_type,
        "title": title,
        "renderer": renderer,
    }


def _calculate_grid_layout(
    charts: list[TearsheetChart],
    custom_layout: GridLayout | None = None,
) -> tuple[int, int, list, list[str], list[float], float, float]:
    """
    Calculate dynamic grid layout based on selected charts.

    Parameters
    ----------
    charts : list[TearsheetChart]
        List of chart objects to include (in order).
    custom_layout : GridLayout, optional
        Custom layout specification.

    Returns
    -------
    tuple
        (rows, cols, specs, subplot_titles, row_heights, vertical_spacing, horizontal_spacing)

    """
    if not charts:
        return (
            1,
            1,
            [[{"type": "scatter"}]],
            [""],
            [1.0],
            _GRID_DEFAULT_SPACING,
            _GRID_DEFAULT_SPACING,
        )

    if custom_layout is not None:
        rows = custom_layout.rows
        cols = custom_layout.cols
        heights = custom_layout.heights
        v_spacing = custom_layout.vertical_spacing
        h_spacing = custom_layout.horizontal_spacing
    else:
        num_charts = len(charts)
        if num_charts <= _GRID_ONE_ROW_MAX_CHARTS:
            rows, cols = 1, num_charts
            heights = [1.0 / rows] * rows
        elif num_charts <= _GRID_TWO_ROWS_MAX_CHARTS:
            rows, cols = 2, 2
            heights = [1.0 / rows] * rows
        elif num_charts <= _GRID_THREE_ROWS_MAX_CHARTS:
            rows, cols = 3, 2
            heights = [1.0 / rows] * rows
        else:
            cols = 2
            rows = max(_GRID_DEFAULT_ROWS, (num_charts + cols - 1) // cols)
            heights = (
                list(_GRID_DEFAULT_HEIGHTS) if rows == _GRID_DEFAULT_ROWS else [1.0 / rows] * rows
            )

        v_spacing = _GRID_DEFAULT_SPACING
        h_spacing = _GRID_DEFAULT_SPACING

    if len(charts) > rows * cols:
        raise ValueError(
            f"Grid has {rows * cols} cells but {len(charts)} charts were configured; "
            f"provide a larger GridLayout via TearsheetConfig.layout",
        )

    specs = []
    titles = []
    chart_idx = 0

    for _ in range(rows):
        row_specs: list[dict[str, Any] | None] = []

        for _ in range(cols):
            if chart_idx < len(charts):
                chart = charts[chart_idx]
                chart_name = chart.name
                spec = _TEARSHEET_CHART_SPECS.get(chart_name, {})
                subplot_type = spec.get("type", "scatter")
                default_title = spec.get("title", chart_name.replace("_", " ").title())
                title = chart.title or default_title

                row_specs.append({"type": subplot_type})
                titles.append(title)
                chart_idx += 1
            else:
                row_specs.append(None)
                titles.append("")
        specs.append(row_specs)

    return rows, cols, specs, titles, heights, v_spacing, h_spacing


register_chart("equity", create_equity_curve)
register_chart("drawdown", create_drawdown_chart)
register_chart("monthly_returns", create_monthly_returns_heatmap)
register_chart("distribution", create_returns_distribution)
register_chart("rolling_sharpe", create_rolling_sharpe)
register_chart("yearly_returns", create_yearly_returns)
register_chart("bars_with_fills", create_bars_with_fills)

register_tearsheet_chart("run_info", "table", "Run Information", _render_run_info)
register_tearsheet_chart("stats_table", "table", "Performance Statistics", _render_stats_table)
register_tearsheet_chart("equity", "scatter", "Equity Curve", _render_equity)
register_tearsheet_chart("drawdown", "scatter", "Drawdown", _render_drawdown)
register_tearsheet_chart("monthly_returns", "heatmap", "Monthly Returns", _render_monthly_returns)
register_tearsheet_chart("distribution", "histogram", "Returns Distribution", _render_distribution)
register_tearsheet_chart(
    "rolling_sharpe",
    "scatter",
    "Rolling Sharpe Ratio (60-day)",
    _render_rolling_sharpe,
)
register_tearsheet_chart("yearly_returns", "bar", "Yearly Returns", _render_yearly_returns)
register_tearsheet_chart(
    "bars_with_fills",
    "scatter",
    "Bars with Order Fills",
    _render_bars_with_fills,
)
