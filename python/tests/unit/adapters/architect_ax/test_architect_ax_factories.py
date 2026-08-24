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

from decimal import Decimal

import pytest
from unit.adapters.example_modules import capture_data_tester_main
from unit.adapters.example_modules import capture_exec_tester_main
from unit.adapters.example_modules import load_example_module

from nautilus_trader.adapters.architect_ax import AX
from nautilus_trader.adapters.architect_ax import AxDataClientConfig
from nautilus_trader.adapters.architect_ax import AxDataClientFactory
from nautilus_trader.adapters.architect_ax import AxEnvironment
from nautilus_trader.adapters.architect_ax import AxExecutionClientConfig
from nautilus_trader.adapters.architect_ax import AxExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import BarType
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TraderId


SMOKE_API_KEY = "test_key"
SMOKE_API_SECRET = "test_secret"
architect_ax_data_tester = load_example_module("architect_ax", "data_tester")
architect_ax_exec_tester = load_example_module("architect_ax", "exec_tester")


def test_architect_ax_factories_expose_python_names() -> None:
    assert AxDataClientFactory().name() == AX
    assert AxExecutionClientFactory().name() == AX


def test_live_node_builder_accepts_architect_ax_data_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("AX-DATA-PYTEST-001", trader_id, Environment.SANDBOX)
        .add_data_client(
            None,
            AxDataClientFactory(),
            AxDataClientConfig(environment=AxEnvironment.SANDBOX),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.SANDBOX


def test_live_node_builder_accepts_architect_ax_exec_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")
    account_id = AccountId.from_str("AX-001")

    node = (
        LiveNode.builder("AX-EXEC-PYTEST-001", trader_id, Environment.SANDBOX)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            AxDataClientFactory(),
            AxDataClientConfig(environment=AxEnvironment.SANDBOX),
        )
        .add_exec_client(
            None,
            AxExecutionClientFactory(),
            AxExecutionClientConfig(
                account_id=account_id,
                api_key=SMOKE_API_KEY,
                api_secret=SMOKE_API_SECRET,
                environment=AxEnvironment.SANDBOX,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.SANDBOX


def test_architect_ax_data_tester_runs(monkeypatch: pytest.MonkeyPatch) -> None:
    captured = capture_data_tester_main(monkeypatch, architect_ax_data_tester)
    kwargs = captured["data_tester_kwargs"]
    data_client_config = captured["data_client_args"][2]

    assert isinstance(kwargs, dict)
    assert captured["builder_args"] == (
        "AX-DATA-TESTER-001",
        TraderId.from_str("TESTER-001"),
        Environment.LIVE,
    )
    assert isinstance(data_client_config, AxDataClientConfig)
    assert data_client_config.environment == AxEnvironment.SANDBOX
    assert kwargs == {
        "client_id": ClientId.from_str(AX),
        "instrument_ids": [InstrumentId.from_str("XAG-PERP.AX")],
        "bar_types": [BarType.from_str("XAG-PERP.AX-1-MINUTE-LAST-EXTERNAL")],
        "subscribe_book_deltas": True,
        "subscribe_quotes": True,
        "subscribe_trades": True,
        "subscribe_mark_prices": True,
        "subscribe_funding_rates": True,
        "subscribe_bars": True,
        "subscribe_instrument_status": True,
        "request_instruments": True,
        "request_trades": True,
        "request_bars": True,
        "request_book_snapshot": True,
        "request_funding_rates": True,
        "manage_book": True,
        "log_data": True,
        "stats_interval_secs": 0,
    }
    assert captured["delay_post_stop_secs"] == 5
    assert captured["run_called"] is True


def test_architect_ax_exec_tester_runs_live_orders(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured = capture_exec_tester_main(monkeypatch, architect_ax_exec_tester)
    kwargs = captured["exec_tester_kwargs"]
    exec_engine_config = captured["exec_engine_config"]
    exec_engine_repr = repr(exec_engine_config)
    data_client_config = captured["data_client_args"][2]
    exec_client_config = captured["exec_client_args"][2]

    assert isinstance(kwargs, dict)
    assert captured["builder_args"] == (
        "AX-EXEC-TESTER-001",
        TraderId.from_str("TESTER-001"),
        Environment.LIVE,
    )
    assert isinstance(data_client_config, AxDataClientConfig)
    assert data_client_config.environment == AxEnvironment.SANDBOX
    assert isinstance(exec_client_config, AxExecutionClientConfig)
    assert exec_client_config.account_id == AccountId.from_str("AX-001")
    assert exec_client_config.environment == AxEnvironment.SANDBOX
    assert 'reconciliation_instrument_ids: Some(["XAG-PERP.AX"])' in exec_engine_repr
    assert "open_check_interval_secs: Some(10.0)" in exec_engine_repr
    assert "position_check_interval_secs: Some(30.0)" in exec_engine_repr
    assert captured["reconciliation"] is True
    assert captured["timeout_disconnection_secs"] == 10
    assert captured["delay_post_stop_secs"] == 5
    assert kwargs["strategy_id"] == StrategyId.from_str("EXEC_TESTER-001")
    assert kwargs["instrument_id"] == InstrumentId.from_str("XAG-PERP.AX")
    assert kwargs["client_id"] == ClientId.from_str(AX)
    assert kwargs["external_order_claims"] == [InstrumentId.from_str("XAG-PERP.AX")]
    assert kwargs["order_qty"] == Quantity.from_str("1")
    assert kwargs["subscribe_quotes"] is True
    assert kwargs["subscribe_trades"] is True
    assert kwargs["open_position_on_start_qty"] == Decimal(1)
    assert kwargs["open_position_on_first_quote"] is True
    assert kwargs["open_position_time_in_force"] == TimeInForce.IOC
    assert kwargs["dry_run"] is False
    assert kwargs["enable_limit_buys"] is True
    assert kwargs["enable_limit_sells"] is True
    assert kwargs["tob_offset_ticks"] == 1
    assert kwargs["use_post_only"] is True
    assert kwargs["cancel_orders_on_stop"] is True
    assert kwargs["close_positions_on_stop"] is True
    assert kwargs["reduce_only_on_stop"] is False
    assert captured["run_called"] is True
