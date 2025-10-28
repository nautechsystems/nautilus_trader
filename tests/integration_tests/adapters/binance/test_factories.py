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

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.urls import get_http_base_url
from nautilus_trader.adapters.binance.common.urls import get_ws_base_url
from nautilus_trader.adapters.binance.config import BinanceDataClientConfig
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.factories import BinanceLiveDataClientFactory
from nautilus_trader.adapters.binance.factories import BinanceLiveExecClientFactory
from nautilus_trader.adapters.binance.futures.data import BinanceFuturesDataClient
from nautilus_trader.adapters.binance.futures.execution import BinanceFuturesExecutionClient
from nautilus_trader.adapters.binance.spot.data import BinanceSpotDataClient
from nautilus_trader.adapters.binance.spot.execution import BinanceSpotExecutionClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestBinanceFactories:
    @pytest.fixture(autouse=True)
    def setup(self, request):
        # Fixture Setup
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

        self.cache = Cache(
            database=self.cache_db,
        )

        yield

    @pytest.mark.parametrize(
        ("account_type", "is_testnet", "is_us", "expected"),
        [
            [
                BinanceAccountType.SPOT,
                False,
                False,
                "https://api.binance.com",
            ],
            [
                BinanceAccountType.MARGIN,
                False,
                False,
                "https://sapi.binance.com",
            ],
            [
                BinanceAccountType.ISOLATED_MARGIN,
                False,
                False,
                "https://sapi.binance.com",
            ],
            [
                BinanceAccountType.USDT_FUTURES,
                False,
                False,
                "https://fapi.binance.com",
            ],
            [
                BinanceAccountType.COIN_FUTURES,
                False,
                False,
                "https://dapi.binance.com",
            ],
            [
                BinanceAccountType.SPOT,
                False,
                True,
                "https://api.binance.us",
            ],
            [
                BinanceAccountType.MARGIN,
                False,
                True,
                "https://sapi.binance.us",
            ],
            [
                BinanceAccountType.ISOLATED_MARGIN,
                False,
                True,
                "https://sapi.binance.us",
            ],
            [
                BinanceAccountType.USDT_FUTURES,
                False,
                True,
                "https://fapi.binance.us",
            ],
            [
                BinanceAccountType.COIN_FUTURES,
                False,
                True,
                "https://dapi.binance.us",
            ],
            [
                BinanceAccountType.SPOT,
                True,
                False,
                "https://testnet.binance.vision",
            ],
            [
                BinanceAccountType.MARGIN,
                True,
                False,
                "https://testnet.binance.vision",
            ],
            [
                BinanceAccountType.ISOLATED_MARGIN,
                True,
                False,
                "https://testnet.binance.vision",
            ],
            [
                BinanceAccountType.USDT_FUTURES,
                True,
                False,
                "https://testnet.binancefuture.com",
            ],
        ],
    )
    def test_get_http_base_url(self, account_type, is_testnet, is_us, expected):
        # Arrange, Act
        base_url = get_http_base_url(account_type, is_testnet, is_us)

        # Assert
        assert base_url == expected

    @pytest.mark.parametrize(
        ("account_type", "is_testnet", "is_us", "expected"),
        [
            [
                BinanceAccountType.SPOT,
                False,
                False,
                "wss://stream.binance.com:9443",
            ],
            [
                BinanceAccountType.MARGIN,
                False,
                False,
                "wss://stream.binance.com:9443",
            ],
            [
                BinanceAccountType.ISOLATED_MARGIN,
                False,
                False,
                "wss://stream.binance.com:9443",
            ],
            [
                BinanceAccountType.USDT_FUTURES,
                False,
                False,
                "wss://fstream.binance.com",
            ],
            [
                BinanceAccountType.COIN_FUTURES,
                False,
                False,
                "wss://dstream.binance.com",
            ],
            [
                BinanceAccountType.SPOT,
                False,
                True,
                "wss://stream.binance.us:9443",
            ],
            [
                BinanceAccountType.MARGIN,
                False,
                True,
                "wss://stream.binance.us:9443",
            ],
            [
                BinanceAccountType.ISOLATED_MARGIN,
                False,
                True,
                "wss://stream.binance.us:9443",
            ],
            [
                BinanceAccountType.USDT_FUTURES,
                False,
                True,
                "wss://fstream.binance.us",
            ],
            [
                BinanceAccountType.COIN_FUTURES,
                False,
                True,
                "wss://dstream.binance.us",
            ],
            [
                BinanceAccountType.SPOT,
                True,
                False,
                "wss://stream.testnet.binance.vision",
            ],
            [
                BinanceAccountType.MARGIN,
                True,
                False,
                "wss://stream.testnet.binance.vision",
            ],
            [
                BinanceAccountType.ISOLATED_MARGIN,
                True,
                False,
                "wss://stream.testnet.binance.vision",
            ],
            [
                BinanceAccountType.USDT_FUTURES,
                True,
                False,
                "wss://stream.binancefuture.com",
            ],
        ],
    )
    def test_get_ws_base_url(self, account_type, is_testnet, is_us, expected):
        # Arrange, Act
        base_url = get_ws_base_url(account_type, is_testnet, is_us)

        # Assert
        assert base_url == expected

    def test_create_binance_live_spot_data_client(self, binance_http_client):
        # Arrange, Act
        data_client = BinanceLiveDataClientFactory.create(
            loop=self.loop,
            name="BINANCE",
            config=BinanceDataClientConfig(  # (S106 Possible hardcoded password)
                api_key="SOME_BINANCE_API_KEY",  # Do not remove or will fail in CI
                api_secret="SOME_BINANCE_API_SECRET",  # Do not remove or will fail in CI
                account_type=BinanceAccountType.SPOT,
            ),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        assert isinstance(data_client, BinanceSpotDataClient)

    def test_create_binance_live_futures_data_client(self, binance_http_client):
        # Arrange, Act
        data_client = BinanceLiveDataClientFactory.create(
            loop=self.loop,
            name="BINANCE",
            config=BinanceDataClientConfig(  # (S106 Possible hardcoded password)
                api_key="SOME_BINANCE_API_KEY",  # Do not remove or will fail in CI
                api_secret="SOME_BINANCE_API_SECRET",  # Do not remove or will fail in CI
                account_type=BinanceAccountType.USDT_FUTURES,
            ),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        assert isinstance(data_client, BinanceFuturesDataClient)

    def test_create_binance_spot_exec_client(self, binance_http_client):
        # Arrange, Act
        exec_client = BinanceLiveExecClientFactory.create(
            loop=self.loop,
            name="BINANCE",
            config=BinanceExecClientConfig(  # (S106 Possible hardcoded password)
                api_key="SOME_BINANCE_API_KEY",  # Do not remove or will fail in CI
                api_secret="SOME_BINANCE_API_SECRET",  # Do not remove or will fail in CI
                account_type=BinanceAccountType.SPOT,
            ),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        assert isinstance(exec_client, BinanceSpotExecutionClient)

    def test_create_binance_futures_exec_client(self, binance_http_client):
        # Arrange, Act
        exec_client = BinanceLiveExecClientFactory.create(
            loop=self.loop,
            name="BINANCE",
            config=BinanceExecClientConfig(  # (S106 Possible hardcoded password)
                api_key="SOME_BINANCE_API_KEY",
                api_secret="SOME_BINANCE_API_SECRET",
                account_type=BinanceAccountType.USDT_FUTURES,
            ),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        assert isinstance(exec_client, BinanceFuturesExecutionClient)
