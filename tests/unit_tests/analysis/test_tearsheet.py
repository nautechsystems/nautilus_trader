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

import numpy as np
import pandas as pd
import plotly.graph_objects as go
import pytest

from nautilus_trader.analysis.config import GridLayout
from nautilus_trader.analysis.config import TearsheetConfig
from nautilus_trader.analysis.tearsheet import PLOTLY_AVAILABLE
from nautilus_trader.analysis.tearsheet import _create_stats_table
from nautilus_trader.analysis.tearsheet import _create_tearsheet_figure
from nautilus_trader.analysis.tearsheet import _normalize_theme_config
from nautilus_trader.analysis.tearsheet import create_drawdown_chart
from nautilus_trader.analysis.tearsheet import create_equity_curve
from nautilus_trader.analysis.tearsheet import create_monthly_returns_heatmap
from nautilus_trader.analysis.tearsheet import create_returns_distribution
from nautilus_trader.analysis.tearsheet import create_tearsheet
from nautilus_trader.analysis.tearsheet import create_tearsheet_from_stats
from nautilus_trader.analysis.tearsheet import get_chart
from nautilus_trader.analysis.tearsheet import list_charts
from nautilus_trader.analysis.tearsheet import register_chart
from nautilus_trader.analysis.themes import get_theme
from nautilus_trader.analysis.themes import list_themes
from nautilus_trader.analysis.themes import register_theme
from nautilus_trader.model.currencies import EUR
from nautilus_trader.model.currencies import USD


# Skip all tests if plotly is not installed
pytestmark = pytest.mark.skipif(
    not PLOTLY_AVAILABLE,
    reason="plotly not installed",
)


@pytest.fixture
def sample_returns():
    """
    Create sample returns series for testing.
    """
    dates = pd.date_range("2024-01-01", "2024-03-31", freq="D")
    rng = np.random.default_rng(42)
    returns = pd.Series(
        rng.normal(0.001, 0.02, len(dates)),
        index=dates,
    )
    return returns


@pytest.fixture
def sample_stats():
    """
    Create sample statistics for testing.
    """
    return {
        "Sharpe Ratio (252 days)": 1.5,
        "Sortino Ratio (252 days)": 2.0,
        "Max Drawdown": -0.15,
        "Win Rate": 0.55,
        "Total Return": 0.25,
    }


def test_create_equity_curve_with_valid_data(sample_returns, tmp_path):
    # Arrange
    output_path = tmp_path / "equity.html"

    # Act
    fig = create_equity_curve(
        returns=sample_returns,
        output_path=str(output_path),
        title="Test Equity Curve",
    )

    # Assert
    assert fig is not None
    assert output_path.exists()
    assert "Test Equity Curve" in fig.layout.title.text


def test_create_equity_curve_without_output_path(sample_returns):
    # Arrange & Act
    fig = create_equity_curve(
        returns=sample_returns,
        title="Test Equity Curve",
    )

    # Assert
    assert fig is not None
    assert "Test Equity Curve" in fig.layout.title.text


def test_create_equity_curve_with_empty_returns():
    # Arrange
    empty_returns = pd.Series(dtype=float)

    # Act
    fig = create_equity_curve(
        returns=empty_returns,
        title="Empty Equity Curve",
    )

    # Assert
    assert fig is not None


def test_create_drawdown_chart_with_valid_data(sample_returns, tmp_path):
    # Arrange
    output_path = tmp_path / "drawdown.html"

    # Act
    fig = create_drawdown_chart(
        returns=sample_returns,
        output_path=str(output_path),
        title="Test Drawdown",
    )

    # Assert
    assert fig is not None
    assert output_path.exists()
    assert "Test Drawdown" in fig.layout.title.text


def test_create_drawdown_chart_calculates_correctly(sample_returns):
    # Arrange & Act
    fig = create_drawdown_chart(
        returns=sample_returns,
        title="Test Drawdown",
    )

    # Assert
    assert fig is not None
    # Drawdown should be negative or zero
    y_data = fig.data[0].y
    assert all(y <= 0 for y in y_data)


