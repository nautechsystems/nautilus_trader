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
Test tearsheet behavior.
"""

from __future__ import annotations

from pathlib import Path
from typing import ClassVar

import pytest

from nautilus_trader.analysis import TearsheetBarsWithFillsChart
from nautilus_trader.analysis import TearsheetConfig
from nautilus_trader.analysis import TearsheetCustomChart
from nautilus_trader.analysis import TearsheetEquityChart
from nautilus_trader.analysis import TearsheetMonthlyReturnsChart
from nautilus_trader.analysis import TearsheetStatsTableChart
from nautilus_trader.analysis import TearsheetYearlyReturnsChart
from nautilus_trader.analysis import create_bars_with_fills
from nautilus_trader.analysis import create_drawdown_chart
from nautilus_trader.analysis import create_equity_curve
from nautilus_trader.analysis import create_monthly_returns_heatmap
from nautilus_trader.analysis import create_returns_distribution
from nautilus_trader.analysis import create_rolling_sharpe
from nautilus_trader.analysis import create_tearsheet
from nautilus_trader.analysis import create_tearsheet_from_stats
from nautilus_trader.analysis import create_yearly_returns
from nautilus_trader.analysis import get_chart
from nautilus_trader.analysis import get_theme
from nautilus_trader.analysis import list_charts
from nautilus_trader.analysis import list_themes
from nautilus_trader.analysis import register_chart
from nautilus_trader.analysis import register_tearsheet_chart
from nautilus_trader.analysis import register_theme
from nautilus_trader.analysis import tearsheet
from nautilus_trader.analysis import themes
from nautilus_trader.model import AccountId
from nautilus_trader.model import Venue


pd = pytest.importorskip("pandas")


def test_tearsheet_config_defaults_and_validation() -> None:
    """
    Test tearsheet config defaults and validation.
    """
    config = TearsheetConfig()

    assert config.chart_names == [
        "run_info",
        "stats_table",
        "equity",
        "drawdown",
        "monthly_returns",
        "distribution",
        "rolling_sharpe",
        "yearly_returns",
    ]
    assert TearsheetBarsWithFillsChart(bar_type="AUD/USD.SIM-1-MINUTE-BID-INTERNAL").kwargs() == {
        "bar_type": "AUD/USD.SIM-1-MINUTE-BID-INTERNAL",
    }

    with pytest.raises(ValueError, match="height must be positive"):
        TearsheetConfig(height=0)


def test_theme_registry_returns_builtins_and_suggestions() -> None:
    """
    Test theme registry returns builtins and suggestions.
    """
    try:
        register_theme(
            name="unit_test_theme",
            template="plotly_white",
            colors={
                "primary": "#000000",
                "positive": "#00ff00",
                "negative": "#ff0000",
                "neutral": "#808080",
                "background": "#ffffff",
                "grid": "#dddddd",
            },
        )

        theme_config = get_theme("unit_test_theme")
        theme_config["colors"]["primary"] = "#111111"

        assert get_theme("unit_test_theme")["template"] == "plotly_white"
        assert get_theme("unit_test_theme")["colors"]["primary"] == "#000000"
        assert "unit_test_theme" in list_themes()

        with pytest.raises(KeyError, match="plotly_white"):
            get_theme("plotly_whit")
    finally:
        themes._THEMES.pop("unit_test_theme", None)


def test_register_theme_validates_name_and_required_colors() -> None:
    """
    Test register theme validates name and required colors.
    """
    required_colors = {
        "primary": "#000000",
        "positive": "#00ff00",
        "negative": "#ff0000",
        "neutral": "#808080",
        "background": "#ffffff",
        "grid": "#dddddd",
    }

    with pytest.raises(ValueError, match="Theme name cannot be empty"):
        register_theme("", "plotly_white", required_colors)

    with pytest.raises(ValueError, match="missing required keys"):
        register_theme("unit_test_invalid_theme", "plotly_white", {"primary": "#000000"})


def test_chart_registry_registers_direct_and_decorator_forms() -> None:
    """
    Test chart registry registers direct and decorator forms.
    """

    def chart_func(returns: object, **kwargs: object) -> object:
        """
        Chart func.
        """
        return returns, kwargs

    try:
        register_chart("unit_test_chart_direct", chart_func)

        @register_chart("unit_test_chart_decorated")
        def decorated_chart(returns: object, **kwargs: object) -> object:
            """
            Build the decorated chart.
            """
            return returns, kwargs

        assert get_chart("unit_test_chart_direct") is chart_func
        assert get_chart("unit_test_chart_decorated") is decorated_chart
        assert "unit_test_chart_direct" in list_charts()

        with pytest.raises(KeyError, match="unit_test_chart_direct"):
            get_chart("unit_test_chart_direc")
    finally:
        tearsheet._CHART_REGISTRY.pop("unit_test_chart_direct", None)
        tearsheet._CHART_REGISTRY.pop("unit_test_chart_decorated", None)


def test_create_tearsheet_uses_v2_result_and_report_api(monkeypatch: pytest.MonkeyPatch) -> None:
    """
    Test create tearsheet uses v2 result and report api.
    """
    captured = {}

    class DummyResult:
        """
        Collect dummy result tests.
        """

        stats_pnls: ClassVar[dict[str, object]] = {
            "USD": {"PnL (total)": 12.5},
            "AUD": {"PnL (total)": 1.0},
        }
        stats_returns: ClassVar[dict[str, object]] = {"Sharpe Ratio (252 days)": 1.23}
        stats_general: ClassVar[dict[str, object]] = {"Long Ratio": 0.5}
        elapsed_time_secs = 1.234
        iterations = 2
        total_events = 3
        total_orders = 4
        total_positions = 5

    class DummyCache:
        """
        Collect dummy cache tests.
        """

        @staticmethod
        def strategy_ids() -> object:
            """
            Strategy ids.
            """
            return ["S-001"]

    class DummyEngine:
        """
        Collect dummy engine tests.
        """

        run_id = "R-001"
        run_started = 1_577_836_800_000_000_000
        run_finished = 1_577_836_801_000_000_000
        backtest_start = 1_577_836_800_000_000_000
        backtest_end = 1_577_923_200_000_000_000
        cache = DummyCache()

        @staticmethod
        def get_result() -> object:
            """
            Get result.
            """
            return DummyResult()

        @staticmethod
        def list_venues() -> object:
            """
            List venues.
            """
            return ["SIM"]

        @staticmethod
        def generate_account_report(venue: Venue) -> object:
            """
            Generate account report.
            """
            return pd.DataFrame(
                {
                    "currency": ["USD", "USD"],
                    "total": ["100.0", "110.0"],
                },
                index=pd.to_datetime(["2020-01-01", "2020-01-02"], utc=True),
            )

    def capture_tearsheet_from_stats(**kwargs: object) -> str:
        """
        Capture tearsheet from stats.
        """
        captured.update(kwargs)
        return "<html></html>"

    monkeypatch.setattr(tearsheet, "PLOTLY_AVAILABLE", True)
    monkeypatch.setattr(tearsheet, "create_tearsheet_from_stats", capture_tearsheet_from_stats)

    html = create_tearsheet(DummyEngine(), output_path=None, currency="USD")

    assert html == "<html></html>"
    assert captured["stats_pnls"] == {"USD": {"PnL (total)": 12.5}}
    assert captured["stats_returns"] == {"Sharpe Ratio (252 days)": 1.23}
    assert captured["stats_general"] == {"Long Ratio": 0.5}
    assert captured["account_info"] == {
        "Starting balance (SIM USD)": "100.0",
        "Ending balance (SIM USD)": "110.0",
    }
    assert captured["run_info"]["Total orders"] == "4"
    assert captured["returns"].iloc[0] == pytest.approx(0.10)
    # "Elapsed time" is wall-clock (run_finished - run_started), distinct from the
    # simulated "Backtest range"; it must not reuse result.elapsed_time_secs (1.234).
    # Both render as human-readable durations rather than raw seconds.
    assert captured["run_info"]["Elapsed time"] == "0 days 00:00:01"
    assert captured["run_info"]["Backtest range"] == "1 days 00:00:00"


def test_create_tearsheet_accepts_backtest_result_and_node(monkeypatch: pytest.MonkeyPatch) -> None:
    """
    Test create tearsheet accepts backtest result and node.
    """
    captured = {}

    class DummyResult:
        """
        Collect dummy result tests.
        """

        run_config_id = "run-001"
        run_id = "R-001"
        run_started = 1_577_836_800_000_000_000
        run_finished = 1_577_836_801_000_000_000
        backtest_start = 1_577_836_800_000_000_000
        backtest_end = 1_577_923_200_000_000_000
        elapsed_time_secs = 86_400.0
        iterations = 2
        total_events = 3
        total_orders = 4
        total_positions = 5
        stats_pnls: ClassVar[dict[str, object]] = {
            "USD": {"PnL (total)": 12.5},
            "AUD": {"PnL (total)": 1.0},
        }
        stats_returns: ClassVar[dict[str, object]] = {"Sharpe Ratio (252 days)": 1.23}
        stats_general: ClassVar[dict[str, object]] = {"Long Ratio": 0.5}
        returns_series: ClassVar[dict[str, object]] = {
            1_577_836_800_000_000_000: 0.01,
            1_577_923_200_000_000_000: -0.02,
        }
        summary: ClassVar[dict[str, object]] = {"account.SIM.id": "SIM-001"}

    class DummyConfig:
        """
        Collect dummy config tests.
        """

        id = "run-001"
        dispose_on_completion = False

    class DummyNode:
        """
        Collect dummy node tests.
        """

        configs: ClassVar[list[object]] = [DummyConfig()]

        @staticmethod
        def get_engine_cache(run_config_id: object) -> str:
            """
            Get engine cache.
            """
            assert run_config_id == "run-001"
            return "cache"

        @staticmethod
        def generate_account_report(run_config_id: object, account_id: AccountId) -> object:
            """
            Generate account report.
            """
            assert run_config_id == "run-001"
            assert str(account_id) == "SIM-001"
            return pd.DataFrame(
                {
                    "currency": ["USD", "USD", "AUD", "AUD"],
                    "total": ["100.0", "110.0", "200.0", "220.0"],
                },
                index=pd.to_datetime(
                    ["2020-01-01", "2020-01-02", "2020-01-01", "2020-01-02"],
                    utc=True,
                ),
            )

        @staticmethod
        def generate_fills_report(run_config_id: object) -> object:
            """
            Generate fills report.
            """
            assert run_config_id == "run-001"
            return pd.DataFrame()

    def capture_tearsheet_from_stats(**kwargs: object) -> str:
        """
        Capture tearsheet from stats.
        """
        captured.update(kwargs)
        return "<html></html>"

    monkeypatch.setattr(tearsheet, "PLOTLY_AVAILABLE", True)
    monkeypatch.setattr(tearsheet, "create_tearsheet_from_stats", capture_tearsheet_from_stats)

    html = create_tearsheet(
        DummyResult(),
        node=DummyNode(),
        output_path=None,
        currency="USD",
        config=TearsheetConfig(charts=[TearsheetStatsTableChart()]),
    )

    assert html == "<html></html>"
    assert captured["stats_pnls"] == {"USD": {"PnL (total)": 12.5}}
    assert captured["account_info"] == {
        "Starting balance (SIM, USD)": "100.0",
        "Ending balance (SIM, USD)": "110.0",
    }
    assert captured["run_info"]["Elapsed time"] == "0 days 00:00:01"
    assert captured["returns"].to_dict() == {
        pd.Timestamp("2020-01-01T00:00:00Z"): 0.01,
        pd.Timestamp("2020-01-02T00:00:00Z"): -0.02,
    }
    assert captured["engine"].cache == "cache"


def test_create_tearsheet_rejects_disposed_node_state(monkeypatch: pytest.MonkeyPatch) -> None:
    """
    Test create tearsheet rejects disposed node state.
    """

    class DummyResult:
        """
        Collect dummy result tests.
        """

        run_config_id = "run-001"

    class DummyConfig:
        """
        Collect dummy config tests.
        """

        id = "run-001"
        dispose_on_completion = True

    class DummyNode:
        """
        Collect dummy node tests.
        """

        configs: ClassVar[list[object]] = [DummyConfig()]

    monkeypatch.setattr(tearsheet, "PLOTLY_AVAILABLE", True)

    with pytest.raises(ValueError, match="dispose_on_completion=False"):
        create_tearsheet(
            DummyResult(),
            node=DummyNode(),
            output_path=None,
            config=TearsheetConfig(charts=[TearsheetStatsTableChart()]),
        )


def test_result_account_info_filters_summary_balance_by_currency() -> None:
    """
    Test result account info filters summary balance by currency.
    """

    class DummyResult:
        """
        Collect dummy result tests.
        """

        summary: ClassVar[dict[str, object]] = {
            "account.SIM.balance.USD.total": "110.0",
            "account.SIM.balance.AUD.total": "220.0",
        }

    account_info = tearsheet._result_account_info(
        DummyResult(),
        node=None,
        run_config_id=None,
        currency="USD",
    )

    assert account_info == {"Ending balance (SIM, USD)": "110.0"}


def test_create_tearsheet_does_not_aggregate_mixed_currency_returns_without_filter(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test create tearsheet does not aggregate mixed currency returns without filter.
    """
    captured = []

    class DummyResult:
        """
        Collect dummy result tests.
        """

        stats_pnls: ClassVar[dict[str, object]] = {
            "USD": {"PnL (total)": 12.5},
            "AUD": {"PnL (total)": 1.0},
        }
        stats_returns: ClassVar[dict[str, object]] = {"Sharpe Ratio (252 days)": 1.23}
        stats_general: ClassVar[dict[str, object]] = {"Long Ratio": 0.5}
        elapsed_time_secs = 1.234
        iterations = 2
        total_events = 3
        total_orders = 4
        total_positions = 5

    class DummyCache:
        """
        Collect dummy cache tests.
        """

        @staticmethod
        def strategy_ids() -> object:
            """
            Strategy ids.
            """
            return ["S-001"]

    class DummyEngine:
        """
        Collect dummy engine tests.
        """

        run_id = "R-001"
        run_started = 1_577_836_800_000_000_000
        run_finished = 1_577_836_801_000_000_000
        backtest_start = 1_577_836_800_000_000_000
        backtest_end = 1_577_923_200_000_000_000
        cache = DummyCache()

        @staticmethod
        def get_result() -> object:
            """
            Get result.
            """
            return DummyResult()

        @staticmethod
        def list_venues() -> object:
            """
            List venues.
            """
            return ["SIM_USD", "SIM_AUD"]

        @staticmethod
        def generate_account_report(venue: Venue) -> object:
            """
            Generate account report.
            """
            currency = "USD" if venue == "SIM_USD" else "AUD"
            return pd.DataFrame(
                {
                    "currency": [currency, currency],
                    "total": ["100.0", "110.0"],
                },
                index=pd.to_datetime(["2020-01-01", "2020-01-02"], utc=True),
            )

    def capture_tearsheet_from_stats(**kwargs: object) -> str:
        """
        Capture tearsheet from stats.
        """
        captured.append(kwargs)
        return "<html></html>"

    monkeypatch.setattr(tearsheet, "PLOTLY_AVAILABLE", True)
    monkeypatch.setattr(tearsheet, "create_tearsheet_from_stats", capture_tearsheet_from_stats)

    create_tearsheet(DummyEngine(), output_path=None)
    create_tearsheet(DummyEngine(), output_path=None, currency="USD")

    assert captured[0]["returns"].empty
    assert captured[1]["stats_pnls"] == {"USD": {"PnL (total)": 12.5}}
    assert captured[1]["returns"].iloc[0] == pytest.approx(0.10)


