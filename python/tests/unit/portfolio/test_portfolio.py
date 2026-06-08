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

from __future__ import annotations

import importlib
import subprocess
import sys
import textwrap


def test_portfolio_public_module_exports_pyo3_classes():
    portfolio = importlib.import_module("nautilus_trader.portfolio")
    native_portfolio = importlib.import_module("nautilus_trader._libnautilus.portfolio")

    assert portfolio.Portfolio is native_portfolio.Portfolio
    assert portfolio.PortfolioConfig is native_portfolio.PortfolioConfig
    assert portfolio.Portfolio.__name__ == "Portfolio"
    assert portfolio.PortfolioConfig.__name__ == "PortfolioConfig"


def test_portfolio_public_module_sets_runtime_module_names():
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


def test_live_reexports_portfolio_config_for_compatibility():
    from nautilus_trader.backtest import BacktestEngineConfig
    from nautilus_trader.live import LiveNodeConfig
    from nautilus_trader.live import PortfolioConfig as LivePortfolioConfig
    from nautilus_trader.portfolio import PortfolioConfig

    live_config = LiveNodeConfig(portfolio=LivePortfolioConfig())
    backtest_config = BacktestEngineConfig(portfolio=PortfolioConfig())

    assert LivePortfolioConfig is PortfolioConfig
    assert isinstance(live_config, LiveNodeConfig)
    assert isinstance(backtest_config.portfolio, PortfolioConfig)
