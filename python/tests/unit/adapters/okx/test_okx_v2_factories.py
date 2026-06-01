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

from nautilus_trader.adapters.okx import OKX
from nautilus_trader.adapters.okx import OKXDataClientConfig
from nautilus_trader.adapters.okx import OKXDataClientFactory
from nautilus_trader.adapters.okx import OKXEnvironment
from nautilus_trader.adapters.okx import OKXExecClientConfig
from nautilus_trader.adapters.okx import OKXExecutionClientFactory
from nautilus_trader.adapters.okx import OKXInstrumentType
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import TraderId


SMOKE_API_KEY = "test_key"
SMOKE_API_SECRET = "test_secret"
SMOKE_API_PASSPHRASE = "test_passphrase"
okx_exec_tester = load_example_module("okx", "exec_tester")


def test_okx_factories_expose_python_v2_names() -> None:
    data_factory = OKXDataClientFactory()
    exec_factory = OKXExecutionClientFactory()

    assert data_factory.name() == OKX
    assert exec_factory.name() == OKX


def test_live_node_builder_accepts_okx_data_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("OKX-DATA-PYTEST-001", trader_id, Environment.LIVE)
        .add_data_client(
            None,
            OKXDataClientFactory(),
            OKXDataClientConfig(
                instrument_types=[OKXInstrumentType.SPOT],
                environment=OKXEnvironment.DEMO,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_live_node_builder_accepts_okx_exec_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")
    account_id = AccountId.from_str("OKX-001")

    node = (
        LiveNode.builder("OKX-EXEC-PYTEST-001", trader_id, Environment.LIVE)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            OKXDataClientFactory(),
            OKXDataClientConfig(
                instrument_types=[OKXInstrumentType.SPOT],
                environment=OKXEnvironment.DEMO,
            ),
        )
        .add_exec_client(
            None,
            OKXExecutionClientFactory(),
            OKXExecClientConfig(
                trader_id=trader_id,
                account_id=account_id,
                instrument_types=[OKXInstrumentType.SPOT],
                environment=OKXEnvironment.DEMO,
                api_key=SMOKE_API_KEY,
                api_secret=SMOKE_API_SECRET,
                api_passphrase=SMOKE_API_PASSPHRASE,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


@pytest.mark.parametrize(
    ("extra_args", "expected_buys", "expected_sells", "expected_dry_run"),
    [
        ([], False, False, True),
        (["--live-orders"], True, False, False),
        (["--live-orders", "--limit-sells"], True, True, False),
    ],
)
def test_okx_exec_tester_limit_sells_are_explicit(
    monkeypatch: pytest.MonkeyPatch,
    extra_args: list[str],
    expected_buys: bool,
    expected_sells: bool,
    expected_dry_run: bool,
) -> None:
    captured: dict[str, object] = {}

    class CapturingExecTesterConfig:
        def __init__(self, **kwargs: object) -> None:
            captured["exec_tester_kwargs"] = kwargs

    class CapturingNode:
        def add_native_strategy(self, config: object) -> None:
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

    monkeypatch.setattr(sys, "argv", ["exec_tester.py", *extra_args])
    monkeypatch.setattr(okx_exec_tester, "ExecTesterConfig", CapturingExecTesterConfig)
    monkeypatch.setattr(okx_exec_tester, "LiveNode", CapturingLiveNode)

    okx_exec_tester.main()

    kwargs = captured["exec_tester_kwargs"]
    assert isinstance(kwargs, dict)
    assert kwargs["enable_limit_buys"] is expected_buys
    assert kwargs["enable_limit_sells"] is expected_sells
    assert kwargs["dry_run"] is expected_dry_run
