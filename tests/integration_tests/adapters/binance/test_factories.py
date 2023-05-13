# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import asyncio

import pytest

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.config import BinanceDataClientConfig
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.factories import BinanceLiveDataClientFactory
from nautilus_trader.adapters.binance.factories import BinanceLiveExecClientFactory
from nautilus_trader.adapters.binance.factories import _get_http_base_url
from nautilus_trader.adapters.binance.factories import _get_ws_base_url
from nautilus_trader.adapters.binance.futures.data import BinanceFuturesDataClient
from nautilus_trader.adapters.binance.futures.execution import BinanceFuturesExecutionClient
from nautilus_trader.adapters.binance.spot.data import BinanceSpotDataClient
from nautilus_trader.adapters.binance.spot.execution import BinanceSpotExecutionClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import Logger
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestBinanceFactories:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.clock = LiveClock()
        self.logger = Logger(
            clock=self.clock,
            level_stdout=LogLevel.DEBUG,
            bypass=True,
        )

        self.trader_id = TestIdStubs.trader_id()
        self.strategy_id = TestIdStubs.strategy_id()
        self.account_id = TestIdStubs.account_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache_db = MockCacheDatabase(
            logger=self.logger,
        )

        self.cache = Cache(
            database=self.cache_db,
            logger=self.logger,
        )

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
                BinanceAccountType.MARGIN_CROSS,
                False,
                False,
                "https://sapi.binance.com",
            ],
            [
                BinanceAccountType.MARGIN_ISOLATED,
                False,
                False,
                "https://sapi.binance.com",
            ],
            [
                BinanceAccountType.FUTURES_USDT,
                False,
                False,
                "https://fapi.binance.com",
            ],
            [
                BinanceAccountType.FUTURES_COIN,
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
                BinanceAccountType.MARGIN_CROSS,
                False,
                True,
                "https://sapi.binance.us",
            ],
            [
                BinanceAccountType.MARGIN_ISOLATED,
                False,
                True,
                "https://sapi.binance.us",
            ],
            [
                BinanceAccountType.FUTURES_USDT,
                False,
                True,
                "https://fapi.binance.us",
            ],
            [
                BinanceAccountType.FUTURES_COIN,
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
                BinanceAccountType.MARGIN_CROSS,
                True,
                False,
                "https://testnet.binance.vision",
            ],
            [
                BinanceAccountType.MARGIN_ISOLATED,
                True,
                False,
                "https://testnet.binance.vision",
            ],
            [
                BinanceAccountType.FUTURES_USDT,
                True,
                False,
                "https://testnet.binancefuture.com",
            ],
        ],
    )
    def test_get_http_base_url(self, account_type, is_testnet, is_us, expected):
        # Arrange, Act
        base_url = _get_http_base_url(account_type, is_testnet, is_us)

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
                BinanceAccountType.MARGIN_CROSS,
                False,
                False,
                "wss://stream.binance.com:9443",
            ],
            [
                BinanceAccountType.MARGIN_ISOLATED,
                False,
                False,
                "wss://stream.binance.com:9443",
            ],
            [
                BinanceAccountType.FUTURES_USDT,
                False,
                False,
                "wss://fstream.binance.com",
            ],
            [
                BinanceAccountType.FUTURES_COIN,
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
                BinanceAccountType.MARGIN_CROSS,
                False,
                True,
                "wss://stream.binance.us:9443",
            ],
            [
                BinanceAccountType.MARGIN_ISOLATED,
                False,
                True,
                "wss://stream.binance.us:9443",
            ],
            [
                BinanceAccountType.FUTURES_USDT,
                False,
                True,
                "wss://fstream.binance.us",
            ],
            [
                BinanceAccountType.FUTURES_COIN,
                False,
                True,
                "wss://dstream.binance.us",
            ],
            [
                BinanceAccountType.SPOT,
                True,
                False,
                "wss://testnet.binance.vision",
            ],
            [
                BinanceAccountType.MARGIN_CROSS,
                True,
                False,
                "wss://testnet.binance.vision",
            ],
            [
                BinanceAccountType.MARGIN_ISOLATED,
                True,
                False,
                "wss://testnet.binance.vision",
            ],
            [
                BinanceAccountType.FUTURES_USDT,
                True,
                False,
                "wss://stream.binancefuture.com",
            ],
        ],
    )
    def test_get_ws_base_url(self, account_type, is_testnet, is_us, expected):
        # Arrange, Act
        base_url = _get_ws_base_url(account_type, is_testnet, is_us)

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
            logger=self.logger,
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
                account_type=BinanceAccountType.FUTURES_USDT,
            ),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
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
            logger=self.logger,
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
                account_type=BinanceAccountType.FUTURES_USDT,
            ),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        assert isinstance(exec_client, BinanceFuturesExecutionClient)
