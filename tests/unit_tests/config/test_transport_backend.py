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

import msgspec
import pytest

from nautilus_trader.adapters.architect_ax import AxDataClientConfig
from nautilus_trader.adapters.architect_ax import AxExecClientConfig
from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.binance import BinanceExecClientConfig
from nautilus_trader.adapters.bitmex import BitmexDataClientConfig
from nautilus_trader.adapters.bitmex import BitmexExecClientConfig
from nautilus_trader.adapters.bybit import BybitDataClientConfig
from nautilus_trader.adapters.bybit import BybitExecClientConfig
from nautilus_trader.adapters.deribit import DeribitDataClientConfig
from nautilus_trader.adapters.deribit import DeribitExecClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidExecClientConfig
from nautilus_trader.adapters.kraken import KrakenDataClientConfig
from nautilus_trader.adapters.kraken import KrakenExecClientConfig
from nautilus_trader.adapters.okx import OKXDataClientConfig
from nautilus_trader.adapters.okx import OKXExecClientConfig
from nautilus_trader.common.config import pyo3_config_json
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.network import TransportBackend
from nautilus_trader.network import WebSocketConfig


@pytest.fixture(autouse=True)
def clear_proxy_env(monkeypatch):
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


@pytest.mark.parametrize(
    "config_cls",
    [
        AxDataClientConfig,
        AxExecClientConfig,
        BinanceDataClientConfig,
        BinanceExecClientConfig,
        BitmexDataClientConfig,
        BitmexExecClientConfig,
        BybitDataClientConfig,
        BybitExecClientConfig,
        DeribitDataClientConfig,
        DeribitExecClientConfig,
        HyperliquidDataClientConfig,
        HyperliquidExecClientConfig,
        KrakenDataClientConfig,
        KrakenExecClientConfig,
        OKXDataClientConfig,
        OKXExecClientConfig,
    ],
)
def test_adapter_configs_round_trip_transport_backend(config_cls):
    config = config_cls(transport_backend=TransportBackend.TUNGSTENITE)

    assert config.transport_backend == TransportBackend.TUNGSTENITE

    payload = msgspec.json.decode(pyo3_config_json(config))
    assert payload["transport_backend"] == "TUNGSTENITE"

    parsed = config_cls.parse(msgspec.json.encode(payload))
    assert parsed.transport_backend == TransportBackend.TUNGSTENITE


def test_polymarket_runtime_configs_round_trip_transport_backend() -> None:
    from nautilus_trader.adapters.polymarket import PolymarketDataClientConfig
    from nautilus_trader.adapters.polymarket import PolymarketExecClientConfig

    for config_cls in (PolymarketDataClientConfig, PolymarketExecClientConfig):
        config = config_cls(transport_backend=TransportBackend.TUNGSTENITE)

        assert config.transport_backend == TransportBackend.TUNGSTENITE

        payload = msgspec.json.decode(pyo3_config_json(config))
        assert payload["transport_backend"] == "TUNGSTENITE"

        parsed = config_cls.parse(msgspec.json.encode(payload))
        assert parsed.transport_backend == TransportBackend.TUNGSTENITE


@pytest.mark.parametrize(
    ("module_name", "class_name", "kwargs", "assert_in_repr"),
    [
        ("coinbase", "CoinbaseDataClientConfig", {}, True),
        ("coinbase", "CoinbaseExecClientConfig", {}, True),
        ("derive", "DeriveDataClientConfig", {}, True),
        ("derive", "DeriveExecClientConfig", {}, True),
        ("lighter", "LighterDataClientConfig", {}, True),
        (
            "lighter",
            "LighterExecClientConfig",
            {
                "trader_id": nautilus_pyo3.TraderId("TESTER-001"),
                "account_id": nautilus_pyo3.AccountId("LIGHTER-001"),
            },
            True,
        ),
        ("polymarket", "PolymarketDataClientConfig", {}, True),
        ("polymarket", "PolymarketExecClientConfig", {}, False),
    ],
)
def test_pyo3_configs_accept_transport_backend(
    module_name,
    class_name,
    kwargs,
    assert_in_repr,
):
    cls = getattr(getattr(nautilus_pyo3, module_name), class_name)
    config = cls(transport_backend=TransportBackend.TUNGSTENITE, **kwargs)

    assert "transport_backend" in inspect.signature(cls).parameters
    if assert_in_repr:
        assert "transport_backend: Tungstenite" in repr(config)


def test_public_network_module_exposes_transport_backend() -> None:
    assert TransportBackend.TUNGSTENITE != TransportBackend.SOCKUDO
    assert inspect.signature(WebSocketConfig).parameters["backend"].default is None

    config = WebSocketConfig(
        "wss://example.invalid",
        [],
        backend=TransportBackend.TUNGSTENITE,
    )

    assert config is not None


def test_transport_backend_none_preserves_wrapper_and_pyo3_defaults() -> None:
    runtime_config = BinanceDataClientConfig()
    parsed_runtime_config = BinanceDataClientConfig.parse(
        msgspec.json.encode({"transport_backend": None}),
    )
    module_name = "binance"
    class_name = "BinanceDataClientConfig"
    pyo3_config = getattr(getattr(nautilus_pyo3, module_name), class_name)(transport_backend=None)

    assert runtime_config.transport_backend is None
    assert parsed_runtime_config.transport_backend is None
    assert "transport_backend: Sockudo" in repr(pyo3_config)
    assert WebSocketConfig("wss://example.invalid", [], backend=None) is not None
