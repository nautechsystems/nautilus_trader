# Visualization

NautilusTrader provides interactive HTML tearsheets for analyzing backtest results through
an extensible visualization system built on Plotly. The system emphasizes configurability
and extensibility, allowing you to generate comprehensive performance reports with minimal
code while maintaining the flexibility to add custom charts and themes.

## Overview

The visualization system is built on three core pillars:

1. **Chart Registry** - Decoupled chart definitions that can be extended with custom visualizations.
2. **Theme System** - Consistent styling with built-in and custom themes.
3. **Configuration** - Declarative specification of what to render and how to display it.

All visualization outputs are self-contained HTML files that can be viewed in any modern
browser, shared with stakeholders, or archived for future reference.

:::note
The visualization system requires `plotly>=6.3.1`. Install it with:

```bash
uv pip install "nautilus_trader[visualization]"
```

or

```bash
uv pip install "plotly>=6.3.1"
```

:::

## Tearsheets

A tearsheet is a comprehensive performance report that combines multiple charts and
statistics into a single interactive visualization. Tearsheets are generated after
completing a backtest run and provide immediate visual feedback on strategy performance.

### Quick start

Generate a tearsheet with default settings:

```python
from nautilus_trader.backtest.engine import BacktestEngine

# After running your backtest
engine.run()

# Generate tearsheet
from nautilus_trader.analysis.tearsheet import create_tearsheet

create_tearsheet(
    engine=engine,
    output_path="backtest_results.html",
)
```

This produces an HTML file with all default charts, using the light theme and automatic
layout. Open `backtest_results.html` in your browser to view the interactive tearsheet.

### Customization

Control which charts appear and how they're styled:

```python
from nautilus_trader.analysis import TearsheetConfig

config = TearsheetConfig(
    charts=["run_info", "stats_table", "equity", "drawdown"],
    theme="nautilus_dark",
    height=2000,
)

create_tearsheet(
    engine=engine,
    output_path="custom_tearsheet.html",
    config=config,
)
```

### Currency filtering

For multi-currency backtests, filter statistics to a specific currency:

```python
from nautilus_trader.model.currencies import USD

create_tearsheet(
    engine=engine,
    output_path="usd_only.html",
    currency=USD,  # Show only USD statistics
)
```

When `currency` is `None` (default), all currencies are displayed in the tearsheet.

## Available charts

The tearsheet can include any combination of the following built-in charts:

| Chart Name         | Type      | Description                                              |
|--------------------|-----------|----------------------------------------------------------|
| `run_info`         | Table     | Run metadata and account balances.                       |
| `stats_table`      | Table     | Performance statistics (PnL, returns, general metrics).  |
| `equity`           | Line      | Cumulative returns over time with optional benchmark.    |
| `drawdown`         | Area      | Drawdown percentage from peak equity.                    |
| `monthly_returns`  | Heatmap   | Monthly return percentages organized by year.            |
| `distribution`     | Histogram | Distribution of individual return values.                |
| `rolling_sharpe`   | Line      | 60-day rolling Sharpe ratio.                             |
| `yearly_returns`   | Bar       | Annual return percentages.                               |

All charts are registered in the chart registry and can be referenced by name in
`TearsheetConfig.charts`.

### Run information table

The `run_info` chart displays critical metadata about the backtest run:

- Run ID, start time, finish time
- Backtest period (start/end dates)
- Total iterations processed
- Event, order, and position counts
- Account starting and ending balances (per currency)

This table appears in the top-left position by default.

### Performance statistics table

The `stats_table` chart displays comprehensive performance metrics organized into sections:

- **PnL Statistics** (per currency): Total PnL, win rate, profit factor, etc.
- **Returns Statistics**: Sharpe ratio, Sortino ratio, max drawdown, etc.
- **General Statistics**: Total trades, average trade duration, etc.

This table appears in the top-right position by default.

### Equity curve

The `equity` chart plots cumulative returns over the backtest period. When `benchmark_returns`
is provided to `create_tearsheet()`, the benchmark is overlaid for comparison.

```python
import pandas as pd

# Load benchmark returns (e.g., from a market index)
benchmark_returns = pd.read_csv("sp500_returns.csv", index_col=0)["return"]

create_tearsheet(
    engine=engine,
    output_path="with_benchmark.html",
    benchmark_returns=benchmark_returns,
    benchmark_name="S&P 500",
)
```

## Themes

Themes control the visual styling of charts including colors, fonts, and backgrounds.
NautilusTrader provides four built-in themes:

| Theme Name      | Description                                    | Use Case                      |
|-----------------|------------------------------------------------|-------------------------------|
| `plotly_white`  | Clean light theme with dark gray headers.      | Default, professional reports.|
| `plotly_dark`   | Dark background with standard Plotly colors.   | Low-light environments.       |
| `nautilus`      | Light theme with NautilusTrader brand colors.  | Official light mode.          |
| `nautilus_dark` | Dark theme with teal/cyan signature colors.    | Official dark mode.           |

### Selecting a theme

Specify the theme in `TearsheetConfig`:

```python
config = TearsheetConfig(theme="nautilus_dark")
create_tearsheet(engine=engine, config=config)
```

### Custom themes

Register a custom theme for consistent branding across all visualizations:

```python
from nautilus_trader.analysis.themes import register_theme

register_theme(
    name="corporate",
    template="plotly_white",  # Base Plotly template
    colors={
        "primary": "#003366",      # Navy blue
        "positive": "#2e8b57",     # Sea green
        "negative": "#c41e3a",     # Cardinal red
        "neutral": "#808080",      # Gray
        "background": "#ffffff",   # White
        "grid": "#e5e5e5",         # Light gray
        # Optional table colors (defaults will be provided if omitted)
        "table_section": "#e5e5e5",
        "table_row_odd": "#f8f8f8",
        "table_row_even": "#ffffff",
        "table_text": "#000000",
    }
)

# Use the custom theme
config = TearsheetConfig(theme="corporate")
```

The theme system automatically provides sensible defaults for `table_*` colors based on
the `background` and `grid` colors, ensuring backward compatibility with themes registered
before table-specific colors were introduced.

## Configuration

The `TearsheetConfig` class provides declarative control over tearsheet generation:

```python
from nautilus_trader.analysis import TearsheetConfig, GridLayout

config = TearsheetConfig(
    charts=["equity", "drawdown", "stats_table"],
    theme="nautilus_dark",
    title="Q4 2024 Strategy Performance",
    height=1800,
    include_benchmark=True,
    benchmark_name="SPY",
    layout=GridLayout(
        rows=2,
        cols=2,
        heights=[0.60, 0.40],
        vertical_spacing=0.08,
        horizontal_spacing=0.12,
    ),
)
```

### Configuration parameters

| Parameter           | Type            | Default                           | Description                                   |
|---------------------|-----------------|-----------------------------------|-----------------------------------------------|
| `charts`            | `list[str]`     | All built-in charts               | List of chart names to include.               |
| `theme`             | `str`           | `"plotly_white"`                  | Theme name for styling.                       |
| `layout`            | `GridLayout`    | `None` (auto-calculated)          | Custom subplot grid layout.                   |
| `title`             | `str`           | Auto-generated with strategy/time | Tearsheet title.                              |
| `include_benchmark` | `bool`          | `True`                            | Show benchmark when provided.                 |
| `benchmark_name`    | `str`           | `"Benchmark"`                     | Display name for benchmark.                   |
| `height`            | `int`           | `1500`                            | Total height in pixels.                       |
| `show_logo`         | `bool`          | `True`                            | Display NautilusTrader logo (not implemented).|

When `layout` is `None`, the grid dimensions and row heights are automatically calculated
based on the number of charts. For 8 charts (the default), a 4×2 grid is used with
heights `[0.50, 0.22, 0.16, 0.12]` to give more space to the top row tables.

## Custom charts

The registry pattern makes adding custom charts straightforward. Charts are functions that
render traces onto a Plotly figure object.

### Registering a custom chart

```python
from nautilus_trader.analysis.tearsheet import register_chart
import plotly.graph_objects as go

def my_custom_chart(returns, output_path=None, title="Custom Chart", theme="plotly_white"):
    """
    Create a custom visualization.

    This function signature matches the built-in chart functions for consistency.
    """
    from nautilus_trader.analysis.themes import get_theme

    theme_config = get_theme(theme)

    # Create your visualization
    fig = go.Figure()
    fig.add_trace(go.Scatter(
        x=returns.index,
        y=returns.cumsum(),
        mode="lines",
        name="Custom Metric",
        line={"color": theme_config["colors"]["primary"]},
    ))

    fig.update_layout(
        title=title,
        template=theme_config["template"],
        xaxis_title="Date",
        yaxis_title="Value",
    )

    if output_path:
        fig.write_html(output_path)

    return fig

# Register the chart for use in tearsheets
register_chart("my_custom", my_custom_chart)

# Include it in tearsheet config
config = TearsheetConfig(
    charts=["stats_table", "equity", "my_custom"],
)
```

### Tearsheet integration

