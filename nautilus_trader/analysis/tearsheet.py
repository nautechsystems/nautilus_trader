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
Backtest visualization and tearsheet generation using Plotly.

This module provides functions to create interactive tearsheets and plots from backtest
results, leveraging existing PortfolioAnalyzer statistics and ReportProvider DataFrames.

"""

from __future__ import annotations

import numbers
from collections.abc import Callable
from difflib import get_close_matches
from typing import TYPE_CHECKING
from typing import Any

import pandas as pd

from nautilus_trader.core.correctness import PyCondition


try:
    import plotly.graph_objects as go
    from plotly.subplots import make_subplots

    PLOTLY_AVAILABLE = True
except ImportError:
    PLOTLY_AVAILABLE = False
    if not TYPE_CHECKING:
        # Define dummy types for when plotly is not installed
        go = None  # type: ignore


# Constants
TRADING_DAYS_PER_YEAR = 252  # Standard number of trading days for annualization

# Chart registry for custom visualizations
_CHART_REGISTRY: dict[str, Callable] = {}


def _hex_to_rgba(hex_color: str, alpha: float = 1.0) -> str:
    """
    Convert hex color to rgba format.

    Parameters
    ----------
    hex_color : str
        Hex color code (e.g., "#d62728").
    alpha : float, default 1.0
        Alpha/opacity value (0.0 to 1.0).

    Returns
    -------
    str
        RGBA color string (e.g., "rgba(214, 39, 40, 0.3)").

    """
    hex_color = hex_color.lstrip("#")
    r = int(hex_color[0:2], 16)
    g = int(hex_color[2:4], 16)
    b = int(hex_color[4:6], 16)
    return f"rgba({r}, {g}, {b}, {alpha})"


def _normalize_theme_config(theme_config: dict[str, Any]) -> dict[str, Any]:
    """
    Ensure theme config has all required color keys with sensible defaults.

    This provides backward compatibility for themes registered before
    table-specific colors were added.

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

    # Provide defaults for table-specific colors if missing
    if "table_section" not in colors:
        colors["table_section"] = colors.get("grid", "#e0e0e0")
    if "table_row_odd" not in colors:
        colors["table_row_odd"] = colors.get("grid", "#f0f0f0")
    if "table_row_even" not in colors:
        colors["table_row_even"] = colors.get("background", "#ffffff")
    if "table_text" not in colors:
        # Use contrasting color based on background brightness
        bg = colors.get("background", "#ffffff")
        colors["table_text"] = "#000000" if bg.lower() in ["#ffffff", "#fff"] else "#eeeeee"

    theme_config["colors"] = colors
    return theme_config


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

    # Calculate cumulative returns from all returns
    cumulative = (1 + returns).cumprod()

    # Prepend baseline 1.0 at time before first return to establish starting equity
    baseline_time = returns.index.min() - pd.Timedelta(seconds=1)
    baseline = pd.Series([1.0], index=[baseline_time])
    cumulative = pd.concat([baseline, cumulative])

    # Calculate running maximum
    running_max = cumulative.cummax()

    # Calculate drawdown as percentage
    return (cumulative - running_max) / running_max * 100


def register_chart(name: str, func: Callable | None = None) -> Callable | None:
    """
    Register a custom chart function for use in tearsheets.

    Can be used as a decorator or called directly.

    Parameters
    ----------
    name : str
        The chart name for reference in TearsheetConfig.charts list.
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
    >>> register_chart("another_chart", my_chart_function)

    """
    PyCondition.not_none(name, "name")

    if not name.strip():
        raise ValueError("Chart name cannot be empty")

    # Used as a decorator: @register_chart("name")
    if func is None:

        def decorator(f: Callable) -> Callable:
            if not callable(f):
                raise ValueError(f"Chart function must be callable, got {type(f)}")
            _CHART_REGISTRY[name] = f
            return f

        return decorator

    # Used directly: register_chart("name", func)
    if not callable(func):
        raise ValueError(f"Chart function must be callable, got {type(func)}")

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
    PyCondition.not_none(name, "name")

    if name not in _CHART_REGISTRY:
        available = ", ".join(_CHART_REGISTRY.keys())

        # Suggest close matches
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


