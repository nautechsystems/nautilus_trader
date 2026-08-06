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

from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersDataClientFactory
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersExecClientConfig
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersExecutionClientFactory
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersInstrumentProvider
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.adapters.interactive_brokers import MarketDataType
from nautilus_trader.adapters.interactive_brokers import SymbologyMethod
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId


IB = "IB"
ib_data_tester = load_example_module("interactive_brokers", "data_tester")
ib_exec_tester = load_example_module("interactive_brokers", "exec_tester")


def test_interactive_brokers_factories_expose_python_names() -> None:
    trader_id = TraderId.from_str("TESTER-001")
    account_id = AccountId.from_str("IB-001")

    assert InteractiveBrokersDataClientFactory().name() == IB
    assert InteractiveBrokersExecutionClientFactory(trader_id, account_id).name() == IB


def test_interactive_brokers_instrument_provider_config_and_empty_surface() -> None:
    instrument_id = InstrumentId.from_str("AAPL.NASDAQ")
    contract = {"secType": "STK", "symbol": "MSFT", "exchange": "SMART"}
    config = InteractiveBrokersInstrumentProviderConfig(
        symbology_method=SymbologyMethod.RAW,
        load_ids={instrument_id},
        load_contracts=[contract],
        min_expiry_days=2,
        max_expiry_days=30,
        build_options_chain=True,
        build_futures_chain=False,
        cache_validity_days=7,
        convert_exchange_to_mic_venue=True,
        symbol_to_mic_venue={"AAPL": "XNAS"},
        filter_sec_types={"OPT", "STK"},
        filter_callable="package.module:filter_instrument",
        cache_path="cache/instruments.json",
    )
    provider = InteractiveBrokersInstrumentProvider(config)

    assert config.symbology_method == SymbologyMethod.RAW
    assert config.load_ids == {instrument_id}
    assert config.load_contracts == [contract]
    assert config.min_expiry_days == 2
    assert config.max_expiry_days == 30
    assert config.build_options_chain is True
    assert config.build_futures_chain is False
    assert config.cache_validity_days == 7
    assert config.convert_exchange_to_mic_venue is True
    assert config.symbol_to_mic_venue == {"AAPL": "XNAS"}
    assert set(config.filter_sec_types) == {"OPT", "STK"}
    assert config.filter_callable == "package.module:filter_instrument"
    assert config.cache_path == "cache/instruments.json"
    assert provider.count() == 0
    assert provider.get_all() == []
    assert provider.find(instrument_id) is None


def test_live_node_builder_accepts_interactive_brokers_data_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("IB-DATA-PYTEST-001", trader_id, Environment.LIVE)
        .add_data_client(
            None,
            InteractiveBrokersDataClientFactory(),
            InteractiveBrokersDataClientConfig(
                client_id=101,
                market_data_type=MarketDataType.DELAYED,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_live_node_builder_accepts_interactive_brokers_exec_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")
    account_id = AccountId.from_str("IB-001")

    node = (
        LiveNode.builder("IB-EXEC-PYTEST-001", trader_id, Environment.LIVE)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            InteractiveBrokersDataClientFactory(),
            InteractiveBrokersDataClientConfig(
                client_id=101,
                market_data_type=MarketDataType.DELAYED,
            ),
        )
        .add_exec_client(
            None,
            InteractiveBrokersExecutionClientFactory(trader_id, account_id),
            InteractiveBrokersExecClientConfig(client_id=101, account_id="U1234567"),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_interactive_brokers_data_tester_builds_offline(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured = capture_data_tester_main(monkeypatch, ib_data_tester, [])
    kwargs = captured["data_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["request_instruments"] is True
    assert "run_called" not in captured


@pytest.mark.parametrize(
    ("extra_args", "expected_dry_run", "expected_limit_sells"),
    [
        ([], True, False),
        (["--live-orders", "--limit-sells"], False, True),
    ],
)
def test_interactive_brokers_exec_tester_gates_live_orders(
    monkeypatch: pytest.MonkeyPatch,
    extra_args: list[str],
    expected_dry_run: bool,
    expected_limit_sells: bool,
) -> None:
    captured = capture_exec_tester_main(monkeypatch, ib_exec_tester, extra_args)
    kwargs = captured["exec_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["dry_run"] is expected_dry_run
    assert kwargs["enable_limit_sells"] is expected_limit_sells
    assert "run_called" not in captured
