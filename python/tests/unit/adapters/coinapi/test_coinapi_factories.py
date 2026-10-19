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
"""
Test CoinAPI Python config and factory bindings.
"""

import pytest

from nautilus_trader.adapters.coinapi import COINAPI
from nautilus_trader.adapters.coinapi import COINAPI_CLIENT_ID
from nautilus_trader.adapters.coinapi import CoinApiDataClientConfig
from nautilus_trader.adapters.coinapi import CoinApiDataClientFactory
from nautilus_trader.adapters.coinapi import CoinApiInstrumentSelection
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId
from nautilus_trader.network import TransportBackend


TEST_API_KEY = "test-api-key"


def build_node(config: CoinApiDataClientConfig) -> LiveNode:
    """
    Build a live node with one CoinAPI data client.
    """
    return (
        LiveNode.builder("COINAPI-PYTEST-001", TraderId.from_str("TESTER-001"), Environment.LIVE)
        .add_data_client(None, CoinApiDataClientFactory(), config)
        .build()
    )


def test_coinapi_constants_and_factory_name() -> None:
    """
    Test the provider constants and factory name.
    """
    assert COINAPI == "COINAPI"
    assert ClientId("COINAPI") == COINAPI_CLIENT_ID
    assert CoinApiDataClientFactory().name() == COINAPI


def test_coinapi_config_defaults() -> None:
    """
    Test the config exposes its defaults.
    """
    config = CoinApiDataClientConfig()

    assert config.base_url_http is None
    assert config.base_url_ws is None
    assert config.instruments == []
    assert config.credit_budget == 100
    assert config.max_retries == 3
    assert config.http_timeout_secs == 30
    assert config.max_concurrent_requests == 1
    assert config.retry_delay_initial_ms == 1_000
    assert config.retry_delay_max_ms == 10_000
    assert config.retry_max_elapsed_ms == 120_000
    assert config.has_proxy_url is False
    assert not hasattr(config, "proxy_url")
    assert config.transport_backend == TransportBackend.SOCKUDO
    assert config.book_snapshot_timeout_secs == 10
    assert config.price_precisions == {}


def test_coinapi_config_exposes_typed_fields() -> None:
    """
    Test the config and instrument selection expose their fields.
    """
    selection = CoinApiInstrumentSelection(
        "COINBASE",
        [InstrumentId.from_str("BTC-USD.COINBASE")],
        filter_symbol_id="COINBASE_SPOT_BTC_USD",
        filter_asset_id="BTC",
    )

    config = CoinApiDataClientConfig(
        api_key=TEST_API_KEY,
        base_url_http="http://127.0.0.1:1/v1/",
        base_url_ws="ws://127.0.0.1:2/v1/",
        instruments=[selection],
        credit_budget=7,
        max_retries=0,
        proxy_url="http://user:proxy-secret@127.0.0.1:3",
        http_timeout_secs=13,
        max_concurrent_requests=4,
        retry_delay_initial_ms=29,
        retry_delay_max_ms=53,
        retry_max_elapsed_ms=71_000,
        transport_backend=TransportBackend.TUNGSTENITE,
        book_snapshot_timeout_secs=0,
        price_precisions={InstrumentId.from_str("BTC-USD.COINBASE"): 3},
    )

    assert config.base_url_http == "http://127.0.0.1:1/v1/"
    assert config.base_url_ws == "ws://127.0.0.1:2/v1/"
    assert config.credit_budget == 7
    assert config.max_retries == 0
    assert config.http_timeout_secs == 13
    assert config.max_concurrent_requests == 4
    assert config.retry_delay_initial_ms == 29
    assert config.retry_delay_max_ms == 53
    assert config.retry_max_elapsed_ms == 71_000
    assert config.has_proxy_url is True
    assert config.transport_backend == TransportBackend.TUNGSTENITE
    assert config.book_snapshot_timeout_secs == 0
    assert config.price_precisions == {InstrumentId.from_str("BTC-USD.COINBASE"): 3}
    [loaded] = config.instruments
    assert loaded.exchange_id == "COINBASE"
    assert loaded.instrument_ids == [InstrumentId.from_str("BTC-USD.COINBASE")]
    assert loaded.filter_symbol_id == "COINBASE_SPOT_BTC_USD"
    assert loaded.filter_asset_id == "BTC"


def test_coinapi_config_redacts_api_key() -> None:
    """
    Test the config does not reveal its API key.
    """
    config = CoinApiDataClientConfig(
        api_key=TEST_API_KEY,
        proxy_url="http://user:proxy-secret@localhost",
    )

    assert repr(config) == "CoinApiDataClientConfig"
    assert not hasattr(config, "api_key")
    assert not hasattr(config, "proxy_url")


def test_live_node_builder_accepts_coinapi_data_factory() -> None:
    """
    Test the live node builder creates a CoinAPI data client from the factory.
    """
    node = build_node(CoinApiDataClientConfig(api_key=TEST_API_KEY))

    assert node.trader_id == TraderId.from_str("TESTER-001")
    assert node.environment == Environment.LIVE


def test_coinapi_api_key_falls_back_to_environment(monkeypatch: pytest.MonkeyPatch) -> None:
    """
    Test an unset API key resolves from the environment variable.
    """
    monkeypatch.setenv("COINAPI_API_KEY", TEST_API_KEY)

    node = build_node(CoinApiDataClientConfig())

    assert node.trader_id == TraderId.from_str("TESTER-001")


def test_coinapi_missing_api_key_fails_build(monkeypatch: pytest.MonkeyPatch) -> None:
    """
    Test the build fails when neither the config nor the environment provides a key.
    """
    monkeypatch.delenv("COINAPI_API_KEY", raising=False)

    with pytest.raises(RuntimeError, match="set api_key or COINAPI_API_KEY"):
        build_node(CoinApiDataClientConfig())


@pytest.mark.parametrize(
    ("base_url_http", "base_url_ws", "expected"),
    [
        ("ws://127.0.0.1:1/v1/", None, "base URL must be HTTP"),
        (None, "http://127.0.0.1:2/v1/", "URL must be WS"),
    ],
)
def test_coinapi_endpoint_overrides_reach_client(
    base_url_http: str | None,
    base_url_ws: str | None,
    expected: str,
) -> None:
    """
    Test endpoint overrides reach client construction.
    """
    config = CoinApiDataClientConfig(
        api_key=TEST_API_KEY,
        base_url_http=base_url_http,
        base_url_ws=base_url_ws,
    )

    with pytest.raises(RuntimeError, match=expected):
        build_node(config)