def create_tearsheet(  # noqa: C901
    engine,
    output_path: str | None = "tearsheet.html",
    title: str = "NautilusTrader Backtest Results",
    currency=None,
    config=None,
    benchmark_returns: pd.Series | None = None,
    benchmark_name: str = "Benchmark",
) -> str | None:
    """
    Generate an interactive HTML tearsheet from backtest results.

    Parameters
    ----------
    engine : BacktestEngine
        The backtest engine with completed run.
    output_path : str, optional
        Path to save HTML tearsheet. If None, returns HTML string.
    title : str, default "NautilusTrader Backtest Results"
        Title for the tearsheet.
    currency : Currency, optional
        Currency for PnL statistics. If None, uses first available currency.
    config : TearsheetConfig, optional
        Configuration for tearsheet customization. If None, uses default configuration.
    benchmark_returns : pd.Series, optional
        Benchmark returns series for comparison. If provided, benchmark will be overlaid
        on visualizations.
    benchmark_name : str, default "Benchmark"
        Display name for the benchmark.

    Returns
    -------
    str or None
        HTML string if output_path is None, otherwise None.

    Raises
    ------
    ImportError
        If plotly is not installed.

    """
    if not PLOTLY_AVAILABLE:
        msg = (
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]"
        )
        raise ImportError(msg)

    # Extract data from engine
    analyzer = engine.portfolio.analyzer
    stats_returns = analyzer.get_performance_stats_returns()
    stats_general = analyzer.get_performance_stats_general()
    returns = analyzer.returns()

    # Build title with strategy name(s) and run time
    if title == "NautilusTrader Backtest Results":
        # Extract strategy names
        strategy_names = []
        if hasattr(engine, "trader") and hasattr(engine.trader, "strategies"):
            strategies = engine.trader.strategies()
            strategy_names = [str(s.id) for s in strategies]

        # Format run time
        run_time = "N/A"
        if hasattr(engine, "run_started") and engine.run_started:
            run_time = str(engine.run_started)

        # Build title: "NautilusTrader - Backtest Results<br>Strategy | Run Time"
        subtitle_parts = []
        if strategy_names:
            subtitle_parts.append(", ".join(strategy_names))
        if run_time != "N/A":
            subtitle_parts.append(run_time)

        if subtitle_parts:
            title = f"<b>NautilusTrader</b> v1.222.0 - Backtest Results<br><sub>{' | '.join(subtitle_parts)}</sub>"
        else:
            title = "<b>NautilusTrader</b> v1.222.0 - Backtest Results"

    # Extract run information
    total_positions = 0
    if hasattr(engine, "kernel"):
        positions = list(engine.kernel.cache.positions()) + list(
            engine.kernel.cache.position_snapshots()
        )
        total_positions = len(positions)

    run_info = {
        "Run ID": str(engine.run_id) if hasattr(engine, "run_id") else "N/A",
        "Run Started": str(engine.run_started) if hasattr(engine, "run_started") else "N/A",
        "Run Finished": str(engine.run_finished) if hasattr(engine, "run_finished") else "N/A",
        "Backtest Start": str(engine.backtest_start)
        if hasattr(engine, "backtest_start")
        else "N/A",
        "Backtest End": str(engine.backtest_end) if hasattr(engine, "backtest_end") else "N/A",
        "Iterations": f"{engine.iteration:,}" if hasattr(engine, "iteration") else "N/A",
        "Total Events": f"{engine.kernel.exec_engine.event_count:,}"
        if hasattr(engine, "kernel")
        else "N/A",
        "Total Orders": f"{engine.kernel.cache.orders_total_count():,}"
        if hasattr(engine, "kernel")
        else "N/A",
        "Total Positions": f"{total_positions:,}",
    }

    # Determine which currencies to display
    all_currencies = analyzer.currencies
    if currency is not None:
        # User specified a currency, only show that one
        currencies = [currency] if currency in all_currencies else []
    else:
        # No currency specified, show all
        currencies = all_currencies

    # Extract account information per currency
    account_info = {}
    if currencies:
        for curr in currencies:
            starting = analyzer._account_balances_starting.get(curr)
            ending = analyzer._account_balances.get(curr)
            if starting and ending:
                account_info[f"Starting Balance ({curr})"] = f"{starting.as_double():.8f} {curr}"
                account_info[f"Ending Balance ({curr})"] = f"{ending.as_double():.8f} {curr}"

    # Get PnL stats for selected currencies
    all_stats_pnls = {}
    if currencies:
        for curr in currencies:
            curr_stats = analyzer.get_performance_stats_pnls(currency=curr)
            if curr_stats:  # Only include if there are stats
                all_stats_pnls[str(curr)] = curr_stats

    # Delegate to stats-first API
    return create_tearsheet_from_stats(
        run_info=run_info,
        account_info=account_info,
        stats_pnls=all_stats_pnls,
        stats_returns=stats_returns,
        stats_general=stats_general,
        returns=returns,
        output_path=output_path,
        title=title,
        config=config,
        benchmark_returns=benchmark_returns,
        benchmark_name=benchmark_name,
    )