@pytest.mark.skipif(not tearsheet.PLOTLY_AVAILABLE, reason="plotly is not installed")
def test_create_tearsheet_from_stats_returns_html() -> None:
    """
    Test create tearsheet from stats returns html.
    """
    returns = pd.Series(
        [0.01, -0.005, 0.002],
        index=pd.date_range("2020-01-01", periods=3, tz="UTC"),
    )

    html = create_tearsheet_from_stats(
        stats_pnls={"USD": {"PnL (total)": 100.0}},
        stats_returns={"Sharpe Ratio (252 days)": 1.0},
        stats_general={"Long Ratio": 0.5},
        returns=returns,
        output_path=None,
        title="Unit Test Tearsheet",
        config=TearsheetConfig(charts=[TearsheetEquityChart()], height=500),
    )

    assert html is not None
    assert "plotly" in html.lower()


@pytest.mark.skipif(not tearsheet.PLOTLY_AVAILABLE, reason="plotly is not installed")
def test_create_tearsheet_from_stats_accepts_empty_chart_config() -> None:
    """
    Test create tearsheet from stats accepts empty chart config.
    """
    html = create_tearsheet_from_stats(
        stats_pnls={"USD": {"PnL (total)": 100.0}},
        stats_returns={},
        stats_general={},
        returns=pd.Series(dtype=float),
        output_path=None,
        config=TearsheetConfig(charts=[]),
    )

    assert html is not None


