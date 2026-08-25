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
Test factories behavior.
"""

import pytest
from unit.adapters.example_modules import load_example_module

from nautilus_trader.adapters.coinbase import COINBASE
from nautilus_trader.adapters.coinbase import CoinbaseDataClientConfig
from nautilus_trader.adapters.coinbase import CoinbaseDataClientFactory
from nautilus_trader.adapters.coinbase import CoinbaseEnvironment
from nautilus_trader.adapters.coinbase import CoinbaseExecutionClientConfig
from nautilus_trader.adapters.coinbase import CoinbaseExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import AccountType
from nautilus_trader.model import TraderId


SMOKE_API_KEY = "organizations/test-org/apiKeys/test-key"
SMOKE_API_SECRET = "test-pem-placeholder"
coinbase_exec_tester = load_example_module("coinbase", "exec_tester")


def test_coinbase_factories_expose_python_names() -> None:
    """
    Test coinbase factories expose python names.
    """
    data_factory = CoinbaseDataClientFactory()
    exec_factory = CoinbaseExecutionClientFactory()

    assert data_factory.name() == COINBASE
    assert exec_factory.name() == COINBASE


def test_live_node_builder_accepts_coinbase_data_factory() -> None:
    """
    Test live node builder accepts coinbase data factory.
    """
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("COINBASE-DATA-PYTEST-001", trader_id, Environment.LIVE)
        .add_data_client(
            None,
            CoinbaseDataClientFactory(),
            CoinbaseDataClientConfig(environment=CoinbaseEnvironment.LIVE),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_live_node_builder_accepts_coinbase_exec_factory() -> None:
    """
    Test live node builder accepts coinbase exec factory.
    """
    trader_id = TraderId.from_str("TESTER-001")
    account_id = AccountId.from_str("COINBASE-001")

    node = (
        LiveNode.builder("COINBASE-EXEC-PYTEST-001", trader_id, Environment.LIVE)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            CoinbaseDataClientFactory(),
            CoinbaseDataClientConfig(environment=CoinbaseEnvironment.LIVE),
        )
        .add_exec_client(
            None,
            CoinbaseExecutionClientFactory(),
            CoinbaseExecutionClientConfig(
                account_id=account_id,
                api_key=SMOKE_API_KEY,
                api_secret=SMOKE_API_SECRET,
                environment=CoinbaseEnvironment.LIVE,
                account_type=AccountType.CASH,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_coinbase_exec_tester_runs_live_orders(monkeypatch: pytest.MonkeyPatch) -> None:
    """
    Test coinbase exec tester runs live orders.
    """
    captured: dict[str, object] = {}

    class CapturingExecTesterConfig:
        """
        Collect capturing exec tester config tests.
        """

        def __init__(self, **kwargs: object) -> None:
            """
            Initialize the helper.
            """
            captured["exec_tester_kwargs"] = kwargs

    class CapturingNode:
        """
        Collect capturing node tests.
        """

        def add_builtin_strategy(self, type_name: str, config: object) -> None:
            """
            Add builtin strategy.
            """
            captured["strategy_type_name"] = type_name
            captured["strategy_config"] = config

        def run(self) -> None:
            """
            Run.
            """
            captured["run_called"] = True

    class CapturingBuilder:
        """
        Collect capturing builder tests.
        """

        def with_reconciliation(self, reconciliation: bool) -> "CapturingBuilder":
            """
            With reconciliation.
            """
            captured["reconciliation"] = reconciliation
            return self

        def with_risk_engine_config(self, config: LiveRiskEngineConfig) -> "CapturingBuilder":
            """
            With risk engine config.
            """
            captured["risk_engine_config"] = config
            return self

        def add_data_client(self, *args: object) -> "CapturingBuilder":
            """
            Add data client.
            """
            captured["data_client_args"] = args
            return self

        def add_exec_client(self, *args: object) -> "CapturingBuilder":
            """
            Add exec client.
            """
            captured["exec_client_args"] = args
            return self

        def build(self) -> CapturingNode:
            """
            Build.
            """
            return CapturingNode()

    class CapturingLiveNode:
        """
        Collect capturing live node tests.
        """

        @staticmethod
        def builder(name: str, trader_id: TraderId, environment: Environment) -> CapturingBuilder:
            """
            Builder.
            """
            captured["builder_args"] = (name, trader_id, environment)
            return CapturingBuilder()

    monkeypatch.setattr(coinbase_exec_tester, "ExecTesterConfig", CapturingExecTesterConfig)
    monkeypatch.setattr(coinbase_exec_tester, "LiveNode", CapturingLiveNode)

    coinbase_exec_tester.main()

    assert captured["strategy_type_name"] == "ExecTester"
    kwargs = captured["exec_tester_kwargs"]
    assert isinstance(kwargs, dict)
    assert kwargs["enable_limit_buys"] is True
    assert kwargs["enable_limit_sells"] is False  # Spot sells require holding the base currency
    assert kwargs["dry_run"] is False
    assert captured["run_called"] is True
