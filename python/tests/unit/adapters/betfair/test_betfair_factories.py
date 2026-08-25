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

from types import ModuleType

import pytest
from unit.adapters.example_modules import capture_data_tester_main
from unit.adapters.example_modules import capture_exec_tester_main
from unit.adapters.example_modules import load_example_module

from nautilus_trader.adapters.betfair import BetfairDataClientConfig
from nautilus_trader.adapters.betfair import BetfairDataClientFactory
from nautilus_trader.adapters.betfair import BetfairExecutionClientConfig
from nautilus_trader.adapters.betfair import BetfairExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId


BETFAIR = "BETFAIR"
SMOKE_USERNAME = "test_user"
SMOKE_PASSWORD = "test_password"
SMOKE_APP_KEY = "test_app_key"
betfair_data_tester = load_example_module("betfair", "data_tester")
betfair_exec_tester = load_example_module("betfair", "exec_tester")


def test_betfair_factories_expose_python_names() -> None:
    assert BetfairDataClientFactory().name() == BETFAIR
    assert BetfairExecutionClientFactory().name() == BETFAIR


def test_live_node_builder_accepts_betfair_data_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("BETFAIR-DATA-PYTEST-001", trader_id, Environment.LIVE)
        .add_data_client(
            None,
            BetfairDataClientFactory(),
            BetfairDataClientConfig(
                account_currency="GBP",
                username=SMOKE_USERNAME,
                password=SMOKE_PASSWORD,
                app_key=SMOKE_APP_KEY,
                market_ids=["1.234567890"],
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_live_node_builder_accepts_betfair_exec_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")
    account_id = AccountId.from_str("BETFAIR-001")

    node = (
        LiveNode.builder("BETFAIR-EXEC-PYTEST-001", trader_id, Environment.LIVE)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            BetfairDataClientFactory(),
            BetfairDataClientConfig(
                account_currency="GBP",
                username=SMOKE_USERNAME,
                password=SMOKE_PASSWORD,
                app_key=SMOKE_APP_KEY,
                market_ids=["1.234567890"],
            ),
        )
        .add_exec_client(
            None,
            BetfairExecutionClientFactory(),
            BetfairExecutionClientConfig(
                account_id=account_id,
                account_currency="GBP",
                username=SMOKE_USERNAME,
                password=SMOKE_PASSWORD,
                app_key=SMOKE_APP_KEY,
                stream_market_ids_filter=["1.234567890"],
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


@pytest.mark.parametrize("module", [betfair_data_tester, betfair_exec_tester])
def test_betfair_examples_require_market_target(
    monkeypatch: pytest.MonkeyPatch,
    module: ModuleType,
) -> None:
    monkeypatch.delenv("BETFAIR_MARKET_ID", raising=False)
    monkeypatch.delenv("BETFAIR_INSTRUMENT_ID", raising=False)

    with pytest.raises(SystemExit, match="BETFAIR_MARKET_ID must be set"):
        module.main()


@pytest.mark.parametrize("module", [betfair_data_tester, betfair_exec_tester])
def test_betfair_examples_require_instrument_target(
    monkeypatch: pytest.MonkeyPatch,
    module: ModuleType,
) -> None:
    monkeypatch.setenv("BETFAIR_MARKET_ID", "1.234567890")
    monkeypatch.delenv("BETFAIR_INSTRUMENT_ID", raising=False)

    with pytest.raises(SystemExit, match="BETFAIR_INSTRUMENT_ID must be set"):
        module.main()


def test_betfair_data_tester_runs(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("BETFAIR_MARKET_ID", "1.234567890")
    monkeypatch.setenv("BETFAIR_INSTRUMENT_ID", "1.234567890-123456.BETFAIR")
    captured = capture_data_tester_main(monkeypatch, betfair_data_tester)
    kwargs = captured["data_tester_kwargs"]
    _, _, config = captured["data_client_args"]

    assert isinstance(kwargs, dict)
    assert isinstance(config, BetfairDataClientConfig)
    assert config.market_ids == ["1.234567890"]
    assert kwargs["instrument_ids"] == [
        InstrumentId.from_str("1.234567890-123456.BETFAIR"),
    ]
    assert kwargs["subscribe_book_deltas"] is True
    assert captured["run_called"] is True


def test_betfair_exec_tester_runs_live_orders(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("BETFAIR_MARKET_ID", "1.234567890")
    monkeypatch.setenv("BETFAIR_INSTRUMENT_ID", "1.234567890-123456.BETFAIR")
    captured = capture_exec_tester_main(monkeypatch, betfair_exec_tester)
    kwargs = captured["exec_tester_kwargs"]
    _, _, data_config = captured["data_client_args"]
    _, _, exec_config = captured["exec_client_args"]

    assert isinstance(kwargs, dict)
    assert isinstance(data_config, BetfairDataClientConfig)
    assert data_config.market_ids == ["1.234567890"]
    assert isinstance(exec_config, BetfairExecutionClientConfig)
    assert exec_config.stream_market_ids_filter == ["1.234567890"]
    assert kwargs["instrument_id"] == InstrumentId.from_str("1.234567890-123456.BETFAIR")
    assert kwargs["dry_run"] is False
    assert kwargs["enable_limit_buys"] is False
    assert kwargs["enable_limit_sells"] is False
    assert captured["run_called"] is True
