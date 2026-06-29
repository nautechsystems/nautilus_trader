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

import numbers
from collections.abc import Callable
from collections.abc import Mapping
from difflib import get_close_matches
from pathlib import Path
from typing import TYPE_CHECKING
from typing import Any

from nautilus_trader.analysis.config import TearsheetChart
from nautilus_trader.analysis.config import TearsheetConfig
from nautilus_trader.core import NAUTILUS_VERSION
from nautilus_trader.core import unix_nanos_to_iso8601
from nautilus_trader.model import AggregationSource
from nautilus_trader.model import BarType


if TYPE_CHECKING:
    import pandas as pd

    from nautilus_trader.backtest import BacktestEngine


def _require_pandas():
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

_STATIC_IMAGE_SUFFIXES = frozenset({".png", ".jpg", ".jpeg", ".webp", ".svg", ".pdf"})

_CHART_REGISTRY: dict[str, Callable] = {}


def _require_not_none(value: Any, name: str) -> None:
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


def _to_returns_series(returns) -> pd.Series:
    pandas = _require_pandas()
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
    suffix = Path(output_path).suffix.lower()
    if suffix in _STATIC_IMAGE_SUFFIXES:
        # Kaleido serializes via orjson, which cannot encode the pandas Timestamp
        # values on datetime axes; a round-trip through plotly's JSON encoder
        # converts them so the resulting trace data is serializable.
        pio.from_json(pio.to_json(fig)).write_image(output_path)
    else:
        fig.write_html(output_path)


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

    if "table_section" not in colors:
        colors["table_section"] = colors.get("grid", "#e0e0e0")
    if "table_row_odd" not in colors:
        colors["table_row_odd"] = colors.get("grid", "#f0f0f0")
    if "table_row_even" not in colors:
        colors["table_row_even"] = colors.get("background", "#ffffff")
    if "table_text" not in colors:
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
    >>> register_chart("another_chart", my_chart_function)

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
    engine: BacktestEngine,
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
        Path to save the tearsheet. File extension selects the format:
        ``.html`` (interactive), or ``.png``, ``.jpg``, ``.webp``, ``.svg``, ``.pdf``
        (static, via Kaleido). If None, returns HTML string.
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

    _require_not_none(engine, "engine")

    result = engine.get_result()
    stats_returns = dict(result.stats_returns)
    stats_general = dict(result.stats_general)
    stats_pnls = dict(result.stats_pnls)
    returns = _resolve_tearsheet_returns(engine=engine, currency=currency)

    if currency is not None:
        currency_code = getattr(currency, "code", str(currency))
        stats_pnls = (
            {currency_code: stats_pnls[currency_code]} if currency_code in stats_pnls else {}
        )

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
        "Iterations": f"{result.iterations:_}",
        "Total events": f"{result.total_events:_}",
        "Total orders": f"{result.total_orders:_}",
        "Total positions": f"{result.total_positions:_}",
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


def _resolve_tearsheet_returns(
    engine: BacktestEngine,
    currency=None,
) -> pd.Series:
    """
    Pick the best available returns series for the tearsheet.

    v2 exposes statistics snapshots rather than the mutable analyzer behind the
    portfolio, so this reconstructs daily account returns from public account reports.

    """
    account_returns = _calculate_account_returns(engine=engine, currency=currency)
    if account_returns is not None and not account_returns.empty:
        return account_returns

    return pd.Series(dtype=float)