def create_tearsheet_from_stats(
    stats_pnls: dict[str, Any] | dict[str, dict[str, Any]],
    stats_returns: dict[str, Any],
    stats_general: dict[str, Any],
    returns: pd.Series,
    output_path: str | None = "tearsheet.html",
    title: str = "NautilusTrader Backtest Results",
    config=None,
    benchmark_returns: pd.Series | None = None,
    benchmark_name: str = "Benchmark",
    run_info: dict[str, Any] | None = None,
    account_info: dict[str, Any] | None = None,
) -> str | None:
    """
    Generate an interactive HTML tearsheet from precomputed statistics.

    This lower-level API is useful for offline analysis when you have
    precomputed statistics and don't want to pass an engine.

    Parameters
    ----------
    stats_pnls : dict[str, Any]
        PnL-based statistics from analyzer.
    stats_returns : dict[str, Any]
        Returns-based statistics from analyzer.
    stats_general : dict[str, Any]
        General statistics from analyzer.
    returns : pd.Series
        Returns series from analyzer.
    output_path : str, optional
        Path to save HTML tearsheet. If None, returns HTML string.
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
    >>> stats_returns = {"Sharpe Ratio (252 days)": 1.5, ...}
    >>> stats_general = {"Win Rate": 0.55, ...}
    >>> stats_pnls = {"PnL (total)": 10000.0, ...}
    >>> returns = pd.Series([0.01, -0.02, ...])
    >>> html = create_tearsheet_from_stats(
    ...     stats_pnls, stats_returns, stats_general, returns,
    ...     output_path=None  # Return HTML instead of saving
    ... )

    """
    if not PLOTLY_AVAILABLE:
        msg = (
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]"
        )
        raise ImportError(msg)

    # Use default config if none provided
    if config is None:
        from nautilus_trader.analysis import TearsheetConfig

        config = TearsheetConfig()

    # Filter out run_info chart if no metadata is available
    # This prevents an empty subplot from wasting grid space
    if not run_info and not account_info and "run_info" in config.charts:
        from nautilus_trader.analysis import TearsheetConfig

        # Create new config without run_info chart
        config = TearsheetConfig(
            charts=[c for c in config.charts if c != "run_info"],
            theme=config.theme,
            layout=config.layout,
            title=config.title,
            include_benchmark=config.include_benchmark,
            benchmark_name=config.benchmark_name,
            height=config.height,
            show_logo=config.show_logo,
        )

    # Create figure with subplots
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
    )

    # Save to HTML or return as string
    if output_path:
        fig.write_html(output_path)
        return None
    else:
        return fig.to_html()