def _sample_returns() -> object:
    return pd.Series(
        [0.01, -0.005, 0.002],
        index=pd.date_range("2020-01-01", periods=3, tz="UTC"),
    )


def _yearly_returns() -> object:
    return pd.Series(
        [0.10, -0.05],
        index=pd.to_datetime(["2020-01-01", "2021-01-01"], utc=True),
    )


@pytest.mark.skipif(not tearsheet.PLOTLY_AVAILABLE, reason="plotly is not installed")
@pytest.mark.parametrize(
    ("builder", "returns_factory", "kwargs_factory", "trace_types", "trace_names"),
    [
        (
            create_equity_curve,
            _sample_returns,
            lambda: {"benchmark_returns": _sample_returns()},
            ["scatter", "scatter"],
            ["Strategy", "Benchmark"],
        ),
        (
            create_drawdown_chart,
            _sample_returns,
            dict,
            ["scatter"],
            ["Drawdown"],
        ),
        (
            create_monthly_returns_heatmap,
            _sample_returns,
            dict,
            ["heatmap"],
            [None],
        ),
        (
            create_returns_distribution,
            _sample_returns,
            dict,
            ["histogram"],
            ["Returns"],
        ),
        (
            create_rolling_sharpe,
            _sample_returns,
            lambda: {"window": 60},
            [],
            [],
        ),
        (
            create_yearly_returns,
            _yearly_returns,
            dict,
            ["bar"],
            [None],
        ),
    ],
)
def test_standalone_chart_builders_return_expected_traces(
    builder: object,
    returns_factory: object,
    kwargs_factory: object,
    trace_types: object,
    trace_names: object,
) -> None:
    """
    Test standalone chart builders return expected traces.
    """
    fig = builder(returns_factory(), **kwargs_factory())

    assert [trace.type for trace in fig.data] == trace_types
    assert [trace.name for trace in fig.data] == trace_names


@pytest.mark.parametrize(
    ("builder", "args", "kwargs"),
    [
        (create_tearsheet, (object(),), {}),
        (
            create_tearsheet_from_stats,
            (),
            {
                "stats_pnls": {},
                "stats_returns": {},
                "stats_general": {},
                "returns": pd.Series(dtype=float),
            },
        ),
        (create_equity_curve, (pd.Series(dtype=float),), {}),
        (create_drawdown_chart, (pd.Series(dtype=float),), {}),
        (create_monthly_returns_heatmap, (pd.Series(dtype=float),), {}),
        (create_returns_distribution, (pd.Series(dtype=float),), {}),
        (create_rolling_sharpe, (pd.Series(dtype=float),), {}),
        (create_yearly_returns, (pd.Series(dtype=float),), {}),
        (create_bars_with_fills, (object(), object()), {}),
    ],
)
def test_public_visualization_builders_require_plotly(
    monkeypatch: pytest.MonkeyPatch,
    builder: object,
    args: tuple[object, ...],
    kwargs: object,
) -> None:
    """
    Test public visualization builders require plotly.
    """
    monkeypatch.setattr(tearsheet, "PLOTLY_AVAILABLE", False)

    with pytest.raises(ImportError, match="plotly is required"):
        builder(*args, **kwargs)


@pytest.mark.skipif(not tearsheet.PLOTLY_AVAILABLE, reason="plotly is not installed")
def test_create_bars_with_fills_uses_engine_fills_report() -> None:
    """
    Test create bars with fills uses engine fills report.
    """

    class DummyBarType:
        """
        Collect dummy bar type tests.
        """

        instrument_id = "AUD/USD.SIM"

        def __str__(self) -> str:
            """
            Str.
            """
            return "AUD/USD.SIM-1-MINUTE-BID-INTERNAL"

    class DummyBar:
        """
        Collect dummy bar tests.
        """

        @staticmethod
        def to_dict() -> object:
            """
            To dict.
            """
            return {
                "open": "1.00000",
                "high": "1.00010",
                "low": "0.99990",
                "close": "1.00005",
                "ts_init": 1_577_836_800_000_000_000,
            }

    class DummyCache:
        """
        Collect dummy cache tests.
        """

        @staticmethod
        def bars(bar_type: object) -> object:
            """
            Bars.
            """
            return [DummyBar()]

    class DummyEngine:
        """
        Collect dummy engine tests.
        """

        cache = DummyCache()

        @staticmethod
        def generate_fills_report() -> object:
            """
            Generate fills report.
            """
            return pd.DataFrame(
                {
                    "strategy_id": ["S-001", "S-001"],
                    "instrument_id": ["AUD/USD.SIM", "AUD/USD.SIM"],
                    "order_side": ["BUY", "SELL"],
                    "last_qty": ["100000", "50000"],
                    "last_px": ["1.00005", "1.00000"],
                    "ts_init": [
                        1_577_836_800_000_000_000,
                        1_577_836_801_000_000_000,
                    ],
                },
            )

    fig = create_bars_with_fills(DummyEngine(), DummyBarType(), output_path=None)

    assert len(fig.data) == 3
    assert fig.data[0].type == "candlestick"
    assert fig.data[1].name == "Buy Fills"
    assert fig.data[2].name == "Sell Fills"