def test_create_drawdown_chart_handles_initial_losses():
    # Arrange
    # Create returns that start with losses (reproducing the bug scenario)
    dates = pd.date_range("2024-01-01", periods=2, freq="D")
    returns = pd.Series([-0.4, -0.1], index=dates)

    # Act
    fig = create_drawdown_chart(
        returns=returns,
        title="Initial Losses Test",
    )

    # Assert
    assert fig is not None
    y_data = fig.data[0].y

    # The first drawdown should be -40% (not 0%)
    # With baseline of 1.0, after -40% return, equity = 0.6
    # Running max = 1.0, so drawdown = (0.6 - 1.0) / 1.0 = -40%
    assert y_data[1] < -30  # Should be around -40%

    # After second -10% loss, equity = 0.6 * 0.9 = 0.54
    # Drawdown = (0.54 - 1.0) / 1.0 = -46%
    assert y_data[2] < -40  # Should be around -46%


def test_create_monthly_returns_heatmap_with_valid_data(sample_returns, tmp_path):
    # Arrange
    output_path = tmp_path / "monthly.html"

    # Act
    fig = create_monthly_returns_heatmap(
        returns=sample_returns,
        output_path=str(output_path),
        title="Test Monthly Returns",
    )

    # Assert
    assert fig is not None
    assert output_path.exists()
    assert "Test Monthly Returns" in fig.layout.title.text


def test_create_monthly_returns_heatmap_with_empty_returns():
    # Arrange
    empty_returns = pd.Series(dtype=float)

    # Act
    fig = create_monthly_returns_heatmap(
        returns=empty_returns,
        title="Empty Heatmap",
    )

    # Assert
    assert fig is not None


def test_create_returns_distribution_with_valid_data(sample_returns, tmp_path):
    # Arrange
    output_path = tmp_path / "distribution.html"

    # Act
    fig = create_returns_distribution(
        returns=sample_returns,
        output_path=str(output_path),
        title="Test Distribution",
    )

    # Assert
    assert fig is not None
    assert output_path.exists()
    assert "Test Distribution" in fig.layout.title.text


def test_create_tearsheet_raises_import_error_when_plotly_not_available(monkeypatch):
    # Arrange
    monkeypatch.setattr("nautilus_trader.analysis.tearsheet.PLOTLY_AVAILABLE", False)

    # Create a mock engine
    class MockEngine:
        class MockPortfolio:
            class MockAnalyzer:
                def get_performance_stats_returns(self):
                    return {}

                def get_performance_stats_general(self):
                    return {}

                def returns(self):
                    return pd.Series(dtype=float)

            analyzer = MockAnalyzer()

        portfolio = MockPortfolio()

    mock_engine = MockEngine()

    # Act & Assert
    with pytest.raises(ImportError, match="plotly is required"):
        create_tearsheet(mock_engine)


def test_create_stats_table(sample_stats):
    # Arrange
    # Split sample stats into categories
    stats_pnls = {}
    stats_returns = {
        k: v
        for k, v in sample_stats.items()
        if k
        in [
            "Sharpe Ratio (252 days)",
            "Sortino Ratio (252 days)",
            "Max Drawdown",
            "Total Return",
        ]
    }
    stats_general = {k: v for k, v in sample_stats.items() if k == "Win Rate"}

    # Act
    table = _create_stats_table(stats_pnls, stats_returns, stats_general)

    # Assert
    assert table is not None
    assert len(table.header["values"]) == 2  # Metric and Value columns
    assert len(table.cells["values"]) == 2  # Two cell columns


