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

import inspect

import pytest
from unit.adapters.example_modules import capture_data_tester_main
from unit.adapters.example_modules import capture_exec_tester_main
from unit.adapters.example_modules import load_example_module

from nautilus_trader.adapters.hyperliquid import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidDataClientFactory
from nautilus_trader.adapters.hyperliquid import HyperliquidEnvironment
from nautilus_trader.adapters.hyperliquid import HyperliquidExecutionClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidExecutionClientFactory
from nautilus_trader.adapters.hyperliquid import HyperliquidHttpClient
from nautilus_trader.adapters.hyperliquid import HyperliquidWebSocketClient
from nautilus_trader.adapters.hyperliquid import hyperliquid_resolve_execution_account_address
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import TraderId


HYPERLIQUID = "HYPERLIQUID"
SMOKE_PRIVATE_KEY = "0x0000000000000000000000000000000000000000000000000000000000000001"
hyperliquid_data_tester = load_example_module("hyperliquid", "data_tester")
hyperliquid_exec_tester = load_example_module("hyperliquid", "exec_tester")


def test_hyperliquid_factories_expose_python_names() -> None:
    assert HyperliquidDataClientFactory().name() == HYPERLIQUID
    assert HyperliquidExecutionClientFactory().name() == HYPERLIQUID


def test_resolve_execution_account_address_prefers_explicit_account() -> None:
    account_address = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

    resolved = hyperliquid_resolve_execution_account_address(
        vault_address=" 0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb ",
        account_address=f" {account_address} ",
        environment=HyperliquidEnvironment.MAINNET,
    )

    assert resolved == account_address


def test_resolve_execution_account_address_uses_vault_fallback() -> None:
    vault_address = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"

    resolved = hyperliquid_resolve_execution_account_address(
        vault_address=f" {vault_address} ",
        environment=HyperliquidEnvironment.MAINNET,
    )

    assert resolved == vault_address


def test_resolve_execution_account_address_rejects_invalid_vault() -> None:
    with pytest.raises(ValueError, match="Vault address must be 20 bytes of valid hex"):
        hyperliquid_resolve_execution_account_address(
            vault_address="0xinvalid",
            environment=HyperliquidEnvironment.MAINNET,
        )


@pytest.mark.asyncio
async def test_websocket_trading_binding_signatures_and_empty_cancel() -> None:
    client = HyperliquidWebSocketClient(
        url="ws://127.0.0.1:9/ws",
        environment=HyperliquidEnvironment.MAINNET,
    )
    signer = HyperliquidHttpClient(
        private_key=SMOKE_PRIVATE_KEY,
        environment=HyperliquidEnvironment.MAINNET,
        timeout_secs=1,
    )
    client.set_post_timeout(timeout_secs=1)

    result = await client.cancel_orders(signer=signer, cancels=[])
    signatures = {
        name: list(inspect.signature(getattr(client, name)).parameters)
        for name in (
            "submit_order",
            "submit_orders",
            "cancel_order",
            "cancel_orders",
            "modify_order",
        )
    }

    assert result == []
    assert signatures == {
        "submit_order": [
            "signer",
            "instrument_id",
            "client_order_id",
            "order_side",
            "order_type",
            "quantity",
            "time_in_force",
            "price",
            "trigger_price",
            "post_only",
            "reduce_only",
        ],
        "submit_orders": ["signer", "orders"],
        "cancel_order": ["signer", "instrument_id", "client_order_id", "venue_order_id"],
        "cancel_orders": ["signer", "cancels"],
        "modify_order": [
            "signer",
            "instrument_id",
            "venue_order_id",
            "order_side",
            "order_type",
            "price",
            "quantity",
            "trigger_price",
            "reduce_only",
            "post_only",
            "time_in_force",
            "client_order_id",
        ],
    }


def test_live_node_builder_accepts_hyperliquid_data_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("HYPERLIQUID-DATA-PYTEST-001", trader_id, Environment.LIVE)
        .add_data_client(
            None,
            HyperliquidDataClientFactory(),
            HyperliquidDataClientConfig(environment=HyperliquidEnvironment.MAINNET),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_live_node_builder_accepts_hyperliquid_exec_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")
    account_id = AccountId.from_str("HYPERLIQUID-001")

    node = (
        LiveNode.builder("HYPERLIQUID-EXEC-PYTEST-001", trader_id, Environment.LIVE)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            HyperliquidDataClientFactory(),
            HyperliquidDataClientConfig(environment=HyperliquidEnvironment.MAINNET),
        )
        .add_exec_client(
            None,
            HyperliquidExecutionClientFactory(),
            HyperliquidExecutionClientConfig(
                account_id,
                private_key=SMOKE_PRIVATE_KEY,
                environment=HyperliquidEnvironment.MAINNET,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_hyperliquid_data_tester_runs(monkeypatch: pytest.MonkeyPatch) -> None:
    captured = capture_data_tester_main(monkeypatch, hyperliquid_data_tester)
    kwargs = captured["data_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["subscribe_funding_rates"] is True
    assert captured["run_called"] is True


def test_hyperliquid_exec_tester_runs_live_orders(monkeypatch: pytest.MonkeyPatch) -> None:
    captured = capture_exec_tester_main(monkeypatch, hyperliquid_exec_tester)
    kwargs = captured["exec_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["dry_run"] is False
    assert kwargs["enable_limit_buys"] is True
    assert kwargs["enable_limit_sells"] is True
    assert captured["run_called"] is True