@pytest.mark.skipif(not tearsheet.PLOTLY_AVAILABLE, reason="plotly is not installed")
def test_create_bars_with_fills_handles_empty_fills_report() -> None:
    """
    Test create bars with fills handles empty fills report.
    """

    class DummyBarType:
        """
        Collect dummy bar type tests.
        """

        instrument_id = "AUD/USD.SIM"

        def __str__(self) -> str:
            """
            Str.
            """
            return "AUD/USD.SIM-1-MINUTE-BID-INTERNAL"

    class DummyBar:
        """
        Collect dummy bar tests.
        """

        @staticmethod
        def to_dict() -> object:
            """
            To dict.
            """
            return {
                "open": "1.00000",
                "high": "1.00010",
                "low": "0.99990",
                "close": "1.00005",
                "ts_init": 1_577_836_800_000_000_000,
            }

    class DummyCache:
        """
        Collect dummy cache tests.
        """

        @staticmethod
        def bars(bar_type: object) -> object:
            """
            Bars.
            """
            return [DummyBar()]

    class DummyEngine:
        """
        Collect dummy engine tests.
        """

        cache = DummyCache()

        @staticmethod
        def generate_fills_report() -> object:
            """
            Generate fills report.
            """
            return pd.DataFrame()

    fig = create_bars_with_fills(DummyEngine(), DummyBarType(), output_path=None)

    assert len(fig.data) == 1
    assert fig.data[0].type == "candlestick"


@pytest.mark.skipif(not tearsheet.PLOTLY_AVAILABLE, reason="plotly is not installed")
def test_render_bars_with_fills_auto_discovers_bar_type_by_aggregation_source() -> None:
    """
    Test render bars with fills auto discovers bar type by aggregation source.
    """
    from plotly.subplots import make_subplots

    from nautilus_trader.model import AggregationSource

    bar_type_str = "AUD/USD.SIM-1-MINUTE-BID-EXTERNAL"

    class DummyBar:
        """
        Collect dummy bar tests.
        """

        @staticmethod
        def to_dict() -> object:
            """
            To dict.
            """
            return {
                "open": "1.00000",
                "high": "1.00010",
                "low": "0.99990",
                "close": "1.00005",
                "ts_init": 1_577_836_800_000_000_000,
            }

    class DummyCache:
        """
        Collect dummy cache tests.
        """

        @staticmethod
        def bar_types(aggregation_source: object) -> object:
            """
            Bar types.
            """
            # v2 requires aggregation_source; a no-argument call raises TypeError.
            return [bar_type_str] if aggregation_source == AggregationSource.EXTERNAL else []

        @staticmethod
        def bars(bar_type: object) -> object:
            """
            Bars.
            """
            return [DummyBar()]

    class DummyEngine:
        """
        Collect dummy engine tests.
        """

        cache = DummyCache()

        @staticmethod
        def generate_fills_report() -> object:
            """
            Generate fills report.
            """
            return pd.DataFrame()

    fig = make_subplots(rows=1, cols=1)
    tearsheet._render_bars_with_fills(fig, row=1, col=1, engine=DummyEngine(), bar_type=None)

    assert [trace.type for trace in fig.data] == ["candlestick"]


@pytest.mark.skipif(not tearsheet.PLOTLY_AVAILABLE, reason="plotly is not installed")
def test_create_tearsheet_from_stats_exports_static_image(tmp_path: Path) -> None:
    """
    Test create tearsheet from stats exports static image.
    """
    pytest.importorskip("kaleido")

    returns = pd.Series(
        [0.01, -0.005, 0.002],
        index=pd.date_range("2020-01-01", periods=3, tz="UTC"),
    )
    output_path = tmp_path / "tearsheet.png"

    try:
        create_tearsheet_from_stats(
            stats_pnls={"USD": {"PnL (total)": 100.0}},
            stats_returns={},
            stats_general={},
            returns=returns,
            output_path=str(output_path),
            config=TearsheetConfig(charts=[TearsheetEquityChart()], height=400),
        )
    except Exception as e:
        # A datetime axis reaching Kaleido unconverted fails with a
        # "Type is not JSON serializable: Timestamp" TypeError; treat that as a
        # real regression, and anything else (e.g. no Chrome) as environmental.
        if isinstance(e, TypeError) or "JSON serializable" in str(e):
            raise
        pytest.skip(f"kaleido static export unavailable: {e}")

    assert output_path.exists()
    assert output_path.stat().st_size > 0


def _run_backtest_with_fills() -> object:
    from nautilus_trader.backtest import BacktestEngine
    from nautilus_trader.backtest import BacktestEngineConfig
    from nautilus_trader.model import AccountType
    from nautilus_trader.model import Currency
    from nautilus_trader.model import Money
    from nautilus_trader.model import OmsType
    from nautilus_trader.model import Price
    from nautilus_trader.model import Quantity
    from nautilus_trader.model import QuoteTick
    from nautilus_trader.model import Venue
    from nautilus_trader.trading import ImportableStrategyConfig
    from tests.providers import TestInstrumentProvider

    audusd = TestInstrumentProvider.audusd_sim()
    usd = Currency.from_str("USD")
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True))
    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=usd,
        starting_balances=[Money(1_000_000.0, usd)],
    )
    engine.add_instrument(audusd)

    ts_start = 1_577_836_800_000_000_000
    one_day_ns = 86_400_000_000_000
    quotes = []

    for idx, px in enumerate(("0.70000", "0.70050", "0.70100", "0.70050", "0.70150", "0.70200")):
        ts = ts_start + idx * one_day_ns
        quotes.append(
            QuoteTick(
                instrument_id=audusd.id,
                bid_price=Price.from_str(px),
                ask_price=Price.from_str(px),
                bid_size=Quantity.from_int(1_000_000),
                ask_size=Quantity.from_int(1_000_000),
                ts_event=ts,
                ts_init=ts,
            ),
        )
    engine.add_data(quotes)
    engine.add_strategy_from_config(
        ImportableStrategyConfig(
            strategy_path="strategies.acceptance:TickScheduled",
            config_path="strategies.acceptance:TickScheduledConfig",
            config={
                "instrument_id": str(audusd.id),
                "actions": [(1, "BUY", "100000"), (4, "SELL", "100000")],
            },
        ),
    )
    engine.run()
    return engine