def _calculate_account_returns(
    engine: BacktestEngine,
    currency=None,
) -> pd.Series | None:
    """
    Compute daily portfolio returns by aggregating v2 account reports.

    Returns ``None`` when accounts use mixed currencies without an explicit
    ``currency`` filter.

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
    if combined.empty:
        return None

    return _calculate_daily_balance_returns(combined.sum(axis=1))


def _collect_account_info(engine: BacktestEngine, currency=None) -> dict[str, str]:
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
    config=None,
    benchmark_returns: pd.Series | None = None,
    benchmark_name: str = "Benchmark",
    run_info: dict[str, Any] | None = None,
    account_info: dict[str, Any] | None = None,
    engine=None,
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
    >>> stats_returns = {"Sharpe Ratio (252 days)": 1.5, ...}
    >>> stats_general = {"Win Rate": 0.55, ...}
    >>> stats_pnls = {"PnL (total)": 10000.0, ...}
    >>> returns = pd.Series([0.01, -0.02, ...])
    >>> html = create_tearsheet_from_stats(
    ...     stats_pnls,
    ...     stats_returns,
    ...     stats_general,
    ...     returns,
    ...     output_path=None,  # Return HTML instead of saving
    ... )

    """
    if not PLOTLY_AVAILABLE:
        msg = (
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]"
        )
        raise ImportError(msg)

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

    returns = _to_returns_series(returns)
    benchmark_returns = (
        _to_returns_series(benchmark_returns) if benchmark_returns is not None else None
    )

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
        msg = (
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]"
        )
        raise ImportError(msg)

    from nautilus_trader.analysis.themes import get_theme

    returns = _to_returns_series(returns)
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
            fillcolor=_hex_to_rgba(neg_color, 0.3),
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
        _write_figure(fig, output_path)

    return fig


