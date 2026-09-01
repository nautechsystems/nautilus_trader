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
Test deribit factories behavior.
"""

import pytest
from unit.adapters.example_modules import capture_actor_example_main
from unit.adapters.example_modules import capture_data_tester_main
from unit.adapters.example_modules import capture_exec_tester_main
from unit.adapters.example_modules import load_example_module

from nautilus_trader.adapters.deribit import DeribitDataClientConfig
from nautilus_trader.adapters.deribit import DeribitDataClientFactory
from nautilus_trader.adapters.deribit import DeribitEnvironment
from nautilus_trader.adapters.deribit import DeribitExecutionClientConfig
from nautilus_trader.adapters.deribit import DeribitExecutionClientFactory
from nautilus_trader.adapters.deribit import DeribitProductType
from nautilus_trader.common import Environment
from nautilus_trader.config import ImportableActorConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import TraderId


DERIBIT = "DERIBIT"
SMOKE_API_KEY = "test_key"
SMOKE_API_SECRET = "test_secret"
deribit_data_tester = load_example_module("deribit", "data_tester")
deribit_exec_tester = load_example_module("deribit", "exec_tester")
deribit_option_chain = load_example_module("deribit", "deribit_option_chain")


def test_deribit_factories_expose_python_names() -> None:
    """
    Test deribit factories expose python names.
    """
    assert DeribitDataClientFactory().name() == DERIBIT
    assert DeribitExecutionClientFactory().name() == DERIBIT


def test_live_node_builder_accepts_deribit_data_factory() -> None:
    """
    Test live node builder accepts deribit data factory.
    """
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("DERIBIT-DATA-PYTEST-001", trader_id, Environment.LIVE)
        .add_data_client(
            None,
            DeribitDataClientFactory(),
            DeribitDataClientConfig(
                product_types=[DeribitProductType.FUTURE],
                environment=DeribitEnvironment.TESTNET,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_live_node_builder_accepts_deribit_exec_factory() -> None:
    """
    Test live node builder accepts deribit exec factory.
    """
    trader_id = TraderId.from_str("TESTER-001")
    account_id = AccountId.from_str("DERIBIT-001")

    node = (
        LiveNode.builder("DERIBIT-EXEC-PYTEST-001", trader_id, Environment.LIVE)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            DeribitDataClientFactory(),
            DeribitDataClientConfig(
                product_types=[DeribitProductType.FUTURE],
                environment=DeribitEnvironment.TESTNET,
            ),
        )
        .add_exec_client(
            None,
            DeribitExecutionClientFactory(),
            DeribitExecutionClientConfig(
                account_id=account_id,
                product_types=[DeribitProductType.FUTURE],
                environment=DeribitEnvironment.TESTNET,
                api_key=SMOKE_API_KEY,
                api_secret=SMOKE_API_SECRET,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_deribit_data_tester_runs(monkeypatch: pytest.MonkeyPatch) -> None:
    """
    Test deribit data tester runs.
    """
    captured = capture_data_tester_main(monkeypatch, deribit_data_tester)
    kwargs = captured["data_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["subscribe_mark_prices"] is True
    assert captured["run_called"] is True


def test_deribit_exec_tester_runs_live_orders(monkeypatch: pytest.MonkeyPatch) -> None:
    """
    Test deribit exec tester runs live orders.
    """
    captured = capture_exec_tester_main(monkeypatch, deribit_exec_tester)
    kwargs = captured["exec_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["dry_run"] is False
    assert kwargs["enable_limit_buys"] is True
    assert kwargs["enable_limit_sells"] is True
    assert captured["run_called"] is True


def test_deribit_option_chain_runs(monkeypatch: pytest.MonkeyPatch) -> None:
    """
    Test deribit option chain runs.
    """
    captured = capture_actor_example_main(monkeypatch, deribit_option_chain)
    data_client_args = captured["data_client_args"]
    actor_config = captured["importable_actor_config"]

    assert isinstance(data_client_args, tuple)
    assert data_client_args[0] is None
    assert isinstance(data_client_args[1], DeribitDataClientFactory)
    assert isinstance(data_client_args[2], DeribitDataClientConfig)
    assert data_client_args[2].product_types == [DeribitProductType.OPTION]
    assert data_client_args[2].environment == DeribitEnvironment.MAINNET

    assert isinstance(actor_config, ImportableActorConfig)
    assert actor_config.actor_path == "deribit_option_chain:OptionChainTester"
    assert actor_config.config_path == "deribit_option_chain:OptionChainTesterConfig"
    assert actor_config.config == {
        "actor_id": "DERIBIT-OPTION-CHAIN-001",
        "underlying": "BTC",
        "strikes_above": 3,
        "strikes_below": 3,
        "snapshot_interval_ms": 2_000,
    }
    assert captured["run_called"] is True