def _run_issue_3899_backtest() -> object:
    from nautilus_trader.backtest import BacktestEngine
    from nautilus_trader.backtest import BacktestEngineConfig
    from nautilus_trader.model import AccountType
    from nautilus_trader.model import Bar
    from nautilus_trader.model import BarAggregation
    from nautilus_trader.model import BarSpecification
    from nautilus_trader.model import BarType
    from nautilus_trader.model import Currency
    from nautilus_trader.model import Equity
    from nautilus_trader.model import InstrumentId
    from nautilus_trader.model import Money
    from nautilus_trader.model import OmsType
    from nautilus_trader.model import OrderSide
    from nautilus_trader.model import Price
    from nautilus_trader.model import PriceType
    from nautilus_trader.model import Quantity
    from nautilus_trader.model import Symbol
    from nautilus_trader.model import TimeInForce
    from nautilus_trader.model import Venue
    from nautilus_trader.trading import Strategy

    class BuyHoldThenSellStrategy(Strategy):
        """
        Collect buy hold then sell strategy tests.
        """

        def __init__(self) -> None:
            """
            Initialize the instance.
            """
            super().__init__()
            self.instrument = None
            self.bar_type = None
            self.day_index = 0

        def on_start(self) -> None:
            """
            On start.
            """
            self.subscribe_bars(self.bar_type)

        def on_bar(self, _bar: object) -> None:
            """
            On bar.
            """
            self.day_index += 1
            if self.day_index not in {1, 7}:
                return

            order_side = OrderSide.BUY if self.day_index == 1 else OrderSide.SELL
            order = self.order_factory.market(
                instrument_id=self.instrument.id,
                order_side=order_side,
                quantity=Quantity.from_int(90),
                time_in_force=TimeInForce.DAY,
            )
            self.submit_order(order)

    usd = Currency.from_str("USD")
    venue = Venue("XNAS")
    instrument = Equity(
        instrument_id=InstrumentId(Symbol("KO"), venue),
        raw_symbol=Symbol("KO"),
        currency=usd,
        price_precision=2,
        price_increment=Price.from_str("0.01"),
        ts_event=0,
        ts_init=0,
    )
    bar_type = BarType(
        instrument.id,
        BarSpecification(1, BarAggregation.DAY, PriceType.LAST),
    )
    prices = [
        ("100.00", "101.00", "99.00", "100.50"),
        ("100.00", "104.00", "99.50", "103.50"),
        ("103.00", "107.00", "102.00", "106.50"),
        ("106.00", "111.00", "105.00", "110.50"),
        ("110.00", "115.00", "109.00", "114.50"),
        ("114.00", "119.00", "113.00", "118.50"),
        ("118.00", "123.00", "117.00", "122.50"),
        ("122.00", "126.00", "121.00", "125.00"),
    ]
    timestamps = pd.date_range("2024-01-02 21:00:00+00:00", periods=8, freq="B")
    bars = [
        Bar(
            bar_type=bar_type,
            open=Price.from_str(open_px),
            high=Price.from_str(high_px),
            low=Price.from_str(low_px),
            close=Price.from_str(close_px),
            volume=Quantity.from_int(1_000_000),
            ts_event=timestamp.value,
            ts_init=timestamp.value,
        )
        for timestamp, (open_px, high_px, low_px, close_px) in zip(timestamps, prices, strict=True)
    ]

    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True))
    engine.add_venue(
        venue=venue,
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        starting_balances=[Money(10_000.0, usd)],
        base_currency=usd,
    )
    engine.add_instrument(instrument)
    engine.add_data(bars)
    strategy = BuyHoldThenSellStrategy()
    strategy.instrument = instrument
    strategy.bar_type = bar_type
    engine.add_strategy(strategy)
    engine.run()
    return engine, timestamps[-1].value


def test_issue_3899_equity_curve_tracks_open_position_mark_to_market() -> None:
    """
    Test issue 3899 equity curve tracks open position mark to market.
    """
    engine, final_ts = _run_issue_3899_backtest()

    try:
        venue = engine.list_venues()[0]
        account = engine.cache.account_for_venue(venue)
        snapshots = engine.portfolio.snapshots(account.id)
        returns = tearsheet._resolve_tearsheet_returns(engine=engine)
        normalized_equity = (1 + returns).cumprod()
        fills = engine.generate_fills_report()
    finally:
        engine.dispose()

    assert fills["last_qty"].tolist() == ["90", "90"]
    assert fills["last_px"].tolist() == ["100.50", "122.50"]
    assert len(snapshots) >= 8
    assert snapshots[-1].ts_event == final_ts
    assert len(normalized_equity) >= 6
    assert normalized_equity.nunique() >= 5
    assert normalized_equity.iloc[-1] == pytest.approx(1.198)


@pytest.mark.skipif(not tearsheet.PLOTLY_AVAILABLE, reason="plotly is not installed")
def test_create_tearsheet_end_to_end_real_engine() -> None:
    """
    Test create tearsheet end to end real engine.
    """
    engine = _run_backtest_with_fills()

    try:
        result = engine.get_result()
        orders = engine.generate_orders_report()
        order_fills = engine.generate_order_fills_report()
        fills = engine.generate_fills_report()
        positions = engine.generate_positions_report()
        account = engine.generate_account_report(venue=engine.list_venues()[0])
        account_info = tearsheet._collect_account_info(engine=engine)
        returns = tearsheet._resolve_tearsheet_returns(engine=engine)
        html = create_tearsheet(engine, output_path=None, title="E2E Tearsheet")
    finally:
        engine.dispose()

    assert result.iterations == 6
    assert result.total_orders == 2
    assert result.total_positions == 1
    assert result.summary["orders.open"] == "0"
    assert result.summary["positions.open"] == "0"
    assert result.stats_pnls["USD"]["PnL (total)"] == pytest.approx(47.2)
    assert result.stats_returns["Average (Return)"] == pytest.approx(returns.mean())
    assert (1 + returns).prod() == pytest.approx(1.0000472)
    assert result.stats_general == {"Long Ratio": 1.0}
    assert len(orders) == result.total_orders
    assert orders["status"].tolist() == ["FILLED", "FILLED"]
    assert len(order_fills) == 2
    assert order_fills["filled_qty"].tolist() == ["100000", "100000"]
    assert len(fills) == 2
    assert set(fills["order_side"]) == {"BUY", "SELL"}
    assert fills["last_qty"].tolist() == ["100000", "100000"]
    assert len(positions) == result.total_positions
    assert positions["side"].tolist() == ["FLAT"]
    assert positions["realized_pnl"].tolist() == ["47.20 USD"]
    assert account["currency"].tolist() == ["USD", "USD", "USD"]
    assert account["total"].tolist() == ["1000000.00", "999998.60", "1000047.20"]
    assert any(key.startswith("Starting balance") for key in account_info)
    assert isinstance(returns, pd.Series)
    assert "plotly" in html.lower()
    assert "PnL Statistics (USD)" in html
    assert engine.cache.orders_open_count() == 0
    assert engine.cache.positions_open_count() == 0