def create_monthly_returns_heatmap(
    returns: pd.Series,
    output_path: str | None = None,
    title: str | None = None,
    compounding: bool = True,
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

    if title is None:
        title = "Monthly Returns (%)" if compounding else "Monthly Returns (% of initial capital)"

    returns = _to_returns_series(returns)
    if returns.empty:
        fig = go.Figure()
        fig.update_layout(title=title)
        return fig

    monthly = _aggregate_period_returns(returns, "ME", compounding)

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
        _write_figure(fig, output_path)

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
        Returns series.
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

    returns = _to_returns_series(returns)
    fig = go.Figure()
    fig.add_trace(
        go.Histogram(
            x=returns.to_numpy() * 100,
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
        _write_figure(fig, output_path)

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
        Returns series.
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

    returns = _to_returns_series(returns)
    if returns.empty or len(returns) < window:
        fig = go.Figure()
        fig.update_layout(title=title)
        return fig

    # Sharpe = (mean / std) * sqrt(TRADING_DAYS_PER_YEAR) annualized
    rolling_mean = returns.rolling(window=window).mean()
    rolling_std = returns.rolling(window=window).std()

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
        _write_figure(fig, output_path)

    return fig


def create_yearly_returns(
    returns: pd.Series,
    output_path: str | None = None,
    title: str | None = None,
    compounding: bool = True,
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

    if title is None:
        title = "Yearly Returns" if compounding else "Yearly Returns (% of initial capital)"

    returns = _to_returns_series(returns)
    if returns.empty:
        fig = go.Figure()
        fig.update_layout(title=title)
        return fig

    yearly = _aggregate_period_returns(returns, "YE", compounding)

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
        _write_figure(fig, output_path)

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
    engine=None,
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
        msg = (
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]"
        )
        raise ImportError(msg)

    from nautilus_trader.analysis.themes import get_theme

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

    fig.update_layout(
        title_text=config.title if config.title != "NautilusTrader Backtest Results" else title,
        title_font_size=20,
        template=theme_config["template"],
        height=config.height,
        showlegend=benchmark_returns is not None,
        margin={"t": 150, "b": 50, "l": 50, "r": 50},
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
    if theme_config is None:
        from nautilus_trader.analysis.themes import get_theme

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
            formatted_value = (
                f"{value:.4f}"
                if isinstance(value, numbers.Real) and not isinstance(value, bool)
                else str(value)
            )
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

        metrics.append(f"<b>{title}</b>")
        values.append("")
        fill_colors.append(theme_config["colors"]["table_section"])

        for key, value in section_data.items():
            metrics.append(key)
            values.append(str(value))

            if len(fill_colors) % 2 == 0:
                fill_colors.append(theme_config["colors"]["table_row_odd"])
            else:
                fill_colors.append(theme_config["colors"]["table_row_even"])

    if run_info:
        add_section("Run Information", run_info)
    if account_info:
        add_section("Account Summary", account_info)

    if not metrics:
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
            fillcolor=_hex_to_rgba(neg_color, 0.3),
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

    monthly = _aggregate_period_returns(returns, "ME", kwargs.get("compounding", True))
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

    yearly = _aggregate_period_returns(returns, "YE", kwargs.get("compounding", True))
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
        msg = (
            "plotly is required for visualization. "
            "Install it with: pip install nautilus_trader[visualization]"
        )
        raise ImportError(msg)

    from nautilus_trader.analysis.themes import get_theme

    _require_not_none(engine, "engine")
    _require_not_none(bar_type, "bar_type")

    theme_config = _normalize_theme_config(get_theme(theme))

    bars = engine.cache.bars(bar_type)
    if not bars:
        fig = go.Figure()
        fig.update_layout(title=title or f"{bar_type}")
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
        title=title,
        theme_config=theme_config,
    )

    fig.update_layout(
        title=title or f"{bar_type} - Bars with Order Fills",
        yaxis_title="Price",
        template=theme_config["template"],
        height=800,
        showlegend=True,
        xaxis1={
            "rangeslider": {
                "visible": True,
            },
        },
    )

    fig.update_yaxes(fixedrange=False, row=1, col=1)

    if output_path:
        _write_figure(fig, output_path)

    return fig


def _render_bars_with_fills(  # noqa: C901
    fig: go.Figure,
    row: int,
    col: int,
    engine=None,
    bar_type=None,
    title: str | None = None,
    theme_config: dict[str, Any] | None = None,
    **kwargs: Any,
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
    title : str, optional
        Chart title override.
    theme_config : dict[str, Any], optional
        Theme configuration dictionary. If None, defaults to plotly_white theme.
    **kwargs : Any
        Additional keyword arguments (ignored).

    """
    if engine is None:
        return

    if theme_config is None:
        from nautilus_trader.analysis.themes import get_theme

        theme_config = _normalize_theme_config(get_theme("plotly_white"))

    if bar_type is None:
        bar_types = [
            *engine.cache.bar_types(aggregation_source=AggregationSource.EXTERNAL),
            *engine.cache.bar_types(aggregation_source=AggregationSource.INTERNAL),
        ]

        if not bar_types:
            return

        bar_type = bar_types[0]

    if isinstance(bar_type, str):
        bar_type = BarType.from_str(bar_type)

    _require_not_none(engine, "engine")
    _require_not_none(bar_type, "bar_type")

    bars = engine.cache.bars(bar_type)
    if not bars:
        return

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

        positive_color = theme_config["colors"]["positive"]
        negative_color = theme_config["colors"]["negative"]

        _add_fill_scatter_trace(
            fig=fig,
            fills_df=buy_fills,
            row=row,
            col=col,
            marker_symbol="triangle-up",
            marker_color=_hex_to_rgba(positive_color, 0.7),
            name="Buy Fills",
        )

        _add_fill_scatter_trace(
            fig=fig,
            fills_df=sell_fills,
            row=row,
            col=col,
            marker_symbol="triangle-down",
            marker_color=_hex_to_rgba(negative_color, 0.7),
            name="Sell Fills",
        )

    fig.update_xaxes(
        title_text="Time",
        row=row,
        col=col,
        rangeslider={"visible": True},
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
            marker_size=13,
            marker_line_width=2,
            marker_line_color="rgba(0,0,0,0.7)",
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
    custom_layout: Any = None,
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
        return 1, 1, [[{"type": "scatter"}]], [""], [1.0], 0.10, 0.10

    if custom_layout is not None:
        rows = custom_layout.rows
        cols = custom_layout.cols
        heights = custom_layout.heights
        v_spacing = custom_layout.vertical_spacing
        h_spacing = custom_layout.horizontal_spacing
    else:
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
            cols = 2
            rows = max(4, (num_charts + cols - 1) // cols)
            heights = [0.50, 0.22, 0.16, 0.12] if rows == 4 else [1.0 / rows] * rows

        v_spacing = 0.10
        h_spacing = 0.10

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
