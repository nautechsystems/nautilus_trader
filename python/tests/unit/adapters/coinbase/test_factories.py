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

import sys

import pytest
from unit.adapters.example_modules import load_example_module

from nautilus_trader.adapters.coinbase import COINBASE
from nautilus_trader.adapters.coinbase import CoinbaseDataClientConfig
from nautilus_trader.adapters.coinbase import CoinbaseDataClientFactory
from nautilus_trader.adapters.coinbase import CoinbaseEnvironment
from nautilus_trader.adapters.coinbase import CoinbaseExecClientConfig
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
    trader_id = TraderId.from_str("TESTER-001")
    account_id = AccountId.from_str("COINBASE-001")

    data_factory = CoinbaseDataClientFactory()
    exec_factory = CoinbaseExecutionClientFactory(trader_id, account_id)

    assert data_factory.name() == COINBASE
    assert exec_factory.name() == COINBASE


def test_live_node_builder_accepts_coinbase_data_factory() -> None:
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
            CoinbaseExecutionClientFactory(trader_id, account_id),
            CoinbaseExecClientConfig(
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


@pytest.mark.parametrize(
    ("extra_args", "expected"),
    [
        ([], False),
        (["--limit-sells"], True),
    ],
)
def test_coinbase_exec_tester_limit_sells_are_explicit(
    monkeypatch: pytest.MonkeyPatch,
    extra_args: list[str],
    expected: bool,
) -> None:
    captured: dict[str, object] = {}

    class CapturingExecTesterConfig:
        def __init__(self, **kwargs: object) -> None:
            captured["exec_tester_kwargs"] = kwargs

    class CapturingNode:
        def add_native_strategy(self, type_name: str, config: object) -> None:
            captured["strategy_type_name"] = type_name
            captured["strategy_config"] = config

    class CapturingBuilder:
        def with_reconciliation(self, reconciliation: bool) -> "CapturingBuilder":
            captured["reconciliation"] = reconciliation
            return self

        def with_risk_engine_config(self, config: LiveRiskEngineConfig) -> "CapturingBuilder":
            captured["risk_engine_config"] = config
            return self

        def add_data_client(self, *args: object) -> "CapturingBuilder":
            captured["data_client_args"] = args
            return self

        def add_exec_client(self, *args: object) -> "CapturingBuilder":
            captured["exec_client_args"] = args
            return self

        def build(self) -> CapturingNode:
            return CapturingNode()

    class CapturingLiveNode:
        @staticmethod
        def builder(name: str, trader_id: TraderId, environment: Environment) -> CapturingBuilder:
            captured["builder_args"] = (name, trader_id, environment)
            return CapturingBuilder()

    monkeypatch.setattr(sys, "argv", ["exec_tester.py", "--live-orders", *extra_args])
    monkeypatch.setattr(coinbase_exec_tester, "ExecTesterConfig", CapturingExecTesterConfig)
    monkeypatch.setattr(coinbase_exec_tester, "LiveNode", CapturingLiveNode)

    coinbase_exec_tester.main()

    assert captured["strategy_type_name"] == "ExecTester"
    kwargs = captured["exec_tester_kwargs"]
    assert isinstance(kwargs, dict)
    assert kwargs["enable_limit_buys"] is True
    assert kwargs["enable_limit_sells"] is expected
    assert kwargs["dry_run"] is False
