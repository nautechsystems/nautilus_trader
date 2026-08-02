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

import msgspec
import pytest

from nautilus_trader.adapters.architect_ax import AxDataClientConfig
from nautilus_trader.adapters.architect_ax import AxExecClientConfig
from nautilus_trader.adapters.architect_ax import AxLiveDataClientFactory
from nautilus_trader.adapters.architect_ax import AxLiveExecClientFactory
from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.binance import BinanceExecClientConfig
from nautilus_trader.adapters.binance import BinanceLiveDataClientFactory
from nautilus_trader.adapters.binance import BinanceLiveExecClientFactory
from nautilus_trader.adapters.bitmex import BitmexDataClientConfig
from nautilus_trader.adapters.bitmex import BitmexExecClientConfig
from nautilus_trader.adapters.bitmex import BitmexLiveDataClientFactory
from nautilus_trader.adapters.bitmex import BitmexLiveExecClientFactory
from nautilus_trader.adapters.bybit import BybitDataClientConfig
from nautilus_trader.adapters.bybit import BybitExecClientConfig
from nautilus_trader.adapters.bybit import BybitLiveDataClientFactory
from nautilus_trader.adapters.bybit import BybitLiveExecClientFactory
from nautilus_trader.adapters.deribit import DeribitDataClientConfig
from nautilus_trader.adapters.deribit import DeribitExecClientConfig
from nautilus_trader.adapters.deribit import DeribitLiveDataClientFactory
from nautilus_trader.adapters.deribit import DeribitLiveExecClientFactory
from nautilus_trader.adapters.hyperliquid import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidExecClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidLiveDataClientFactory
from nautilus_trader.adapters.hyperliquid import HyperliquidLiveExecClientFactory
from nautilus_trader.adapters.kraken import KrakenDataClientConfig
from nautilus_trader.adapters.kraken import KrakenExecClientConfig
from nautilus_trader.adapters.kraken import KrakenLiveDataClientFactory
from nautilus_trader.adapters.kraken import KrakenLiveExecClientFactory
from nautilus_trader.adapters.okx import OKXDataClientConfig
from nautilus_trader.adapters.okx import OKXExecClientConfig
from nautilus_trader.adapters.okx import OKXLiveDataClientFactory
from nautilus_trader.adapters.okx import OKXLiveExecClientFactory
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.network import TransportBackend
from nautilus_trader.test_kit.functions import ensure_all_tasks_completed
from nautilus_trader.trading.strategy import Strategy


RUNTIME_ADAPTER_CASES = [
    (
        "AX",
        AxDataClientConfig,
        AxExecClientConfig,
        AxLiveDataClientFactory,
        AxLiveExecClientFactory,
    ),
    (
        "BINANCE",
        BinanceDataClientConfig,
        BinanceExecClientConfig,
        BinanceLiveDataClientFactory,
        BinanceLiveExecClientFactory,
    ),
    (
        "BITMEX",
        BitmexDataClientConfig,
        BitmexExecClientConfig,
        BitmexLiveDataClientFactory,
        BitmexLiveExecClientFactory,
    ),
    (
        "BYBIT",
        BybitDataClientConfig,
        BybitExecClientConfig,
        BybitLiveDataClientFactory,
        BybitLiveExecClientFactory,
    ),
    (
        "DERIBIT",
        DeribitDataClientConfig,
        DeribitExecClientConfig,
        DeribitLiveDataClientFactory,
        DeribitLiveExecClientFactory,
    ),
    (
        "HYPERLIQUID",
        HyperliquidDataClientConfig,
        HyperliquidExecClientConfig,
        HyperliquidLiveDataClientFactory,
        HyperliquidLiveExecClientFactory,
    ),
    (
        "KRAKEN",
        KrakenDataClientConfig,
        KrakenExecClientConfig,
        KrakenLiveDataClientFactory,
        KrakenLiveExecClientFactory,
    ),
    (
        "OKX",
        OKXDataClientConfig,
        OKXExecClientConfig,
        OKXLiveDataClientFactory,
        OKXLiveExecClientFactory,
    ),
]


class SmokeStrategy(Strategy):
    def __init__(self) -> None:
        super().__init__()
        self.started = False

    def on_start(self) -> None:
        self.started = True


def _set_required_env_vars(monkeypatch) -> None:
    for proxy_var in (
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "NO_PROXY",
        "no_proxy",
    ):
        monkeypatch.delenv(proxy_var, raising=False)

    monkeypatch.setenv("BINANCE_API_KEY", "SOME_API_KEY")
    monkeypatch.setenv("BINANCE_API_SECRET", "SOME_API_SECRET")
    monkeypatch.setenv("DERIBIT_API_KEY", "SOME_API_KEY")
    monkeypatch.setenv("DERIBIT_API_SECRET", "SOME_API_SECRET")
    monkeypatch.setenv("OKX_API_KEY", "SOME_API_KEY")
    monkeypatch.setenv("OKX_API_SECRET", "SOME_API_SECRET")
    monkeypatch.setenv("OKX_API_PASSPHRASE", "SOME_API_PASSPHRASE")
    monkeypatch.setenv("POLYMARKET_API_KEY", "SOME_API_KEY")
    monkeypatch.setenv("POLYMARKET_API_SECRET", "SOME_API_SECRET")
    monkeypatch.setenv("POLYMARKET_PASSPHRASE", "SOME_API_PASSPHRASE")
    monkeypatch.setenv("POLYMARKET_FUNDER", "0x1111111111111111111111111111111111111111")
    monkeypatch.setenv(
        "POLYMARKET_PK",
        "0x1111111111111111111111111111111111111111111111111111111111111111",
    )