def create_equity_curve(
    returns: pd.Series,
    output_path: str | None = None,
    title: str = "Equity Curve",
    benchmark_returns: pd.Series | None = None,
    benchmark_name: str = "Benchmark",
) -> go.Figure:
    """
    Create an interactive equity curve plot with optional benchmark overlay.

    Parameters
    ----------
    returns : pd.Series
        Returns series from portfolio analyzer.
    output_path : str, optional
        Path to save HTML plot. If None, plot is not saved.
    title : str, default "Equity Curve"
        Plot title.
    benchmark_returns : pd.Series, optional
        Benchmark returns series for comparison. If provided, benchmark equity
        curve will be overlaid on the chart.
    benchmark_name : str, default "Benchmark"
        Display name for the benchmark in the legend.

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
        msg = (
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]"
        )
        raise ImportError(msg)

    # Calculate cumulative returns (equity curve)
    equity = (1 + returns).cumprod()

    fig = go.Figure()
    fig.add_trace(
        go.Scatter(
            x=equity.index,
            y=equity.values,
            mode="lines",
            name="Strategy",
            line={"color": "#1f77b4", "width": 2},
            hovertemplate="<b>%{x}</b><br>Strategy: %{y:.4f}<extra></extra>",
        ),
    )

    # Add benchmark if provided
    if benchmark_returns is not None:
        benchmark_equity = (1 + benchmark_returns).cumprod()
        fig.add_trace(
            go.Scatter(
                x=benchmark_equity.index,
                y=benchmark_equity.values,
                mode="lines",
                name=benchmark_name,
                line={"color": "#7f7f7f", "width": 2, "dash": "dash"},
                hovertemplate=f"<b>%{{x}}</b><br>{benchmark_name}: %{{y:.4f}}<extra></extra>",
            ),
        )

    fig.update_layout(
        title=title,
        xaxis_title="Date",
        yaxis_title="Equity",
        hovermode="x unified",
        template="plotly_white",
        height=500,
        showlegend=benchmark_returns is not None,
    )

    if output_path:
        fig.write_html(output_path)

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
        Returns series from portfolio analyzer.
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
        msg = (
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]"
        )
        raise ImportError(msg)

    from nautilus_trader.analysis.themes import get_theme

    theme_config = _normalize_theme_config(get_theme(theme))
    drawdown = _calculate_drawdown(returns)
    neg_color = theme_config["colors"]["negative"]
    fig = go.Figure()
    fig.add_trace(
        go.Scatter(
            x=drawdown.index,
            y=drawdown.values,
            mode="lines",
            name="Drawdown",
            fill="tozeroy",
            line={"color": neg_color, "width": 1},
            fillcolor=_hex_to_rgba(neg_color, 0.3),  # 30% opacity
            hovertemplate="<b>%{x}</b><br>Drawdown: %{y:.2f}%<extra></extra>",
        ),
    )

    fig.update_layout(
        title=title,
        xaxis_title="Date",
        yaxis_title="Drawdown (%)",
        hovermode="x unified",
        template=theme_config["template"],
        height=400,
    )

    if output_path:
        fig.write_html(output_path)

    return fig


def create_monthly_returns_heatmap(
    returns: pd.Series,
    output_path: str | None = None,
    title: str = "Monthly Returns (%)",
) -> go.Figure:
    """
    Create an interactive monthly returns heatmap.

    Parameters
    ----------
    returns : pd.Series
        Returns series from portfolio analyzer.
    output_path : str, optional
        Path to save HTML plot. If None, plot is not saved.
    title : str, default "Monthly Returns (%)"
        Plot title.

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
        msg = (
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]"
        )
        raise ImportError(msg)

    if returns.empty:
        # Return empty figure if no data
        fig = go.Figure()
        fig.update_layout(title=title)
        return fig

    # Resample to monthly returns
    monthly = returns.resample("ME").apply(lambda x: (1 + x).prod() - 1) * 100

    # Pivot to year x month matrix
    monthly_pivot = pd.DataFrame(
        {
            "Year": monthly.index.year,
            "Month": monthly.index.month,
            "Return": monthly.to_numpy(),
        },
    )
    heatmap_data = monthly_pivot.pivot_table(index="Year", columns="Month", values="Return")

    # Month names for x-axis
    month_names = [
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

    fig = go.Figure(
        data=go.Heatmap(
            z=heatmap_data.values,
            x=[month_names[int(m) - 1] for m in heatmap_data.columns],
            y=heatmap_data.index.astype(str),
            colorscale="RdYlGn",
            zmid=0,
            text=heatmap_data.values,
            texttemplate="%{text:.1f}%",
            textfont={"size": 10},
            hovertemplate="<b>%{y} %{x}</b><br>Return: %{z:.2f}%<extra></extra>",
        ),
    )

    fig.update_layout(
        title=title,
        xaxis_title="Month",
        yaxis_title="Year",
        template="plotly_white",
        height=400,
    )

    if output_path:
        fig.write_html(output_path)

    return fig


def create_returns_distribution(
    returns: pd.Series,
    output_path: str | None = None,
    title: str = "Returns Distribution",
) -> go.Figure:
    """
    Create an interactive returns distribution histogram.

    Parameters
    ----------
    returns : pd.Series
        Returns series from portfolio analyzer.
    output_path : str, optional
        Path to save HTML plot. If None, plot is not saved.
    title : str, default "Returns Distribution"
        Plot title.

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
        msg = (
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]"
        )
        raise ImportError(msg)

    fig = go.Figure()
    fig.add_trace(
        go.Histogram(
            x=returns.to_numpy() * 100,  # Convert to percentage
            nbinsx=50,
            name="Returns",
            marker={"color": "#1f77b4"},
            hovertemplate="Return: %{x:.2f}%<br>Count: %{y}<extra></extra>",
        ),
    )

    fig.update_layout(
        title=title,
        xaxis_title="Return (%)",
        yaxis_title="Frequency",
        template="plotly_white",
        height=400,
        bargap=0.1,
    )

    if output_path:
        fig.write_html(output_path)

    return fig


def create_rolling_sharpe(
    returns: pd.Series,
    window: int = 60,
    output_path: str | None = None,
    title: str = "Rolling Sharpe Ratio (60-day)",
) -> go.Figure:
    """
    Create an interactive rolling Sharpe ratio chart.

    Parameters
    ----------
    returns : pd.Series
        Returns series from portfolio analyzer.
    window : int, default 60
        Rolling window size in days.
    output_path : str, optional
        Path to save HTML plot. If None, plot is not saved.
    title : str, default "Rolling Sharpe Ratio (60-day)"
        Plot title.

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
        msg = (
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]"
        )
        raise ImportError(msg)

    if returns.empty or len(returns) < window:
        # Return empty figure if insufficient data
        fig = go.Figure()
        fig.update_layout(title=title)
        return fig

    # Calculate rolling Sharpe ratio
    # Sharpe = (mean / std) * sqrt(TRADING_DAYS_PER_YEAR) annualized
    rolling_mean = returns.rolling(window=window).mean()
    rolling_std = returns.rolling(window=window).std()

    # Avoid division by zero
    rolling_sharpe = (rolling_mean / rolling_std.replace(0, float("nan"))) * (
        TRADING_DAYS_PER_YEAR**0.5
    )

    fig = go.Figure()
    fig.add_trace(
        go.Scatter(
            x=rolling_sharpe.index,
            y=rolling_sharpe.values,
            mode="lines",
            name=f"Rolling Sharpe ({window}d)",
            line={"color": "#2ca02c", "width": 2},
            hovertemplate="<b>%{x}</b><br>Sharpe: %{y:.3f}<extra></extra>",
        ),
    )

    # Add zero line for reference
    fig.add_hline(y=0, line={"color": "gray", "dash": "dash", "width": 1})

    fig.update_layout(
        title=title,
        xaxis_title="Date",
        yaxis_title="Sharpe Ratio",
        hovermode="x unified",
        template="plotly_white",
        height=400,
    )

    if output_path:
        fig.write_html(output_path)

    return fig