def test_to_returns_series_normalizes_inputs() -> None:
    """
    Test to returns series normalizes inputs.
    """
    # None and empty inputs collapse to an empty float Series.
    assert tearsheet._to_returns_series(None).empty
    assert tearsheet._to_returns_series(pd.Series(dtype=float)).empty

    # An integer index is interpreted as epoch nanoseconds.
    epoch_ns = {1_577_836_800_000_000_000: 0.01, 1_577_923_200_000_000_000: -0.02}
    from_epoch = tearsheet._to_returns_series(epoch_ns)
    assert isinstance(from_epoch.index, pd.DatetimeIndex)
    assert str(from_epoch.index.tz) == "UTC"
    assert from_epoch.index[0] == pd.Timestamp("2020-01-01", tz="UTC")

    # A string index is parsed as calendar dates (not epoch) and sorted ascending.
    from_iso = tearsheet._to_returns_series({"2021-03-02": 0.03, "2021-03-01": -0.01})
    assert from_iso.index[0] == pd.Timestamp("2021-03-01", tz="UTC")
    assert from_iso.iloc[0] == pytest.approx(-0.01)

    # An existing tz-aware DatetimeIndex Series is preserved and sorted.
    unsorted = pd.Series(
        [0.02, 0.01],
        index=pd.to_datetime(["2020-01-02", "2020-01-01"], utc=True),
    )
    normalized = tearsheet._to_returns_series(unsorted)
    assert list(normalized.index) == sorted(normalized.index)
    assert normalized.iloc[0] == pytest.approx(0.01)


def _account_report(rows: object) -> object:
    return pd.DataFrame(
        {"currency": [currency for _, currency, _ in rows], "total": [total for *_, total in rows]},
        index=pd.to_datetime([ts for ts, _, _ in rows], utc=True),
    )


def test_resolve_tearsheet_returns_prefers_mark_to_market_snapshots() -> None:
    """
    Test resolve tearsheet returns prefers mark to market snapshots.
    """

    class DummyCurrency:
        """
        Collect dummy currency tests.
        """

        code = "USD"

    class DummyMoney:
        """
        Collect dummy money tests.
        """

        currency = DummyCurrency()

        def __init__(self, value: object) -> None:
            """
            Initialize the instance.
            """
            self.value = value

        def as_double(self) -> object:
            """
            As double.
            """
            return self.value

    class DummySnapshot:
        """
        Collect dummy snapshot tests.
        """

        base_currency_equity = None

        def __init__(self, value: object, ts_event: object, unpriced: object = False) -> None:
            """
            Initialize the instance.
            """
            self.total_equity = [DummyMoney(value)]
            self.ts_event = ts_event
            self.unpriced_instruments = ["KO.XNAS"] if unpriced else []

    class DummyAccount:
        """
        Collect dummy account tests.
        """

        id = "SIM-001"

    class DummyCache:
        """
        Collect dummy cache tests.
        """

        @staticmethod
        def account_for_venue(venue: Venue) -> object:
            """
            Account for venue.
            """
            return DummyAccount()

    class DummyPortfolio:
        """
        Collect dummy portfolio tests.
        """

        @staticmethod
        def snapshots(account_id: AccountId) -> object:
            """
            Snapshots.
            """
            return [
                DummySnapshot(100.0, 1_577_880_000_000_000_000),
                DummySnapshot(105.0, 1_577_901_600_000_000_000),
                DummySnapshot(110.0, 1_577_923_200_000_000_000),
                DummySnapshot(1.0, 1_578_009_600_000_000_000, unpriced=True),
                DummySnapshot(121.0, 1_578_096_000_000_000_000),
            ]

    class DummyEngine:
        """
        Collect dummy engine tests.
        """

        cache = DummyCache()
        portfolio = DummyPortfolio()

        @staticmethod
        def list_venues() -> object:
            """
            List venues.
            """
            return ["SIM"]

        @staticmethod
        def generate_account_report(venue: Venue) -> object:
            """
            Generate account report.
            """
            return _account_report(
                [("2020-01-01", "USD", "100.0"), ("2020-01-02", "USD", "110.0")],
            )

    returns = tearsheet._resolve_tearsheet_returns(engine=DummyEngine())

    assert returns.index.strftime("%Y-%m-%d").tolist() == ["2020-01-01", "2020-01-02", "2020-01-03"]
    assert returns.tolist() == pytest.approx([0.10, 0.0, 0.10])


def test_calculate_snapshot_returns_rejects_mixed_account_currencies() -> None:
    """
    Test calculate snapshot returns rejects mixed account currencies.
    """

    class DummyCurrency:
        """
        Collect dummy currency tests.
        """

        def __init__(self, code: object) -> None:
            """
            Initialize the instance.
            """
            self.code = code

    class DummyMoney:
        """
        Collect dummy money tests.
        """

        def __init__(self, value: object, currency: object) -> None:
            """
            Initialize the instance.
            """
            self.value = value
            self.currency = DummyCurrency(currency)

        def as_double(self) -> object:
            """
            As double.
            """
            return self.value

    class DummySnapshot:
        """
        Collect dummy snapshot tests.
        """

        base_currency_equity = None

        def __init__(self, value: object, currency: object, ts_event: object) -> None:
            """
            Initialize the instance.
            """
            self.total_equity = [DummyMoney(value, currency)]
            self.ts_event = ts_event

    class DummyAccount:
        """
        Collect dummy account tests.
        """

        def __init__(self, account_id: AccountId) -> None:
            """
            Initialize the instance.
            """
            self.id = account_id

    class DummyCache:
        """
        Collect dummy cache tests.
        """

        @staticmethod
        def account_for_venue(venue: Venue) -> object:
            """
            Account for venue.
            """
            return DummyAccount(venue)

    class DummyPortfolio:
        """
        Collect dummy portfolio tests.
        """

        @staticmethod
        def snapshots(account_id: AccountId) -> object:
            """
            Snapshots.
            """
            currency = "USD" if account_id == "SIM_USD" else "AUD"
            return [
                DummySnapshot(100.0, currency, 1_577_836_800_000_000_000),
                DummySnapshot(110.0, currency, 1_577_923_200_000_000_000),
            ]

    class DummyEngine:
        """
        Collect dummy engine tests.
        """

        cache = DummyCache()
        portfolio = DummyPortfolio()

        @staticmethod
        def list_venues() -> object:
            """
            List venues.
            """
            return ["SIM_USD", "SIM_AUD"]

    assert tearsheet._calculate_snapshot_returns(engine=DummyEngine()) is None


def test_resolve_snapshot_equity_uses_requested_total_currency() -> None:
    """
    Test resolve snapshot equity uses requested total currency.
    """
    from nautilus_trader.model import Currency
    from nautilus_trader.model import Money

    class DummySnapshot:
        """
        Collect dummy snapshot tests.
        """

        base_currency_equity = Money(90.0, Currency.from_str("EUR"))
        total_equity: ClassVar[list[object]] = [Money(100.0, Currency.from_str("USD"))]

    assert tearsheet._resolve_snapshot_equity(DummySnapshot(), "USD") == (100.0, "USD")
    assert tearsheet._resolve_snapshot_equity(DummySnapshot(), "AUD") is None