def _raw_node_config(transport_backend: str) -> bytes:
    return msgspec.json.encode(
        {
            "environment": "live",
            "trader_id": "Test-111",
            "data_clients": {
                "BINANCE": {
                    "path": "nautilus_trader.adapters.binance.config:BinanceDataClientConfig",
                    "config": {
                        "instrument_provider": {
                            "load_all": False,
                        },
                        "transport_backend": transport_backend,
                    },
                },
            },
            "exec_clients": {
                "BINANCE": {
                    "path": "nautilus_trader.adapters.binance.config:BinanceExecClientConfig",
                    "config": {
                        "instrument_provider": {
                            "load_all": False,
                        },
                        "transport_backend": transport_backend,
                    },
                },
            },
        },
    )


class TestTradingNodeTransportBackend:
    def teardown(self):
        ensure_all_tasks_completed()

    @pytest.mark.parametrize(
        ("transport_backend", "expected_backend"),
        [
            ("TUNGSTENITE", TransportBackend.TUNGSTENITE),
            ("SOCKUDO", TransportBackend.SOCKUDO),
        ],
    )
    def test_node_config_parse_accepts_transport_backend(
        self,
        transport_backend: str,
        expected_backend: TransportBackend,
    ) -> None:
        config = TradingNodeConfig.parse(_raw_node_config(transport_backend))

        data_config = config.data_clients["BINANCE"].create()
        exec_config = config.exec_clients["BINANCE"].create()

        assert data_config.transport_backend == expected_backend
        assert exec_config.transport_backend == expected_backend

    @pytest.mark.parametrize(
        "transport_backend",
        [TransportBackend.TUNGSTENITE, TransportBackend.SOCKUDO],
    )
    @pytest.mark.parametrize(
        (
            "client_name",
            "data_config_cls",
            "exec_config_cls",
            "data_factory_cls",
            "exec_factory_cls",
        ),
        RUNTIME_ADAPTER_CASES,
    )
    def test_node_build_accepts_transport_backend_for_runtime_adapters(
        self,
        monkeypatch,
        event_loop_for_setup,
        transport_backend: TransportBackend,
        client_name: str,
        data_config_cls,
        exec_config_cls,
        data_factory_cls,
        exec_factory_cls,
    ) -> None:
        _set_required_env_vars(monkeypatch)

        config = TradingNodeConfig(
            trader_id="Test-111",
            data_clients={
                client_name: data_config_cls(transport_backend=transport_backend),
            },
            exec_clients={
                client_name: exec_config_cls(transport_backend=transport_backend),
            },
        )

        node = TradingNode(config=config, loop=event_loop_for_setup)
        node.add_data_client_factory(client_name, data_factory_cls)
        node.add_exec_client_factory(client_name, exec_factory_cls)
        node.build()

        assert node.is_built()

    @pytest.mark.parametrize(
        "transport_backend",
        [TransportBackend.TUNGSTENITE, TransportBackend.SOCKUDO],
    )
    def test_node_build_accepts_transport_backend_for_polymarket(
        self,
        monkeypatch,
        event_loop_for_setup,
        transport_backend: TransportBackend,
    ) -> None:
        from nautilus_trader.adapters.polymarket import PolymarketDataClientConfig
        from nautilus_trader.adapters.polymarket import PolymarketExecClientConfig
        from nautilus_trader.adapters.polymarket import PolymarketLiveDataClientFactory
        from nautilus_trader.adapters.polymarket import PolymarketLiveExecClientFactory

        _set_required_env_vars(monkeypatch)

        config = TradingNodeConfig(
            trader_id="Test-111",
            data_clients={
                "POLYMARKET": PolymarketDataClientConfig(transport_backend=transport_backend),
            },
            exec_clients={
                "POLYMARKET": PolymarketExecClientConfig(transport_backend=transport_backend),
            },
        )

        node = TradingNode(config=config, loop=event_loop_for_setup)
        node.add_data_client_factory("POLYMARKET", PolymarketLiveDataClientFactory)
        node.add_exec_client_factory("POLYMARKET", PolymarketLiveExecClientFactory)
        node.build()

        assert node.is_built()

    @pytest.mark.parametrize(
        "transport_backend",
        [TransportBackend.TUNGSTENITE, TransportBackend.SOCKUDO],
    )
    def test_python_strategy_can_be_added_to_trading_node(
        self,
        monkeypatch,
        event_loop_for_setup,
        transport_backend: TransportBackend,
    ) -> None:
        _set_required_env_vars(monkeypatch)

        node = TradingNode(
            config=TradingNodeConfig(
                trader_id="Test-111",
                data_clients={
                    "BINANCE": BinanceDataClientConfig(transport_backend=transport_backend),
                },
                exec_clients={
                    "BINANCE": BinanceExecClientConfig(transport_backend=transport_backend),
                },
            ),
            loop=event_loop_for_setup,
        )
        node.add_data_client_factory("BINANCE", BinanceLiveDataClientFactory)
        node.add_exec_client_factory("BINANCE", BinanceLiveExecClientFactory)
        node.build()

        strategy = SmokeStrategy()
        node.trader.add_strategy(strategy)

        assert node.is_built()
        assert strategy in node.trader.strategies()
