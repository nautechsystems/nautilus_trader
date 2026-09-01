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
Test kraken factories behavior.
"""

import pytest
from unit.adapters.example_modules import capture_data_tester_main
from unit.adapters.example_modules import capture_exec_tester_main
from unit.adapters.example_modules import load_example_module

from nautilus_trader.adapters.kraken import KrakenDataClientConfig
from nautilus_trader.adapters.kraken import KrakenDataClientFactory
from nautilus_trader.adapters.kraken import KrakenExecutionClientConfig
from nautilus_trader.adapters.kraken import KrakenExecutionClientFactory
from nautilus_trader.adapters.kraken import KrakenProductType
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import TraderId


KRAKEN = "KRAKEN"
SMOKE_API_KEY = "test_key"
SMOKE_API_SECRET = "test_secret"
kraken_data_tester = load_example_module("kraken", "data_tester")
kraken_exec_tester = load_example_module("kraken", "exec_tester")


def test_kraken_factories_expose_python_names() -> None:
    """
    Test kraken factories expose python names.
    """
    assert KrakenDataClientFactory().name() == KRAKEN
    assert KrakenExecutionClientFactory().name() == KRAKEN


def test_live_node_builder_accepts_kraken_data_factory() -> None:
    """
    Test live node builder accepts kraken data factory.
    """
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("KRAKEN-DATA-PYTEST-001", trader_id, Environment.LIVE)
        .add_data_client(
            None,
            KrakenDataClientFactory(),
            KrakenDataClientConfig(product_type=KrakenProductType.FUTURES),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_live_node_builder_accepts_kraken_exec_factory() -> None:
    """
    Test live node builder accepts kraken exec factory.
    """
    trader_id = TraderId.from_str("TESTER-001")
    account_id = AccountId.from_str("KRAKEN-001")

    node = (
        LiveNode.builder("KRAKEN-EXEC-PYTEST-001", trader_id, Environment.LIVE)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            KrakenDataClientFactory(),
            KrakenDataClientConfig(product_type=KrakenProductType.FUTURES),
        )
        .add_exec_client(
            None,
            KrakenExecutionClientFactory(),
            KrakenExecutionClientConfig(
                account_id=account_id,
                api_key=SMOKE_API_KEY,
                api_secret=SMOKE_API_SECRET,
                product_type=KrakenProductType.FUTURES,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_kraken_data_tester_runs(monkeypatch: pytest.MonkeyPatch) -> None:
    """
    Test kraken data tester runs.
    """
    captured = capture_data_tester_main(monkeypatch, kraken_data_tester)
    kwargs = captured["data_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["subscribe_index_prices"] is True
    assert captured["run_called"] is True


def test_kraken_exec_tester_runs_live_orders(monkeypatch: pytest.MonkeyPatch) -> None:
    """
    Test kraken exec tester runs live orders.
    """
    monkeypatch.setenv("KRAKEN_API_KEY", SMOKE_API_KEY)
    monkeypatch.setenv("KRAKEN_API_SECRET", SMOKE_API_SECRET)
    captured = capture_exec_tester_main(monkeypatch, kraken_exec_tester)
    kwargs = captured["exec_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["dry_run"] is False
    assert kwargs["enable_limit_buys"] is True
    assert kwargs["enable_limit_sells"] is True
    assert captured["run_called"] is True


def test_kraken_exec_tester_requires_credentials(monkeypatch: pytest.MonkeyPatch) -> None:
    """
    Test kraken exec tester requires credentials.
    """
    monkeypatch.delenv("KRAKEN_API_KEY", raising=False)
    monkeypatch.delenv("KRAKEN_API_SECRET", raising=False)

    with pytest.raises(SystemExit, match="KRAKEN_API_KEY and KRAKEN_API_SECRET must be set"):
        kraken_exec_tester.main()