For full tearsheet integration with proper grid placement, use the lower-level registration:

```python
from nautilus_trader.analysis.tearsheet import _register_tearsheet_chart

def _render_my_metric(fig, row, col, returns, theme_config, **kwargs):
    """
    Render custom metric directly onto a subplot.

    Parameters
    ----------
    fig : go.Figure
        The figure to add traces to.
    row : int
        Subplot row position.
    col : int
        Subplot column position.
    returns : pd.Series
        Strategy returns from analyzer.
    theme_config : dict
        Theme configuration dictionary.
    **kwargs : dict
        Additional parameters (stats_pnls, stats_returns, benchmark_returns, etc.).
    """
    metric_values = returns.rolling(30).std() * 100  # Example metric

    fig.add_trace(
        go.Scatter(
            x=returns.index,
            y=metric_values,
            mode="lines",
            name="30-Day Volatility",
            line={"color": theme_config["colors"]["neutral"]},
        ),
        row=row,
        col=col,
    )

    fig.update_xaxes(title_text="Date", row=row, col=col)
    fig.update_yaxes(title_text="Volatility (%)", row=row, col=col)

# Register for tearsheet use
_register_tearsheet_chart(
    name="volatility",
    subplot_type="scatter",
    title="Rolling Volatility (30-day)",
    renderer=_render_my_metric,
)

# Now "volatility" can be used in TearsheetConfig.charts
```

The renderer function receives all necessary data (returns, statistics, theme configuration)
and renders directly onto the specified subplot position.

## Offline analysis

For situations where you have precomputed statistics but not a `BacktestEngine` instance,
use the lower-level API:

```python
from nautilus_trader.analysis.tearsheet import create_tearsheet_from_stats

# Load precomputed data
stats_pnls = {"USD": {...}}  # Per-currency PnL statistics
stats_returns = {...}         # Returns-based statistics
stats_general = {...}         # General statistics
returns = pd.Series(...)      # Returns series

create_tearsheet_from_stats(
    stats_pnls=stats_pnls,
    stats_returns=stats_returns,
    stats_general=stats_general,
    returns=returns,
    output_path="offline_analysis.html",
)
```

This approach is useful for:

- Analyzing results from multiple backtest runs stored separately.
- Comparing strategies using precomputed metrics.
- Integrating with external analysis pipelines.

## Best practices

### Chart selection

- Use default charts for exploratory analysis to see all available metrics.
- Customize charts when you know which metrics matter for your strategy.
- Remove irrelevant charts to reduce visual clutter and file size.

### Theme usage

- Use `plotly_white` for professional reports and presentations.
- Use `nautilus_dark` for official materials or low-light viewing.
- Create custom themes to match internal guidelines or personal preferences.

### Performance considerations

- Tearsheet HTML files contain all data inline and can be several megabytes for long backtests.
- Consider generating separate tearsheets for different analysis timeframes.
- For very large datasets, use the individual chart functions instead of full tearsheets.

### Custom statistics integration

Custom charts work best when paired with [custom statistics](reports.md) registered in the
`PortfolioAnalyzer`. This ensures your visualizations display metrics computed consistently
with the rest of the system:

```python
from nautilus_trader.analysis.statistic import PortfolioStatistic

class MyCustomStatistic(PortfolioStatistic):
    """Custom metric for specialized strategy analysis."""

    def calculate_from_returns(self, returns):
        # Your calculation logic
        return custom_metric_value

# Register with analyzer
analyzer.register_statistic(MyCustomStatistic())

# Now available in stats_returns for custom charts
```

## API levels

The visualization system provides two API levels:

### High-level API

Recommended for most use cases:

```python
create_tearsheet(engine=engine, config=config)
```

Automatically extracts data from the `BacktestEngine`, generates all configured charts,
and produces a complete HTML tearsheet.

### Low-level API

For advanced customization or offline analysis:

```python
create_tearsheet_from_stats(
    stats_pnls=stats_pnls,
    stats_returns=stats_returns,
    stats_general=stats_general,
    returns=returns,
    run_info=run_info,
    account_info=account_info,
    config=config,
)
```

Provides fine-grained control over data inputs and allows analysis of precomputed statistics.

Individual chart functions (`create_equity_curve`, `create_drawdown_chart`, etc.) offer
the most control for creating standalone visualizations outside the tearsheet framework.

## Related guides

- [Backtesting](backtesting.md) - Learn how to run backtests that generate tearsheets.
- [Reports](reports.md) - Understand the underlying statistics displayed in tearsheets.
- [Portfolio](portfolio.md) - Explore portfolio tracking and performance metrics.