def test_resolve_snapshot_equity_rejects_implicit_multi_currency_total() -> None:
    """
    Test resolve snapshot equity rejects implicit multi currency total.
    """
    from nautilus_trader.model import Currency
    from nautilus_trader.model import Money

    class DummySnapshot:
        """
        Collect dummy snapshot tests.
        """

        base_currency_equity = Money(100.0, Currency.from_str("USD"))
        total_equity: ClassVar[list[object]] = [
            Money(100.0, Currency.from_str("USD")),
            Money(50.0, Currency.from_str("AUD")),
        ]

    assert tearsheet._resolve_snapshot_equity(DummySnapshot(), None) is None


def test_calculate_account_returns_rejects_single_venue_mixed_currency() -> None:
    """
    Test calculate account returns rejects single venue mixed currency.
    """

    class DummyEngine:
        """
        Collect dummy engine tests.
        """

        @staticmethod
        def list_venues() -> object:
            """
            List venues.
            """
            return ["SIM"]

        @staticmethod
        def generate_account_report(venue: Venue) -> object:
            """
            Generate account report.
            """
            return _account_report(
                [("2020-01-01", "USD", "100.0"), ("2020-01-02", "EUR", "200.0")],
            )

    engine = DummyEngine()

    assert tearsheet._calculate_account_returns(engine=engine) is None
    assert tearsheet._resolve_tearsheet_returns(engine=engine).empty


def test_calculate_account_returns_aggregates_same_currency_across_venues() -> None:
    """
    Test calculate account returns aggregates same currency across venues.
    """
    reports = {
        "SIM_A": _account_report(
            [("2020-01-01", "USD", "100.0"), ("2020-01-02", "USD", "110.0")],
        ),
        "SIM_B": _account_report(
            [("2020-01-01", "USD", "50.0"), ("2020-01-02", "USD", "50.0")],
        ),
    }

    class DummyEngine:
        """
        Collect dummy engine tests.
        """

        @staticmethod
        def list_venues() -> object:
            """
            List venues.
            """
            return list(reports)

        @staticmethod
        def generate_account_report(venue: Venue) -> object:
            """
            Generate account report.
            """
            return reports[venue]

    returns = tearsheet._calculate_account_returns(engine=DummyEngine())

    # Combined balance 150 -> 160 across both venues yields one daily return.
    assert returns is not None
    assert returns.iloc[0] == pytest.approx(160 / 150 - 1)


def test_normalize_theme_config_fills_table_colors() -> None:
    """
    Test normalize theme config fills table colors.
    """
    minimal = {
        "template": "plotly_white",
        "colors": {
            "primary": "#000000",
            "positive": "#00ff00",
            "negative": "#ff0000",
            "neutral": "#808080",
            "background": "#ffffff",
            "grid": "#dddddd",
        },
    }

    colors = tearsheet._normalize_theme_config(minimal)["colors"]

    assert colors["table_section"] == "#dddddd"  # falls back to grid
    assert colors["table_row_even"] == "#ffffff"  # falls back to background
    assert colors["table_text"] == "#000000"  # white background -> dark text


@pytest.mark.skipif(not tearsheet.PLOTLY_AVAILABLE, reason="plotly is not installed")
def test_create_tearsheet_from_stats_renders_per_currency_pnl() -> None:
    """
    Test create tearsheet from stats renders per currency pnl.
    """
    html = create_tearsheet_from_stats(
        stats_pnls={"USD": {"PnL (total)": 100.0}, "AUD": {"PnL (total)": -5.0}},
        stats_returns={},
        stats_general={},
        returns=pd.Series(dtype=float),
        output_path=None,
        config=TearsheetConfig(charts=[TearsheetStatsTableChart()], height=400),
    )

    assert html is not None
    assert "PnL Statistics (USD)" in html
    assert "PnL Statistics (AUD)" in html


@pytest.mark.skipif(not tearsheet.PLOTLY_AVAILABLE, reason="plotly is not installed")
def test_register_tearsheet_chart_renders_custom_chart_in_grid() -> None:
    """
    Test register tearsheet chart renders custom chart in grid.
    """
    import plotly.graph_objects as go

    def _render_custom(
        fig: object,
        row: object,
        col: object,
        returns: object,
        theme_config: object,
        **kwargs: object,
    ) -> None:
        fig.add_trace(
            go.Scatter(x=[1, 2, 3], y=[3, 2, 1], name="CustomSignal"),
            row=row,
            col=col,
        )

    try:
        register_tearsheet_chart("unit_test_custom", "scatter", "Custom", _render_custom)

        fig = tearsheet._create_tearsheet_figure(
            stats_returns={},
            stats_general={},
            stats_pnls={},
            returns=pd.Series(dtype=float),
            title="Custom Chart Test",
            config=TearsheetConfig(charts=[TearsheetCustomChart(chart="unit_test_custom")]),
        )
    finally:
        tearsheet._TEARSHEET_CHART_SPECS.pop("unit_test_custom", None)

    assert [trace.name for trace in fig.data] == ["CustomSignal"]


@pytest.mark.skipif(not tearsheet.PLOTLY_AVAILABLE, reason="plotly is not installed")
def test_tearsheet_unknown_chart_name_raises_with_suggestion() -> None:
    """
    Test tearsheet unknown chart name raises with suggestion.
    """
    # An unregistered chart name fails loud with a suggestion rather than
    # silently rendering an empty titled cell.
    with pytest.raises(KeyError, match="Did you mean: equity"):
        create_tearsheet_from_stats(
            stats_pnls={},
            stats_returns={},
            stats_general={},
            returns=pd.Series(dtype=float),
            output_path=None,
            config=TearsheetConfig(charts=[TearsheetCustomChart(chart="equit")]),
        )


@pytest.mark.parametrize("num_charts", [9, 11, 16])
def test_calculate_grid_layout_grows_to_fit_all_charts(num_charts: object) -> None:
    """
    Test calculate grid layout grows to fit all charts.
    """
    rows, cols, *_ = tearsheet._calculate_grid_layout([TearsheetEquityChart()] * num_charts)

    assert rows * cols >= num_charts


def test_calculate_grid_layout_raises_when_layout_too_small() -> None:
    """
    Test calculate grid layout raises when layout too small.
    """
    from nautilus_trader.analysis.config import GridLayout

    charts = [TearsheetEquityChart(), TearsheetEquityChart()]
    layout = GridLayout(rows=1, cols=1, heights=[1.0])

    with pytest.raises(ValueError, match="2 charts were configured"):
        tearsheet._calculate_grid_layout(charts, layout)


@pytest.mark.skipif(not tearsheet.PLOTLY_AVAILABLE, reason="plotly is not installed")
def test_create_tearsheet_renders_chart_beyond_default_grid() -> None:
    """
    Test create tearsheet renders chart beyond default grid.
    """
    import plotly.graph_objects as go

    def _render_ninth(
        fig: object,
        row: object,
        col: object,
        returns: object,
        theme_config: object,
        **kwargs: object,
    ) -> None:
        fig.add_trace(go.Scatter(x=[1], y=[1], name="NinthChart"), row=row, col=col)

    try:
        register_tearsheet_chart("unit_test_ninth", "scatter", "Ninth", _render_ninth)

        charts = [*TearsheetConfig().charts, TearsheetCustomChart(chart="unit_test_ninth")]
        assert len(charts) == 9

        fig = tearsheet._create_tearsheet_figure(
            stats_returns={},
            stats_general={},
            stats_pnls={},
            returns=pd.Series(dtype=float),
            title="Overflow Test",
            config=TearsheetConfig(charts=charts),
        )
    finally:
        tearsheet._TEARSHEET_CHART_SPECS.pop("unit_test_ninth", None)

    # The 9th chart must render rather than being silently dropped by the grid cap.
    assert any(trace.name == "NinthChart" for trace in fig.data)


