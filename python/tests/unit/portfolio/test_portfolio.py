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
Test portfolio behavior.
"""

from __future__ import annotations

import importlib
import subprocess
import sys
import textwrap

import pytest

from nautilus_trader.analysis import PortfolioStatistic
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


def test_portfolio_public_module_exports_pyo3_classes() -> None:
    """
    Test portfolio public module exports pyo3 classes.
    """
    portfolio = importlib.import_module("nautilus_trader.portfolio")
    native_portfolio = importlib.import_module("nautilus_trader._libnautilus.portfolio")

    assert portfolio.Portfolio is native_portfolio.Portfolio
    assert portfolio.PortfolioConfig is native_portfolio.PortfolioConfig
    assert portfolio.Portfolio.__name__ == "Portfolio"
    assert portfolio.PortfolioConfig.__name__ == "PortfolioConfig"


def test_portfolio_config_defaults_equity_curve_on_and_allows_opt_out() -> None:
    """
    Test portfolio config defaults equity curve on and allows opt out.
    """
    from nautilus_trader.portfolio import PortfolioConfig

    default = PortfolioConfig()
    disabled = PortfolioConfig(equity_curve=False)

    assert default.equity_curve is True
    assert default.snapshot_interval_ms is None
    assert disabled.equity_curve is False


def test_portfolio_public_module_sets_runtime_module_names() -> None:
    """
    Test portfolio public module sets runtime module names.
    """
    script = textwrap.dedent(
        """
        import importlib

        portfolio = importlib.import_module("nautilus_trader.portfolio")
        native_portfolio = importlib.import_module("nautilus_trader._libnautilus.portfolio")

        assert portfolio.Portfolio is native_portfolio.Portfolio
        assert portfolio.PortfolioConfig is native_portfolio.PortfolioConfig
        assert portfolio.Portfolio.__module__ == "nautilus_trader.portfolio"
        assert portfolio.PortfolioConfig.__module__ == "nautilus_trader.portfolio"
        """,
    )

    result = subprocess.run(
        [sys.executable, "-c", script],
        capture_output=True,
        check=False,
        text=True,
    )

    assert result.returncode == 0, result.stderr


def test_live_reexports_portfolio_config_for_compatibility() -> None:
    """
    Test live reexports portfolio config for compatibility.
    """
    from nautilus_trader.backtest import BacktestEngineConfig
    from nautilus_trader.live import LiveNodeConfig
    from nautilus_trader.live import PortfolioConfig as LivePortfolioConfig
    from nautilus_trader.portfolio import PortfolioConfig

    live_config = LiveNodeConfig(portfolio=LivePortfolioConfig())
    backtest_config = BacktestEngineConfig(portfolio=PortfolioConfig())

    assert LivePortfolioConfig is PortfolioConfig
    assert isinstance(live_config, LiveNodeConfig)
    assert isinstance(backtest_config.portfolio, PortfolioConfig)


_TS_START = 1_577_836_800_000_000_000
_BID_PRICES = ("0.70000", "0.70000", "0.70010", "0.70020", "0.70020")


class CategorySentinel(PortfolioStatistic):
    """
    Returns a distinct constant per category and records the input size it was fed.
    """

    def __init__(self) -> None:
        """
        Initialize the per-category input counters.
        """
        self.returns_seen = 0
        self.realized_pnls_seen = 0
        self.positions_seen = 0

    def calculate_from_returns(self, returns: dict[int, float]) -> float | None:
        """
        Record the returns count and return the returns sentinel.
        """
        self.returns_seen = len(returns)
        return 11.0

    def calculate_from_realized_pnls(self, realized_pnls: list[float]) -> float | None:
        """
        Record the realized PnL count and return the PnL sentinel.
        """
        self.realized_pnls_seen = len(realized_pnls)
        return 22.0

    def calculate_from_positions(self, positions: list) -> float | None:
        """
        Record the position count and return the position sentinel.
        """
        self.positions_seen = len(positions)
        return 33.0


def _quotes(instrument: object) -> list[QuoteTick]:
    quotes: list[QuoteTick] = []

    for idx, bid_price in enumerate(_BID_PRICES):
        ts = _TS_START + idx * 60_000_000_000
        quotes.append(
            QuoteTick(
                instrument_id=instrument.id,
                bid_price=Price.from_str(bid_price),
                ask_price=Price.from_str(bid_price),
                bid_size=Quantity.from_int(1_000_000),
                ask_size=Quantity.from_int(1_000_000),
                ts_event=ts,
                ts_init=ts,
            ),
        )
    return quotes


def _engine_with_fills() -> BacktestEngine:
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
    engine.add_data(_quotes(audusd))
    engine.add_strategy_from_config(
        ImportableStrategyConfig(
            strategy_path="strategies.acceptance:TickScheduled",
            config_path="strategies.acceptance:TickScheduledConfig",
            config={
                "instrument_id": str(audusd.id),
                "actions": [(2, "BUY", "100000"), (4, "SELL", "100000")],
            },
        ),
    )
    return engine


def test_registered_statistic_reaches_statistics_and_backtest_result() -> None:
    """
    Test registered statistic reaches statistics and backtest result.
    """
    engine = _engine_with_fills()
    sentinel = CategorySentinel()
    engine.portfolio.register_statistic(sentinel)

    engine.run()

    statistics = engine.portfolio.statistics()
    result = engine.get_result()

    assert statistics.returns["Category Sentinel"] == 11.0
    assert statistics.pnls["USD"]["Category Sentinel"] == 22.0
    assert statistics.general["Category Sentinel"] == 33.0

    assert result.stats_returns["Category Sentinel"] == 11.0
    assert result.stats_pnls["USD"]["Category Sentinel"] == 22.0
    assert result.stats_general["Category Sentinel"] == 33.0

    # The built-in defaults are preserved alongside the registration.
    assert "Long Ratio" in result.stats_general

    engine.dispose()


def test_registered_statistic_receives_authoritative_inputs() -> None:
    """
    Test registered statistic receives authoritative inputs.
    """
    engine = _engine_with_fills()
    sentinel = CategorySentinel()
    engine.portfolio.register_statistic(sentinel)

    engine.run()
    statistics = engine.portfolio.statistics()

    expected_positions = len(engine.cache.positions()) + len(engine.cache.position_snapshots())

    assert expected_positions > 0
    assert statistics.returns_series
    assert sentinel.positions_seen == expected_positions
    assert sentinel.returns_seen == len(statistics.returns_series)
    assert sentinel.realized_pnls_seen == 1

    engine.dispose()


def test_registered_statistic_survives_repeated_statistics_queries() -> None:
    """
    Test registered statistic survives repeated statistics queries.
    """
    engine = _engine_with_fills()
    engine.portfolio.register_statistic(CategorySentinel())

    engine.run()

    for _ in range(3):
        statistics = engine.portfolio.statistics()
        assert statistics.general["Category Sentinel"] == 33.0

    engine.dispose()


def test_deregistered_statistic_leaves_backtest_result() -> None:
    """
    Test deregistered statistic leaves backtest result.
    """
    engine = _engine_with_fills()
    sentinel = CategorySentinel()
    engine.portfolio.register_statistic(sentinel)

    engine.run()
    assert "Category Sentinel" in engine.get_result().stats_general

    engine.portfolio.deregister_statistic(sentinel)
    result = engine.get_result()

    assert "Category Sentinel" not in result.stats_general
    assert "Category Sentinel" not in result.stats_returns
    assert "Long Ratio" in result.stats_general

    engine.dispose()


def test_portfolio_register_statistic_rejects_invalid_name() -> None:
    """
    Test portfolio register statistic rejects invalid name.
    """
    engine = _engine_with_fills()

    with pytest.raises(ValueError, match="Invalid statistic"):
        engine.portfolio.register_statistic(object())

    engine.dispose()
