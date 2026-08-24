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

import pytest
from unit.adapters.example_modules import capture_actor_example_main
from unit.adapters.example_modules import capture_data_tester_main
from unit.adapters.example_modules import capture_exec_tester_main
from unit.adapters.example_modules import load_example_module

from nautilus_trader.adapters.bybit import BybitDataClientConfig
from nautilus_trader.adapters.bybit import BybitDataClientFactory
from nautilus_trader.adapters.bybit import BybitEnvironment
from nautilus_trader.adapters.bybit import BybitExecutionClientConfig
from nautilus_trader.adapters.bybit import BybitExecutionClientFactory
from nautilus_trader.adapters.bybit import BybitProductType
from nautilus_trader.common import Environment
from nautilus_trader.config import ImportableActorConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import TraderId


BYBIT = "BYBIT"
SMOKE_API_KEY = "test_key"
SMOKE_API_SECRET = "test_secret"
bybit_data_tester = load_example_module("bybit", "data_tester")
bybit_exec_tester = load_example_module("bybit", "exec_tester")
bybit_option_chain = load_example_module("bybit", "bybit_option_chain")


def test_bybit_factories_expose_python_names() -> None:
    assert BybitDataClientFactory().name() == BYBIT
    assert BybitExecutionClientFactory().name() == BYBIT


def test_live_node_builder_accepts_bybit_data_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("BYBIT-DATA-PYTEST-001", trader_id, Environment.LIVE)
        .add_data_client(
            None,
            BybitDataClientFactory(),
            BybitDataClientConfig(
                product_types=[BybitProductType.LINEAR],
                environment=BybitEnvironment.MAINNET,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_live_node_builder_accepts_bybit_exec_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")
    account_id = AccountId.from_str("BYBIT-001")

    node = (
        LiveNode.builder("BYBIT-EXEC-PYTEST-001", trader_id, Environment.LIVE)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            BybitDataClientFactory(),
            BybitDataClientConfig(
                product_types=[BybitProductType.LINEAR],
                environment=BybitEnvironment.MAINNET,
            ),
        )
        .add_exec_client(
            None,
            BybitExecutionClientFactory(),
            BybitExecutionClientConfig(
                product_types=[BybitProductType.LINEAR],
                environment=BybitEnvironment.MAINNET,
                api_key=SMOKE_API_KEY,
                api_secret=SMOKE_API_SECRET,
                account_id=account_id,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_bybit_data_tester_runs(monkeypatch: pytest.MonkeyPatch) -> None:
    captured = capture_data_tester_main(monkeypatch, bybit_data_tester)
    kwargs = captured["data_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["subscribe_funding_rates"] is True
    assert captured["run_called"] is True


def test_bybit_exec_tester_runs_live_orders(monkeypatch: pytest.MonkeyPatch) -> None:
    captured = capture_exec_tester_main(monkeypatch, bybit_exec_tester)
    kwargs = captured["exec_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["dry_run"] is False
    assert kwargs["enable_limit_buys"] is True
    assert kwargs["enable_limit_sells"] is True
    assert captured["run_called"] is True


def test_bybit_option_chain_runs(monkeypatch: pytest.MonkeyPatch) -> None:
    captured = capture_actor_example_main(monkeypatch, bybit_option_chain)
    data_client_args = captured["data_client_args"]
    actor_config = captured["importable_actor_config"]

    assert isinstance(data_client_args, tuple)
    assert data_client_args[0] is None
    assert isinstance(data_client_args[1], BybitDataClientFactory)
    assert isinstance(data_client_args[2], BybitDataClientConfig)
    assert data_client_args[2].product_types == [BybitProductType.OPTION]
    assert data_client_args[2].environment == BybitEnvironment.MAINNET

    assert isinstance(actor_config, ImportableActorConfig)
    assert actor_config.actor_path == "bybit_option_chain:OptionChainTester"
    assert actor_config.config_path == "bybit_option_chain:OptionChainTesterConfig"
    assert actor_config.config == {
        "actor_id": "BYBIT-OPTION-CHAIN-001",
        "underlying": "BTC",
        "strikes_above": 3,
        "strikes_below": 3,
        "snapshot_interval_ms": 5_000,
    }
    assert captured["run_called"] is True