def test_create_stats_table_formats_numpy_floats():
    # Arrange
    stats_pnls = {"PnL": 1.23456789}
    stats_returns = {"Numpy": np.float64(1.23456789)}
    stats_general = {"Integer": 42}

    # Act
    table = _create_stats_table(stats_pnls, stats_returns, stats_general)

    # Assert
    assert table is not None

    # Check section headers are present
    metrics = table.cells["values"][0]
    assert "<b>PnL Statistics</b>" in metrics
    assert "<b>Returns Statistics</b>" in metrics
    assert "<b>General Statistics</b>" in metrics


def test_create_stats_table_handles_booleans():
    # Arrange
    stats_general = {
        "Has Trades": True,
        "Is Profitable": False,
        "Float Value": 1.2345,
    }

    # Act
    table = _create_stats_table({}, {}, stats_general)

    # Assert
    assert table is not None
    values = table.cells["values"][1]

    # Booleans should remain as True/False, not "1.0000"/"0.0000"
    assert "True" in values
    assert "False" in values
    # Float should still be formatted
    assert "1.2345" in values


def test_create_tearsheet_figure_with_valid_data(sample_returns, sample_stats):
    # Arrange & Act
    fig = _create_tearsheet_figure(
        stats_returns=sample_stats,
        stats_general={},
        stats_pnls={},
        returns=sample_returns,
        title="Test Tearsheet",
    )

    # Assert
    assert fig is not None
    assert "Test Tearsheet" in fig.layout.title.text
    # Should have multiple traces (stats table, equity, drawdown, heatmap, histogram)
    assert len(fig.data) > 1


def test_create_tearsheet_figure_includes_pnl_stats(sample_returns):
    # Arrange
    pnl_stats = {
        "PnL (total)": 12345.67,
        "PnL% (total)": 0.1234,
    }

    # Act
    fig = _create_tearsheet_figure(
        stats_returns={},
        stats_general={},
        stats_pnls=pnl_stats,
        returns=sample_returns,
        title="Test PnL Stats",
    )

    # Assert
    assert fig is not None
    # Check that PnL stats are in the table (first trace)
    table_data = fig.data[0]
    metrics = table_data.cells["values"][0]
    assert "PnL (total)" in metrics
    assert "PnL% (total)" in metrics


def test_create_tearsheet_from_stats(sample_returns, sample_stats):
    # Arrange & Act
    html = create_tearsheet_from_stats(
        stats_pnls={"PnL (total)": 10000.0},
        stats_returns=sample_stats,
        stats_general={},
        returns=sample_returns,
        output_path=None,  # Return HTML string
        title="Test Stats",
    )

    # Assert
    assert html is not None
    assert isinstance(html, str)
    assert "Test Stats" in html
    assert "plotly" in html.lower()


def test_create_tearsheet_from_stats_saves_file(sample_returns, sample_stats, tmp_path):
    # Arrange
    output_path = tmp_path / "stats_tearsheet.html"

    # Act
    result = create_tearsheet_from_stats(
        stats_pnls={"PnL (total)": 10000.0},
        stats_returns=sample_stats,
        stats_general={},
        returns=sample_returns,
        output_path=str(output_path),
        title="Test Stats",
    )

    # Assert
    assert result is None  # Returns None when writing to file
    assert output_path.exists()



def test_get_theme_with_valid_name():
    # Arrange
    # Act
    theme = get_theme("plotly_white")

    # Assert
    assert theme is not None
    assert "template" in theme
    assert "colors" in theme
    assert theme["template"] == "plotly_white"


def test_get_theme_with_invalid_name_raises_error():
    # Arrange
    # Act & Assert
    with pytest.raises(KeyError, match="Theme 'invalid_theme' not found"):
        get_theme("invalid_theme")


def test_get_theme_fuzzy_matching_suggests_similar_names():
    # Arrange
    # Act & Assert
    with pytest.raises(KeyError, match="Did you mean: plotly_white"):
        get_theme("plotly_whit")


def test_list_themes_returns_all_registered_themes():
    # Arrange
    # Act
    themes = list_themes()

    # Assert
    assert len(themes) >= 4
    assert "plotly_white" in themes
    assert "plotly_dark" in themes
    assert "nautilus" in themes
    assert "nautilus_dark" in themes


