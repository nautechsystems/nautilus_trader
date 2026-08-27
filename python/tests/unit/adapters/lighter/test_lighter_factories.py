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
Test lighter factories behavior.
"""

import pytest
from unit.adapters.example_modules import load_example_module

from nautilus_trader.adapters import lighter
from nautilus_trader.adapters.lighter import LIGHTER
from nautilus_trader.adapters.lighter import LIGHTER_CLIENT_ID
from nautilus_trader.adapters.lighter import LIGHTER_ROBINHOOD
from nautilus_trader.adapters.lighter import LIGHTER_ROBINHOOD_CLIENT_ID
from nautilus_trader.adapters.lighter import LighterDataClientConfig
from nautilus_trader.adapters.lighter import LighterDataClientFactory
from nautilus_trader.adapters.lighter import LighterDeployment
from nautilus_trader.adapters.lighter import LighterEnvironment
from nautilus_trader.adapters.lighter import LighterExecutionClientConfig
from nautilus_trader.adapters.lighter import LighterExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import ClientId
from nautilus_trader.model import TraderId
from nautilus_trader.model import Venue


lighter_exec_tester = load_example_module("lighter", "exec_tester")


def test_lighter_facade_exports_integrator_revocation() -> None:
    """
    Test lighter facade exports integrator revocation.
    """
    assert "revoke_lighter_integrator" in lighter.__all__
    assert callable(lighter.revoke_lighter_integrator)


def test_lighter_factories_expose_python_names() -> None:
    """
    Test lighter factories expose python names.
    """
    data_factory = LighterDataClientFactory()
    exec_factory = LighterExecutionClientFactory()

    assert data_factory.name() == LIGHTER
    assert exec_factory.name() == LIGHTER


def test_lighter_configs_expose_deployment_and_custom_venue() -> None:
    """
    Test Lighter configs expose deployment and custom venue.
    """
    venue = Venue.from_str("LIGHTER_CUSTOM")
    account_id = AccountId.from_str("LIGHTER_CUSTOM-001")

    data_config = LighterDataClientConfig(
        deployment=LighterDeployment.ROBINHOOD,
        venue=venue,
    )
    exec_config = LighterExecutionClientConfig(
        account_id=account_id,
        deployment=LighterDeployment.ROBINHOOD,
        venue=venue,
    )

    assert LIGHTER_ROBINHOOD == "LIGHTER_ROBINHOOD"
    assert ClientId.from_str(LIGHTER) == LIGHTER_CLIENT_ID
    assert ClientId.from_str(LIGHTER_ROBINHOOD) == LIGHTER_ROBINHOOD_CLIENT_ID
    assert data_config.deployment == LighterDeployment.ROBINHOOD
    assert data_config.venue == venue
    assert exec_config.deployment == LighterDeployment.ROBINHOOD
    assert exec_config.venue == venue


def test_live_node_builder_accepts_lighter_data_factory() -> None:
    """
    Test live node builder accepts lighter data factory.
    """
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("LIGHTER-DATA-PYTEST-001", trader_id, Environment.LIVE)
        .add_data_client(
            None,
            LighterDataClientFactory(),
            LighterDataClientConfig(environment=LighterEnvironment.TESTNET),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_live_node_builder_accepts_lighter_exec_factory() -> None:
    """
    Test live node builder accepts lighter exec factory.
    """
    trader_id = TraderId.from_str("TESTER-001")
    account_id = AccountId.from_str("LIGHTER-001")

    node = (
        LiveNode.builder("LIGHTER-EXEC-PYTEST-001", trader_id, Environment.LIVE)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            LighterDataClientFactory(),
            LighterDataClientConfig(environment=LighterEnvironment.TESTNET),
        )
        .add_exec_client(
            None,
            LighterExecutionClientFactory(),
            LighterExecutionClientConfig(
                account_id=account_id,
                environment=LighterEnvironment.TESTNET,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_live_node_accepts_lighter_and_robinhood_clients() -> None:
    """
    Test live node accepts Lighter and Robinhood data and execution clients together.
    """
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("LIGHTER-DUAL-EXEC-PYTEST-001", trader_id, Environment.LIVE)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .with_reconciliation(False)
        .add_data_client(
            LIGHTER,
            LighterDataClientFactory(),
            LighterDataClientConfig(environment=LighterEnvironment.TESTNET),
        )
        .add_data_client(
            LIGHTER_ROBINHOOD,
            LighterDataClientFactory(),
            LighterDataClientConfig(
                environment=LighterEnvironment.TESTNET,
                deployment=LighterDeployment.ROBINHOOD,
            ),
        )
        .add_exec_client(
            LIGHTER,
            LighterExecutionClientFactory(),
            LighterExecutionClientConfig(
                account_id=AccountId.from_str("LIGHTER-001"),
                environment=LighterEnvironment.TESTNET,
            ),
        )
        .add_exec_client(
            LIGHTER_ROBINHOOD,
            LighterExecutionClientFactory(),
            LighterExecutionClientConfig(
                account_id=AccountId.from_str("LIGHTER_ROBINHOOD-001"),
                environment=LighterEnvironment.TESTNET,
                deployment=LighterDeployment.ROBINHOOD,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_lighter_exec_tester_runs_live_orders_by_default(  # noqa: C901
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test lighter exec tester runs live orders by default.
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
            captured["node_ran"] = True

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

        def with_exec_engine_config(self, config: object) -> "CapturingBuilder":
            """
            With exec engine config.
            """
            captured["exec_engine_config"] = config
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

    monkeypatch.setattr(lighter_exec_tester, "ExecTesterConfig", CapturingExecTesterConfig)
    monkeypatch.setattr(lighter_exec_tester, "LiveNode", CapturingLiveNode)

    lighter_exec_tester.main()

    assert captured["strategy_type_name"] == "ExecTester"
    assert captured["reconciliation"] is True
    assert captured["node_ran"] is True
    data_client_args = captured["data_client_args"]
    exec_client_args = captured["exec_client_args"]
    assert isinstance(data_client_args, tuple)
    assert isinstance(exec_client_args, tuple)
    assert data_client_args[0] == lighter_exec_tester.VENUE
    assert exec_client_args[0] == lighter_exec_tester.VENUE
    kwargs = captured["exec_tester_kwargs"]
    assert isinstance(kwargs, dict)
    assert kwargs["enable_limit_buys"] is True
    assert kwargs["enable_limit_sells"] is True
    assert kwargs["use_post_only"] is True
    assert kwargs["dry_run"] is False