def create_yearly_returns(
    returns: pd.Series,
    output_path: str | None = None,
    title: str = "Yearly Returns",
) -> go.Figure:
    """
    Create an interactive yearly returns bar chart.

    Parameters
    ----------
    returns : pd.Series
        Returns series from portfolio analyzer.
    output_path : str, optional
        Path to save HTML plot. If None, plot is not saved.
    title : str, default "Yearly Returns"
        Plot title.

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
        msg = (
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]"
        )
        raise ImportError(msg)

    if returns.empty:
        # Return empty figure if no data
        fig = go.Figure()
        fig.update_layout(title=title)
        return fig

    # Resample to yearly returns
    yearly = returns.resample("YE").apply(lambda x: (1 + x).prod() - 1) * 100

    # Determine bar colors (green for positive, red for negative)
    colors = ["#2ca02c" if r >= 0 else "#d62728" for r in yearly.to_numpy()]

    fig = go.Figure()
    fig.add_trace(
        go.Bar(
            x=yearly.index.year,
            y=yearly.values,
            marker={"color": colors},
            hovertemplate="<b>%{x}</b><br>Return: %{y:.2f}%<extra></extra>",
        ),
    )

    # Add zero line for reference
    fig.add_hline(y=0, line={"color": "gray", "width": 1})

    fig.update_layout(
        title=title,
        xaxis_title="Year",
        yaxis_title="Return (%)",
        template="plotly_white",
        height=400,
        showlegend=False,
    )

    if output_path:
        fig.write_html(output_path)

    return fig


def _create_tearsheet_figure(
    stats_returns: dict[str, Any],
    stats_general: dict[str, Any],
    stats_pnls: dict[str, Any] | dict[str, dict[str, Any]],
    returns: pd.Series,
    title: str,
    config=None,
    benchmark_returns: pd.Series | None = None,
    benchmark_name: str = "Benchmark",
    run_info: dict[str, Any] | None = None,
    account_info: dict[str, Any] | None = None,
) -> go.Figure:
    """
    Create the complete tearsheet figure with subplots using dynamic chart registry.

    Parameters
    ----------
    stats_returns : dict[str, Any]
        Returns-based statistics from analyzer.
    stats_general : dict[str, Any]
        General statistics from analyzer.
    stats_pnls : dict[str, Any]
        PnL-based statistics from analyzer.
    returns : pd.Series
        Returns series from analyzer.
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

    Returns
    -------
    go.Figure
        Complete tearsheet figure.

    """
    # Import here to avoid circular dependency
    from nautilus_trader.analysis.config import TearsheetConfig
    from nautilus_trader.analysis.themes import get_theme

    # Use default config if none provided
    if config is None:
        config = TearsheetConfig()

    # Get theme configuration and normalize for backward compatibility
    theme_config = _normalize_theme_config(get_theme(config.theme))

    # Honor benchmark configuration
    # Only hide benchmark if explicitly disabled via config
    if not config.include_benchmark and benchmark_returns is not None:
        benchmark_returns = None

    # Use provided benchmark_name parameter if not default, otherwise use config
    if benchmark_name == "Benchmark":  # Still using default value
        benchmark_name = config.benchmark_name

    # Calculate dynamic grid layout based on selected charts
    rows, cols, specs, subplot_titles, heights, v_spacing, h_spacing = _calculate_grid_layout(
        config.charts,
        config.layout,
    )

    # Create subplots with dynamic layout
    fig = make_subplots(
        rows=rows,
        cols=cols,
        subplot_titles=subplot_titles,
        specs=specs,
        vertical_spacing=v_spacing,
        horizontal_spacing=h_spacing,
        row_heights=heights,
    )

    # Render each chart using its registered renderer
    chart_idx = 0

    for row in range(1, rows + 1):
        for col in range(1, cols + 1):
            if chart_idx >= len(config.charts):
                break

            chart_name = config.charts[chart_idx]
            chart_idx += 1

            # Get chart spec
            if chart_name not in _TEARSHEET_CHART_SPECS:
                # Skip unknown charts (could log a warning)
                continue

            # Get renderer function
            renderer = _TEARSHEET_CHART_SPECS[chart_name]["renderer"]

            # Call renderer with all available data
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
            )

    # Update global layout
    fig.update_layout(
        title_text=config.title if config.title != "NautilusTrader Backtest Results" else title,
        title_font_size=20,  # Larger title font
        template=theme_config["template"],
        height=config.height,
        showlegend=benchmark_returns is not None,
        margin={"t": 150, "b": 50, "l": 50, "r": 50},  # Increased top margin for more padding
    )

    return fig


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
    # Use default theme if not provided
    if theme_config is None:
        from nautilus_trader.analysis.themes import get_theme

        theme_config = _normalize_theme_config(get_theme("plotly_white"))
    # Build table rows with section headers
    metrics = []
    values = []
    fill_colors = []

    # Helper to add section
    def add_section(title: str, section_stats: dict[str, Any]) -> None:
        if not section_stats:
            return

        # Add section header
        metrics.append(f"<b>{title}</b>")
        values.append("")
        fill_colors.append(theme_config["colors"]["table_section"])

        # Add stats from this section
        for metric, value in section_stats.items():
            metrics.append(metric)
            formatted_value = (
                f"{value:.4f}"
                if isinstance(value, numbers.Real) and not isinstance(value, bool)
                else str(value)
            )
            values.append(formatted_value)
            # Alternate row colors using theme colors
            if len(fill_colors) % 2 == 0:
                fill_colors.append(theme_config["colors"]["table_row_odd"])
            else:
                fill_colors.append(theme_config["colors"]["table_row_even"])

    # Add sections in post-run order:
    # 1. Run Information
    if run_info:
        add_section("Run Information", run_info)

    # 2. Account Summary
    if account_info:
        add_section("Account Summary", account_info)

    # 3. PnL Statistics (per currency if dict of dicts, otherwise single)
    if stats_pnls:
        # Check if it's per-currency (dict of dicts) or single dict
        first_value = next(iter(stats_pnls.values()), None) if stats_pnls else None
        if first_value is not None and isinstance(first_value, dict):
            # Per-currency PnL stats
            for currency, curr_stats in stats_pnls.items():
                add_section(f"PnL Statistics ({currency})", curr_stats)
        else:
            # Single currency PnL stats
            add_section("PnL Statistics", stats_pnls)

    # 4. Returns Statistics
    add_section("Returns Statistics", stats_returns)

    # 5. General/Position Statistics
    add_section("General Statistics", stats_general)

    return go.Table(
        header={
            "values": ["<b>Metric</b>", "<b>Value</b>"],
            "fill_color": theme_config["colors"]["primary"],
            "font": {"color": "white", "size": 12},
            "align": "left",
        },
        cells={
            "values": [metrics, values],
            "fill_color": [fill_colors, fill_colors],
            "align": "left",
            "font": {"size": 11, "color": theme_config["colors"]["table_text"]},
        },
    )


# Tearsheet chart renderers
def _render_run_info(
    fig: go.Figure,
    row: int,
    col: int,
    theme_config: dict[str, Any],
    run_info: dict[str, Any] | None = None,
    account_info: dict[str, Any] | None = None,
    **kwargs: Any,
) -> None:
    """
    Render run information and account summary table.
    """
    if theme_config is None:
        from nautilus_trader.analysis.themes import get_theme

        theme_config = _normalize_theme_config(get_theme("plotly_white"))

    metrics = []
    values = []
    fill_colors = []

    def add_section(title: str, section_data: dict[str, Any]) -> None:
        if not section_data:
            return

        # Add section header
        metrics.append(f"<b>{title}</b>")
        values.append("")
        fill_colors.append(theme_config["colors"]["table_section"])

        # Add data rows
        for key, value in section_data.items():
            metrics.append(key)
            values.append(str(value))
            if len(fill_colors) % 2 == 0:
                fill_colors.append(theme_config["colors"]["table_row_odd"])
            else:
                fill_colors.append(theme_config["colors"]["table_row_even"])

    # Add run info and account info
    if run_info:
        add_section("Run Information", run_info)
    if account_info:
        add_section("Account Summary", account_info)

    if not metrics:  # No data to display
        return

    run_info_table = go.Table(
        header={
            "values": ["<b>Metric</b>", "<b>Value</b>"],
            "fill_color": theme_config["colors"]["primary"],
            "font": {"color": "white", "size": 12},
            "align": "left",
        },
        cells={
            "values": [metrics, values],
            "fill_color": [fill_colors, fill_colors],
            "align": "left",
            "font": {"size": 11, "color": theme_config["colors"]["table_text"]},
        },
    )
    fig.add_trace(run_info_table, row=row, col=col)


def _render_stats_table(
    fig: go.Figure,
    row: int,
    col: int,
    stats_pnls: dict[str, dict[str, Any]],
    stats_returns: dict[str, Any],
    stats_general: dict[str, Any],
    theme_config: dict[str, Any],
    **kwargs: Any,
) -> None:
    """
    Render performance statistics table (PnL, Returns, General).
    """
    stats_table = _create_stats_table(
        stats_pnls,
        stats_returns,
        stats_general,
        theme_config,
        run_info=None,  # Don't include run info here
        account_info=None,  # Don't include account info here
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
    **kwargs: Any,
) -> None:
    """
    Render equity curve with optional benchmark.
    """
    if returns.empty:
        return

    equity = (1 + returns).cumprod()
    fig.add_trace(
        go.Scatter(
            x=equity.index,
            y=equity.values,
            mode="lines",
            name="Strategy",
            line={"color": theme_config["colors"]["primary"], "width": 2},
            showlegend=benchmark_returns is not None,
        ),
        row=row,
        col=col,
    )

    if benchmark_returns is not None and not benchmark_returns.empty:
        benchmark_equity = (1 + benchmark_returns).cumprod()
        fig.add_trace(
            go.Scatter(
                x=benchmark_equity.index,
                y=benchmark_equity.values,
                mode="lines",
                name=benchmark_name,
                line={"color": theme_config["colors"]["neutral"], "width": 2, "dash": "dash"},
                showlegend=True,
            ),
            row=row,
            col=col,
        )

    fig.update_xaxes(title_text="Date", row=row, col=col)
    fig.update_yaxes(title_text="Equity", row=row, col=col)


def _render_drawdown(
    fig: go.Figure,
    row: int,
    col: int,
    returns: pd.Series,
    theme_config: dict[str, Any],
    **kwargs: Any,
) -> None:
    """
    Render drawdown chart.
    """
    drawdown = _calculate_drawdown(returns)
    neg_color = theme_config["colors"]["negative"]

    fig.add_trace(
        go.Scatter(
            x=drawdown.index,
            y=drawdown.values,
            mode="lines",
            name="Drawdown",
            fill="tozeroy",
            line={"color": neg_color, "width": 1},
            fillcolor=_hex_to_rgba(neg_color, 0.3),  # 30% opacity
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
    **kwargs: Any,
) -> None:
    """
    Render monthly returns heatmap.
    """
    if returns.empty:
        return

    monthly = returns.resample("ME").apply(lambda x: (1 + x).prod() - 1) * 100
    monthly_pivot = pd.DataFrame(
        {
            "Year": monthly.index.year,
            "Month": monthly.index.month,
            "Return": monthly.to_numpy(),
        },
    )
    heatmap_data = monthly_pivot.pivot_table(index="Year", columns="Month", values="Return")

    month_names = [
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

    fig.add_trace(
        go.Heatmap(
            z=heatmap_data.values,
            x=[month_names[int(m) - 1] for m in heatmap_data.columns],
            y=heatmap_data.index.astype(str),
            colorscale="RdYlGn",
            zmid=0,
            text=heatmap_data.values,
            texttemplate="%{text:.1f}%",
            textfont={"size": 10},
            showscale=False,
        ),
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
    **kwargs: Any,
) -> None:
    """
    Render returns distribution histogram.
    """
    if returns.empty:
        return

    fig.add_trace(
        go.Histogram(
            x=returns.to_numpy() * 100,
            nbinsx=50,
            name="Returns",
            marker={"color": theme_config["colors"]["primary"]},
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
    **kwargs: Any,
) -> None:
    """
    Render rolling Sharpe ratio.
    """
    if returns.empty or len(returns) < window:
        return

    rolling_mean = returns.rolling(window=window).mean()
    rolling_std = returns.rolling(window=window).std()
    rolling_sharpe = (rolling_mean / rolling_std.replace(0, float("nan"))) * (
        TRADING_DAYS_PER_YEAR**0.5
    )

    fig.add_trace(
        go.Scatter(
            x=rolling_sharpe.index,
            y=rolling_sharpe.values,
            mode="lines",
            name="Rolling Sharpe",
            line={"color": theme_config["colors"]["positive"], "width": 2},
            showlegend=False,
        ),
        row=row,
        col=col,
    )

    # Add horizontal line at y=0
    # Note: We can't use fig.add_hline with subplots, need to use shapes
    # This will be handled in the main function

    fig.update_xaxes(title_text="Date", row=row, col=col)
    fig.update_yaxes(title_text="Sharpe Ratio", row=row, col=col)


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
        return

    yearly = returns.resample("YE").apply(lambda x: (1 + x).prod() - 1) * 100
    colors = [
        theme_config["colors"]["positive"] if r >= 0 else theme_config["colors"]["negative"]
        for r in yearly.to_numpy()
    ]

    fig.add_trace(
        go.Bar(
            x=yearly.index.year,
            y=yearly.to_numpy(),
            marker={"color": colors},
            showlegend=False,
        ),
        row=row,
        col=col,
    )

    fig.update_xaxes(title_text="Year", row=row, col=col)
    fig.update_yaxes(title_text="Return (%)", row=row, col=col)


# Chart specifications for tearsheet integration
_TEARSHEET_CHART_SPECS: dict[str, dict[str, Any]] = {}


def _register_tearsheet_chart(
    name: str,
    subplot_type: str,
    title: str,
    renderer: Callable,
) -> None:
    """
    Register a chart for tearsheet rendering.

    Parameters
    ----------
    name : str
        Chart name (used in config.charts).
    subplot_type : str
        Plotly subplot type ('scatter', 'bar', 'table', 'heatmap', 'histogram').
    title : str
        Display title for the subplot.
    renderer : Callable
        Function that adds traces to the figure. Signature:
        renderer(fig, row, col, returns, stats_pnls, stats_returns, stats_general, theme_config, benchmark_returns, benchmark_name, **kwargs)

    """
    _TEARSHEET_CHART_SPECS[name] = {
        "type": subplot_type,
        "title": title,
        "renderer": renderer,
    }


def _calculate_grid_layout(
    charts: list[str],
    custom_layout: Any = None,
) -> tuple[int, int, list, list[str], list[float], float, float]:
    """
    Calculate dynamic grid layout based on selected charts.

    Parameters
    ----------
    charts : list[str]
        List of chart names to include.
    custom_layout : GridLayout, optional
        Custom layout specification.

    Returns
    -------
    tuple
        (rows, cols, specs, subplot_titles, row_heights, vertical_spacing, horizontal_spacing)

    """
    if custom_layout is not None:
        # Use custom layout if provided
        # Build specs based on charts
        rows = custom_layout.rows
        cols = custom_layout.cols
        heights = custom_layout.heights
        v_spacing = custom_layout.vertical_spacing
        h_spacing = custom_layout.horizontal_spacing
    else:
        # Auto-calculate layout based on number of charts
        num_charts = len(charts)
        if num_charts <= 2:
            rows, cols = 1, num_charts
            heights = [1.0 / rows] * rows
        elif num_charts <= 4:
            rows, cols = 2, 2
            heights = [1.0 / rows] * rows
        elif num_charts <= 6:
            rows, cols = 3, 2
            heights = [1.0 / rows] * rows
        else:
            # Use default 4-row layout with custom heights for tables at top
            rows, cols = 4, 2
            heights = [0.50, 0.22, 0.16, 0.12]

        v_spacing = 0.10
        h_spacing = 0.10

    # Build specs and titles for each chart
    specs = []
    titles = []
    chart_idx = 0

    for _ in range(rows):
        row_specs: list[dict[str, Any] | None] = []

        for _ in range(cols):
            if chart_idx < len(charts):
                chart_name = charts[chart_idx]
                spec = _TEARSHEET_CHART_SPECS.get(chart_name, {})
                subplot_type = spec.get("type", "scatter")
                title = spec.get("title", chart_name.replace("_", " ").title())

                row_specs.append({"type": subplot_type})
                titles.append(title)
                chart_idx += 1
            else:
                row_specs.append(None)
                titles.append("")
        specs.append(row_specs)

    return rows, cols, specs, titles, heights, v_spacing, h_spacing


# Register built-in chart functions (for standalone use)
register_chart("equity", create_equity_curve)
register_chart("drawdown", create_drawdown_chart)
register_chart("monthly_returns", create_monthly_returns_heatmap)
register_chart("distribution", create_returns_distribution)
register_chart("rolling_sharpe", create_rolling_sharpe)
register_chart("yearly_returns", create_yearly_returns)

# Register built-in charts for tearsheet integration
_register_tearsheet_chart("run_info", "table", "Run Information", _render_run_info)
_register_tearsheet_chart("stats_table", "table", "Performance Statistics", _render_stats_table)
_register_tearsheet_chart("equity", "scatter", "Equity Curve", _render_equity)
_register_tearsheet_chart("drawdown", "scatter", "Drawdown", _render_drawdown)
_register_tearsheet_chart("monthly_returns", "heatmap", "Monthly Returns", _render_monthly_returns)
_register_tearsheet_chart("distribution", "histogram", "Returns Distribution", _render_distribution)
_register_tearsheet_chart(
    "rolling_sharpe", "scatter", "Rolling Sharpe Ratio (60-day)", _render_rolling_sharpe
)
_register_tearsheet_chart("yearly_returns", "bar", "Yearly Returns", _render_yearly_returns)