def test_register_theme_with_custom_theme():
    # Arrange
    # Act
    register_theme(
        name="test_custom",
        template="plotly_white",
        colors={
            "primary": "#123456",
            "positive": "#00ff00",
            "negative": "#ff0000",
            "neutral": "#888888",
            "background": "#ffffff",
            "grid": "#e0e0e0",
        },
    )

    # Assert
    theme = get_theme("test_custom")
    assert theme["template"] == "plotly_white"
    assert theme["colors"]["primary"] == "#123456"


def test_theme_normalization_provides_table_color_defaults():
    # Arrange
    # Create theme without table colors (simulating old custom theme)
    old_theme = {
        "template": "plotly_white",
        "colors": {
            "primary": "#123456",
            "positive": "#00ff00",
            "negative": "#ff0000",
            "neutral": "#888888",
            "background": "#ffffff",
            "grid": "#e0e0e0",
        },
    }

    # Act
    normalized = _normalize_theme_config(old_theme)

    # Assert
    assert "table_section" in normalized["colors"]
    assert "table_row_odd" in normalized["colors"]
    assert "table_row_even" in normalized["colors"]
    assert "table_text" in normalized["colors"]
    # Defaults should be based on existing colors
    assert normalized["colors"]["table_section"] == "#e0e0e0"  # From grid
    assert normalized["colors"]["table_row_even"] == "#ffffff"  # From background


def test_theme_normalization_preserves_existing_table_colors():
    # Arrange
    # Create theme with all table colors
    complete_theme = {
        "template": "plotly_white",
        "colors": {
            "primary": "#123456",
            "positive": "#00ff00",
            "negative": "#ff0000",
            "neutral": "#888888",
            "background": "#ffffff",
            "grid": "#e0e0e0",
            "table_section": "#custom1",
            "table_row_odd": "#custom2",
            "table_row_even": "#custom3",
            "table_text": "#custom4",
        },
    }

    # Act
    normalized = _normalize_theme_config(complete_theme)

    # Assert
    assert normalized["colors"]["table_section"] == "#custom1"
    assert normalized["colors"]["table_row_odd"] == "#custom2"
    assert normalized["colors"]["table_row_even"] == "#custom3"
    assert normalized["colors"]["table_text"] == "#custom4"


def test_theme_normalization_handles_dark_backgrounds():
    # Arrange
    # Create dark theme without table colors
    dark_theme = {
        "template": "plotly_dark",
        "colors": {
            "primary": "#00cfbe",
            "positive": "#2fadd7",
            "negative": "#ff6b6b",
            "neutral": "#a7aab5",
            "background": "#2a2a2d",
            "grid": "#202022",
        },
    }

    # Act
    normalized = _normalize_theme_config(dark_theme)

    # Assert
    # Dark background should get light text
    assert normalized["colors"]["table_text"] == "#eeeeee"



def test_register_chart_and_retrieve():
    # Arrange
    def custom_chart(returns, output_path=None, title="Custom", theme="plotly_white"):
        fig = go.Figure()
        fig.add_trace(go.Scatter(x=returns.index, y=returns, mode="lines"))
        fig.update_layout(title=title)
        return fig

    # Act
    register_chart("test_custom_chart", custom_chart)
    chart_func = get_chart("test_custom_chart")

    # Assert
    assert chart_func is not None
    assert callable(chart_func)


def test_get_chart_with_invalid_name_raises_error():
    # Arrange
    # Act & Assert
    with pytest.raises(KeyError, match="Chart 'nonexistent_chart' not found"):
        get_chart("nonexistent_chart")


def test_get_chart_fuzzy_matching_suggests_similar_names():
    # Arrange
    # Act & Assert
    with pytest.raises(KeyError, match="Did you mean:"):
        get_chart("equit")  # Should suggest "equity"


