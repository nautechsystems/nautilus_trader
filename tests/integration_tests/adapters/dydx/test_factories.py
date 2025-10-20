# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
Unit tests for the dYdX factories.
"""

import pytest

from nautilus_trader.adapters.dydx.common.urls import get_grpc_base_url
from nautilus_trader.adapters.dydx.common.urls import get_http_base_url
from nautilus_trader.adapters.dydx.common.urls import get_ws_base_url
from nautilus_trader.adapters.dydx.config import DYDXDataClientConfig
from nautilus_trader.adapters.dydx.data import DYDXDataClient
from nautilus_trader.adapters.dydx.factories import DYDXLiveDataClientFactory
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase


@pytest.mark.parametrize(
    ("is_testnet", "expected"),
    [
        (False, "https://indexer.dydx.trade/v4"),
        (True, "https://indexer.v4testnet.dydx.exchange/v4"),
    ],
)
def test_get_http_base_url(is_testnet: bool, expected: str) -> None:
    """
    Test the base url for the http endpoints.
    """
    base_url = get_http_base_url(is_testnet)
    assert base_url == expected


@pytest.mark.parametrize(
    ("is_testnet", "expected"),
    [
        (False, "wss://indexer.dydx.trade/v4/ws"),
        (True, "wss://indexer.v4testnet.dydx.exchange/v4/ws"),
    ],
)
def test_get_ws_base_url(is_testnet: bool, expected: str) -> None:
    """
    Test the base url for the websocket endpoints.
    """
    base_url = get_ws_base_url(is_testnet)
    assert base_url == expected


@pytest.mark.parametrize(
    ("is_testnet", "expected"),
    [
        (True, "test-dydx-grpc.kingnodes.com"),
        (False, "dydx-ops-grpc.kingnodes.com:443"),
    ],
)
def test_grpc_base_url(is_testnet: bool, expected: str) -> None:
    """
    Test the base url for the GRPC endpoints.
    """
    base_url = get_grpc_base_url(is_testnet)
    assert base_url == expected


def test_create_dydx_live_data_client(event_loop_for_setup) -> None:
    """
    Test the data client factory for dYdX.
    """
    # Prepare
    clock = LiveClock()
    msgbus = MessageBus(
        trader_id=TraderId("TESTER-000"),
        clock=clock,
    )
    cache = Cache(database=MockCacheDatabase())

    data_client = DYDXLiveDataClientFactory.create(
        loop=event_loop_for_setup,
        name="DYDX",
        config=DYDXDataClientConfig(wallet_address="DYDX_WALLET_ADDRESS"),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Assert
    assert isinstance(data_client, DYDXDataClient)
