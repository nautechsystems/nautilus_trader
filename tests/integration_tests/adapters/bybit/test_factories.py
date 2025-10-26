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


import pytest

from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.common.urls import get_http_base_url
from nautilus_trader.adapters.bybit.common.urls import get_ws_base_url_public
from nautilus_trader.adapters.bybit.config import BybitDataClientConfig
from nautilus_trader.adapters.bybit.config import BybitExecClientConfig
from nautilus_trader.adapters.bybit.data import BybitDataClient
from nautilus_trader.adapters.bybit.execution import BybitExecutionClient
from nautilus_trader.adapters.bybit.factories import BybitLiveDataClientFactory
from nautilus_trader.adapters.bybit.factories import BybitLiveExecClientFactory
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestBybitFactories:
    @pytest.fixture(autouse=True)
    def setup(self, request):
        self.loop = request.getfixturevalue("event_loop")
        self.clock = LiveClock()

        self.trader_id = TestIdStubs.trader_id()
        self.strategy_id = TestIdStubs.strategy_id()
        self.account_id = TestIdStubs.account_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache_db = MockCacheDatabase()
        self.cache = Cache(database=self.cache_db)

        yield

    @pytest.mark.parametrize(
        ("is_demo", "is_testnet", "expected"),
        [
            [False, False, "https://api.bybit.com"],
            [False, True, "https://api-testnet.bybit.com"],
            [True, False, "https://api-demo.bybit.com"],
        ],
    )
    def test_get_http_base_url(self, is_demo, is_testnet, expected):
        base_url = get_http_base_url(is_demo, is_testnet)
        assert base_url == expected

    @pytest.mark.parametrize(
        ("product_type", "is_demo", "is_testnet", "expected"),
        [
            [
                BybitProductType.SPOT,
                False,
                False,
                "wss://stream.bybit.com/v5/public/spot",
            ],
            [
                BybitProductType.SPOT,
                False,
                True,
                "wss://stream-testnet.bybit.com/v5/public/spot",
            ],
            [
                BybitProductType.SPOT,
                True,
                False,
                "wss://stream-demo.bybit.com/v5/public/spot",
            ],
            [
                BybitProductType.LINEAR,
                False,
                False,
                "wss://stream.bybit.com/v5/public/linear",
            ],
            [
                BybitProductType.LINEAR,
                False,
                True,
                "wss://stream-testnet.bybit.com/v5/public/linear",
            ],
            [
                BybitProductType.LINEAR,
                True,
                False,
                "wss://stream-demo.bybit.com/v5/public/linear",
            ],
            [
                BybitProductType.INVERSE,
                False,
                False,
                "wss://stream.bybit.com/v5/public/inverse",
            ],
            [
                BybitProductType.INVERSE,
                False,
                True,
                "wss://stream-testnet.bybit.com/v5/public/inverse",
            ],
            [
                BybitProductType.INVERSE,
                True,
                False,
                "wss://stream-demo.bybit.com/v5/public/inverse",
            ],
        ],
    )
    def test_get_ws_base_url(self, product_type, is_demo, is_testnet, expected):
        base_url = get_ws_base_url_public(product_type, is_demo, is_testnet)
        assert base_url == expected

    def test_create_bybit_live_data_client(self, bybit_http_client):
        data_client = BybitLiveDataClientFactory.create(
            loop=self.loop,
            name="BYBIT",
            config=BybitDataClientConfig(
                api_key="SOME_BYBIT_API_KEY",
                api_secret="SOME_BYBIT_API_SECRET",
                product_types=[BybitProductType.LINEAR],
            ),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        assert isinstance(data_client, BybitDataClient)

    def test_create_bybit_live_exec_client(self, bybit_http_client):
        data_client = BybitLiveExecClientFactory.create(
            loop=self.loop,
            name="BYBIT",
            config=BybitExecClientConfig(
                api_key="SOME_BYBIT_API_KEY",
                api_secret="SOME_BYBIT_API_SECRET",
                product_types=[BybitProductType.LINEAR],
            ),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        assert isinstance(data_client, BybitExecutionClient)