def test_list_charts_returns_all_registered_charts():
    # Arrange
    # Act
    charts = list_charts()

    # Assert
    # Note: run_info and stats_table are special and not in the chart registry
    assert len(charts) >= 6
    assert "equity" in charts
    assert "drawdown" in charts
    assert "monthly_returns" in charts
    assert "distribution" in charts
    assert "rolling_sharpe" in charts
    assert "yearly_returns" in charts



def test_tearsheet_config_with_defaults():
    # Arrange
    # Act
    config = TearsheetConfig()

    # Assert
    assert config.theme == "plotly_white"
    assert len(config.charts) == 8
    assert config.height == 1500
    assert config.include_benchmark is True
    assert config.benchmark_name == "Benchmark"


def test_tearsheet_config_with_custom_values():
    # Arrange
    # Act
    config = TearsheetConfig(
        charts=["equity", "drawdown"],
        theme="nautilus_dark",
        height=2000,
        title="Custom Title",
    )

    # Assert
    assert config.charts == ["equity", "drawdown"]
    assert config.theme == "nautilus_dark"
    assert config.height == 2000
    assert config.title == "Custom Title"


def test_grid_layout_with_custom_dimensions():
    # Arrange
    # Act
    layout = GridLayout(
        rows=3,
        cols=2,
        heights=[0.5, 0.3, 0.2],
        vertical_spacing=0.05,
        horizontal_spacing=0.1,
    )

    # Assert
    assert layout.rows == 3
    assert layout.cols == 2
    assert layout.heights == [0.5, 0.3, 0.2]
    assert layout.vertical_spacing == 0.05
    assert layout.horizontal_spacing == 0.1


def test_tearsheet_config_with_grid_layout():
    # Arrange
    layout = GridLayout(rows=2, cols=2, heights=[0.6, 0.4])

    # Act
    config = TearsheetConfig(
        charts=["equity", "drawdown", "monthly_returns", "distribution"],
        layout=layout,
    )

    # Assert
    assert config.layout is not None
    assert config.layout.rows == 2
    assert config.layout.cols == 2



def test_single_currency_pnl_stats(sample_returns):
    # Arrange
    stats_pnls = {
        USD: {
            "PnL (total)": 10000.0,
            "PnL% (total)": 0.10,
        },
    }

    # Act
    html = create_tearsheet_from_stats(
        stats_pnls=stats_pnls,
        stats_returns={"Sharpe Ratio (252 days)": 1.5},
        stats_general={},
        returns=sample_returns,
        output_path=None,
    )

    # Assert
    assert html is not None
    assert isinstance(html, str)


def test_multi_currency_pnl_stats(sample_returns):
    # Arrange
    stats_pnls = {
        USD: {"PnL (total)": 10000.0},
        EUR: {"PnL (total)": 8000.0},
    }

    # Act
    html = create_tearsheet_from_stats(
        stats_pnls=stats_pnls,
        stats_returns={"Sharpe Ratio (252 days)": 1.5},
        stats_general={},
        returns=sample_returns,
        output_path=None,
    )

    # Assert
    assert html is not None
    assert isinstance(html, str)



def test_run_info_filtered_when_no_metadata(sample_returns):
    # Arrange
    config = TearsheetConfig(
        charts=["run_info", "stats_table", "equity"],  # Explicitly include run_info
    )

    # Act - pass no run_info or account_info
    html = create_tearsheet_from_stats(
        stats_pnls={},
        stats_returns={"Sharpe Ratio (252 days)": 1.5},
        stats_general={},
        returns=sample_returns,
        run_info=None,
        account_info=None,
        output_path=None,
        config=config,
    )

    # Assert
    assert html is not None
    # run_info should be automatically filtered out