def test_register_tearsheet_chart_validates_inputs() -> None:
    """
    Test register tearsheet chart validates inputs.
    """
    with pytest.raises(ValueError, match="cannot be empty"):
        register_tearsheet_chart("", "scatter", "X", lambda **kwargs: None)

    with pytest.raises(ValueError, match="must be callable"):
        register_tearsheet_chart("unit_test_bad", "scatter", "X", "not-callable")


@pytest.fixture
def two_month_returns() -> object:
    """
    Two month returns.
    """
    # Two +10% single-day returns, one per consecutive month: the equity index ends
    # each month at 1.10 then 1.21, so compounded monthly returns are [10%, 10%] and
    # simple (fixed-base) monthly returns are [10%, 11%], both totalling 21%.
    index = pd.to_datetime(["2024-01-31", "2024-02-29"])
    return pd.Series([0.10, 0.10], index=index)


def test_aggregate_period_returns_compounding_uses_running_base(two_month_returns: object) -> None:
    """
    Test aggregate period returns compounding uses running base.
    """
    result = tearsheet._aggregate_period_returns(two_month_returns, "ME", compounding=True)

    assert result.tolist() == pytest.approx([10.0, 10.0])


def test_aggregate_period_returns_simple_uses_fixed_initial_base(two_month_returns: object) -> None:
    """
    Test aggregate period returns simple uses fixed initial base.
    """
    result = tearsheet._aggregate_period_returns(two_month_returns, "ME", compounding=False)

    assert result.tolist() == pytest.approx([10.0, 11.0])


def test_aggregate_period_returns_simple_sums_to_total_return(two_month_returns: object) -> None:
    """
    Test aggregate period returns simple sums to total return.
    """
    total = ((1 + two_month_returns).prod() - 1) * 100  # 21%

    simple = tearsheet._aggregate_period_returns(two_month_returns, "ME", compounding=False)
    compounded = tearsheet._aggregate_period_returns(two_month_returns, "ME", compounding=True)

    assert simple.sum() == pytest.approx(total)
    assert ((1 + compounded / 100).prod() - 1) * 100 == pytest.approx(total)


def test_aggregate_period_returns_simple_is_order_independent() -> None:
    """
    Test aggregate period returns simple is order independent.
    """
    # Same two returns as the sorted fixture, but rows out of date order
    unsorted = pd.Series([0.10, 0.10], index=pd.to_datetime(["2024-02-29", "2024-01-31"]))

    result = tearsheet._aggregate_period_returns(unsorted, "ME", compounding=False)

    assert result.tolist() == pytest.approx([10.0, 11.0])


@pytest.mark.parametrize("compounding", [True, False])
def test_aggregate_period_returns_empty_interior_period_is_zero(compounding: object) -> None:
    """
    Test aggregate period returns empty interior period is zero.
    """
    # Jan and Mar have returns, Feb has none (an empty interior month)
    returns = pd.Series([0.10, 0.10], index=pd.to_datetime(["2024-01-31", "2024-03-31"]))

    result = tearsheet._aggregate_period_returns(returns, "ME", compounding=compounding)

    assert list(result.index.month) == [1, 2, 3]
    assert result.iloc[1] == pytest.approx(0.0)
    assert not result.isna().any()


def test_aggregate_period_returns_single_period_modes_match() -> None:
    """
    Test aggregate period returns single period modes match.
    """
    returns = pd.Series([0.10], index=pd.to_datetime(["2024-01-31"]))

    compounded = tearsheet._aggregate_period_returns(returns, "ME", compounding=True)
    simple = tearsheet._aggregate_period_returns(returns, "ME", compounding=False)

    assert compounded.tolist() == pytest.approx([10.0])
    assert simple.tolist() == pytest.approx([10.0])


def test_monthly_returns_chart_compounding_kwarg() -> None:
    """
    Test monthly returns chart compounding kwarg.
    """
    assert TearsheetMonthlyReturnsChart().kwargs() == {"compounding": True}
    assert TearsheetMonthlyReturnsChart(compounding=False).kwargs() == {"compounding": False}


def test_yearly_returns_chart_compounding_kwarg() -> None:
    """
    Test yearly returns chart compounding kwarg.
    """
    assert TearsheetYearlyReturnsChart().kwargs() == {"compounding": True}
    assert TearsheetYearlyReturnsChart(compounding=False).kwargs() == {"compounding": False}


def test_create_monthly_returns_heatmap_simple_default_title(two_month_returns: object) -> None:
    """
    Test create monthly returns heatmap simple default title.
    """
    compounded = create_monthly_returns_heatmap(two_month_returns)
    simple = create_monthly_returns_heatmap(two_month_returns, compounding=False)

    assert compounded.layout.title.text == "Monthly Returns (%)"
    assert simple.layout.title.text == "Monthly Returns (% of initial capital)"


def test_create_yearly_returns_simple_default_title(two_month_returns: object) -> None:
    """
    Test create yearly returns simple default title.
    """
    compounded = create_yearly_returns(two_month_returns)
    simple = create_yearly_returns(two_month_returns, compounding=False)

    assert compounded.layout.title.text == "Yearly Returns"
    assert simple.layout.title.text == "Yearly Returns (% of initial capital)"


def test_create_monthly_returns_heatmap_explicit_title_preserved(two_month_returns: object) -> None:
    """
    Test create monthly returns heatmap explicit title preserved.
    """
    fig = create_monthly_returns_heatmap(two_month_returns, compounding=False, title="Custom")

    assert fig.layout.title.text == "Custom"


def test_tearsheet_figure_routes_compounding_flag_to_renderer(two_month_returns: object) -> None:
    """
    Test tearsheet figure routes compounding flag to renderer.
    """
    config = TearsheetConfig(charts=[TearsheetMonthlyReturnsChart(compounding=False)])

    fig = tearsheet._create_tearsheet_figure(
        stats_returns={},
        stats_general={},
        stats_pnls={},
        returns=two_month_returns,
        title="Routing",
        config=config,
    )

    heatmaps = [trace for trace in fig.data if trace.type == "heatmap"]
    assert len(heatmaps) == 1
    # Jan (fixed base) == 10%, Feb (fixed base) == 11%
    assert heatmaps[0].z[0] == pytest.approx([10.0, 11.0])


def test_create_tearsheet_figure_requires_plotly(
    monkeypatch: pytest.MonkeyPatch,
    two_month_returns: object,
) -> None:
    """
    Test create tearsheet figure requires plotly.
    """
    monkeypatch.setattr(tearsheet, "PLOTLY_AVAILABLE", False)

    with pytest.raises(ImportError, match="plotly is required"):
        tearsheet._create_tearsheet_figure(
            stats_returns={},
            stats_general={},
            stats_pnls={},
            returns=two_month_returns,
            title="X",
        )
