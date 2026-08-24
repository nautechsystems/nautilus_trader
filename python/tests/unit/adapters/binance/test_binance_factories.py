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

from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.binance import BinanceDataClientFactory
from nautilus_trader.adapters.binance import BinanceEnvironment
from nautilus_trader.adapters.binance import BinanceExecutionClientConfig
from nautilus_trader.adapters.binance import BinanceExecutionClientFactory
from nautilus_trader.adapters.binance import BinanceProductType
from nautilus_trader.adapters.binance import BinanceSpotMarketDataMode
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import TraderId


BINANCE = "BINANCE"
SMOKE_API_KEY = "test_key"
SMOKE_API_SECRET = "test_secret"
binance_data_tester = load_example_module("binance", "data_tester")
binance_exec_tester = load_example_module("binance", "exec_tester")


def test_binance_factories_expose_python_names() -> None:
    assert BinanceDataClientFactory().name() == BINANCE
    assert BinanceExecutionClientFactory().name() == BINANCE


def test_binance_config_proxy_readback_exposes_presence_only() -> None:
    proxy_url = "http://user:password@proxy.example.test"
    data_config = BinanceDataClientConfig(proxy_url=proxy_url)
    data_config_without_proxy = BinanceDataClientConfig()
    exec_config = BinanceExecutionClientConfig(
        account_id=AccountId("BINANCE-001"),
        proxy_url=proxy_url,
    )

    assert data_config.has_proxy_url is True
    assert data_config_without_proxy.has_proxy_url is False
    assert exec_config.has_proxy_url is True
    assert not hasattr(data_config, "proxy_url")
    assert not hasattr(exec_config, "proxy_url")
    assert proxy_url not in repr(data_config)
    assert proxy_url not in repr(exec_config)


def test_live_node_builder_accepts_binance_data_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("BINANCE-DATA-PYTEST-001", trader_id, Environment.LIVE)
        .add_data_client(
            None,
            BinanceDataClientFactory(),
            BinanceDataClientConfig(
                product_type=BinanceProductType.SPOT,
                environment=BinanceEnvironment.LIVE,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_live_node_builder_accepts_binance_exec_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")
    account_id = AccountId.from_str("BINANCE-001")

    node = (
        LiveNode.builder("BINANCE-EXEC-PYTEST-001", trader_id, Environment.LIVE)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            BinanceDataClientFactory(),
            BinanceDataClientConfig(
                product_type=BinanceProductType.SPOT,
                environment=BinanceEnvironment.LIVE,
            ),
        )
        .add_exec_client(
            None,
            BinanceExecutionClientFactory(),
            BinanceExecutionClientConfig(
                account_id=account_id,
                product_type=BinanceProductType.SPOT,
                environment=BinanceEnvironment.LIVE,
                api_key=SMOKE_API_KEY,
                api_secret=SMOKE_API_SECRET,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_binance_data_tester_runs(monkeypatch: pytest.MonkeyPatch) -> None:
    captured = capture_data_tester_main(monkeypatch, binance_data_tester)
    kwargs = captured["data_tester_kwargs"]
    _, _, config = captured["data_client_args"]

    assert isinstance(kwargs, dict)
    assert isinstance(config, BinanceDataClientConfig)
    assert config.spot_market_data_mode == BinanceSpotMarketDataMode.Json
    assert kwargs["subscribe_book_at_interval"] is True
    assert captured["run_called"] is True


def test_binance_exec_tester_runs_live_orders(monkeypatch: pytest.MonkeyPatch) -> None:
    captured = capture_exec_tester_main(monkeypatch, binance_exec_tester)
    kwargs = captured["exec_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["dry_run"] is False
    assert kwargs["enable_limit_buys"] is True
    assert kwargs["enable_limit_sells"] is False  # Spot sells require holding the base currency
    assert captured["run_called"] is True