def test_run_info_kept_when_metadata_provided(sample_returns):
    # Arrange
    config = TearsheetConfig(
        charts=["run_info", "stats_table", "equity"],
    )

    run_info = {
        "Run ID": "BACKTEST-001",
        "Start Time": "2024-01-01 00:00:00",
    }

    # Act
    html = create_tearsheet_from_stats(
        stats_pnls={},
        stats_returns={"Sharpe Ratio (252 days)": 1.5},
        stats_general={},
        returns=sample_returns,
        run_info=run_info,
        account_info=None,
        output_path=None,
        config=config,
    )

    # Assert
    assert html is not None
    assert "Run ID" in html or "BACKTEST-001" in html


def test_run_info_kept_when_account_info_provided(sample_returns):
    # Arrange
    config = TearsheetConfig(
        charts=["run_info", "stats_table", "equity"],
    )

    account_info = {
        "Starting Balance": "1,000,000 USD",
        "Ending Balance": "1,100,000 USD",
    }

    # Act
    html = create_tearsheet_from_stats(
        stats_pnls={},
        stats_returns={"Sharpe Ratio (252 days)": 1.5},
        stats_general={},
        returns=sample_returns,
        run_info=None,
        account_info=account_info,
        output_path=None,
        config=config,
    )

    # Assert
    assert html is not None
    assert "Starting Balance" in html or "1,000,000 USD" in html



def test_tearsheet_with_benchmark_overlay(sample_returns):
    # Arrange
    # Create benchmark returns
    rng = np.random.default_rng(43)
    benchmark_returns = pd.Series(
        rng.normal(0.0005, 0.015, len(sample_returns)),
        index=sample_returns.index,
    )

    config = TearsheetConfig(
        charts=["equity"],
        include_benchmark=True,
        benchmark_name="S&P 500",
    )

    # Act
    html = create_tearsheet_from_stats(
        stats_pnls={},
        stats_returns={"Sharpe Ratio (252 days)": 1.5},
        stats_general={},
        returns=sample_returns,
        benchmark_returns=benchmark_returns,
        output_path=None,
        config=config,
    )

    # Assert
    assert html is not None
    assert "S&P 500" in html


def test_tearsheet_with_custom_grid_layout(sample_returns):
    # Arrange
    layout = GridLayout(
        rows=2,
        cols=2,
        heights=[0.6, 0.4],
        vertical_spacing=0.08,
        horizontal_spacing=0.12,
    )

    config = TearsheetConfig(
        charts=["stats_table", "equity", "drawdown", "distribution"],
        layout=layout,
    )

    # Act
    html = create_tearsheet_from_stats(
        stats_pnls={},
        stats_returns={"Sharpe Ratio (252 days)": 1.5},
        stats_general={},
        returns=sample_returns,
        output_path=None,
        config=config,
    )

    # Assert
    assert html is not None


def test_tearsheet_with_multi_currency_stats(sample_returns):
    # Arrange
    stats_pnls = {
        USD: {
            "PnL (total)": 10000.0,
            "PnL% (total)": 0.10,
            "Win Rate": 0.55,
        },
        EUR: {
            "PnL (total)": 8000.0,
            "PnL% (total)": 0.08,
            "Win Rate": 0.52,
        },
    }

    # Act
    html = create_tearsheet_from_stats(
        stats_pnls=stats_pnls,
        stats_returns={"Sharpe Ratio (252 days)": 1.5},
        stats_general={"Total Trades": 100},
        returns=sample_returns,
        output_path=None,
    )

    # Assert
    assert html is not None


def test_tearsheet_with_all_themes(sample_returns):
    # Arrange
    themes = ["plotly_white", "plotly_dark", "nautilus", "nautilus_dark"]

    # Act & Assert
    for theme in themes:
        config = TearsheetConfig(
            charts=["equity"],
            theme=theme,
        )

        html = create_tearsheet_from_stats(
            stats_pnls={},
            stats_returns={"Sharpe Ratio (252 days)": 1.5},
            stats_general={},
            returns=sample_returns,
            output_path=None,
            config=config,
        )

        assert html is not None
