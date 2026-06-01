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
from unit.adapters.example_modules import capture_data_tester_main
from unit.adapters.example_modules import capture_exec_tester_main
from unit.adapters.example_modules import load_example_module

from nautilus_trader.adapters.dydx import DydxDataClientConfig
from nautilus_trader.adapters.dydx import DydxDataClientFactory
from nautilus_trader.adapters.dydx import DydxExecClientConfig
from nautilus_trader.adapters.dydx import DydxExecutionClientFactory
from nautilus_trader.adapters.dydx import DydxNetwork
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import TraderId


DYDX = "DYDX"
SMOKE_PRIVATE_KEY = "0x0000000000000000000000000000000000000000000000000000000000000001"
SMOKE_WALLET_ADDRESS = "dydx1abc123"
dydx_data_tester = load_example_module("dydx", "data_tester")
dydx_exec_tester = load_example_module("dydx", "exec_tester")


def test_dydx_factories_expose_python_names() -> None:
    assert DydxDataClientFactory().name() == DYDX
    assert DydxExecutionClientFactory().name() == DYDX


def test_live_node_builder_accepts_dydx_data_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("DYDX-DATA-PYTEST-001", trader_id, Environment.LIVE)
        .add_data_client(
            None,
            DydxDataClientFactory(),
            DydxDataClientConfig(network=DydxNetwork.MAINNET),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_live_node_builder_accepts_dydx_exec_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")
    account_id = AccountId.from_str("DYDX-001")

    node = (
        LiveNode.builder("DYDX-EXEC-PYTEST-001", trader_id, Environment.LIVE)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            DydxDataClientFactory(),
            DydxDataClientConfig(network=DydxNetwork.MAINNET),
        )
        .add_exec_client(
            None,
            DydxExecutionClientFactory(),
            DydxExecClientConfig(
                trader_id=trader_id,
                account_id=account_id,
                network=DydxNetwork.MAINNET,
                private_key=SMOKE_PRIVATE_KEY,
                wallet_address=SMOKE_WALLET_ADDRESS,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_dydx_data_tester_builds_offline(monkeypatch: pytest.MonkeyPatch) -> None:
    captured = capture_data_tester_main(monkeypatch, dydx_data_tester, [])
    kwargs = captured["data_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["subscribe_book_at_interval"] is True
    assert "run_called" not in captured


@pytest.mark.parametrize(
    ("extra_args", "expected_dry_run", "expected_limit_sells"),
    [
        ([], True, False),
        (["--live-orders", "--limit-sells"], False, True),
    ],
)
def test_dydx_exec_tester_gates_live_orders(
    monkeypatch: pytest.MonkeyPatch,
    extra_args: list[str],
    expected_dry_run: bool,
    expected_limit_sells: bool,
) -> None:
    captured = capture_exec_tester_main(monkeypatch, dydx_exec_tester, extra_args)
    kwargs = captured["exec_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["dry_run"] is expected_dry_run
    assert kwargs["enable_limit_sells"] is expected_limit_sells
    assert "run_called" not in captured
