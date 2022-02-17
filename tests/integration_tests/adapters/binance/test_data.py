# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
import pkgutil
from typing import Dict

import orjson
import pytest

from nautilus_trader.adapters.binance.common import BINANCE_VENUE
from nautilus_trader.adapters.binance.data import BinanceDataClient
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.providers import BinanceInstrumentProvider
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.msgbus.bus import MessageBus
from tests.test_kit.stubs import TestStubs


class TestBinanceDataClient:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(clock=self.clock)

        self.trader_id = TestStubs.trader_id()
        self.venue = BINANCE_VENUE
        self.account_id = AccountId(self.venue.value, "001")

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestStubs.cache()

        self.http_client = BinanceHttpClient(  # noqa: S106 (no hardcoded password)
            loop=asyncio.get_event_loop(),
            clock=self.clock,
            logger=self.logger,
            key="SOME_BINANCE_API_KEY",
            secret="SOME_BINANCE_API_SECRET",
        )

        self.provider = BinanceInstrumentProvider(
            client=self.http_client,
            logger=self.logger,
        )

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_client = BinanceDataClient(
            loop=self.loop,
            client=self.http_client,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            instrument_provider=self.provider,
        )

    @pytest.mark.asyncio
    async def test_connect(self, monkeypatch):
        # Arrange: prepare data for monkey patch
        response1 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_wallet_trading_fee.json",
        )

        response2 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_spot_market_exchange_info.json",
        )

        responses = [response2, response1]

        # Mock coroutine for patch
        async def mock_send_request(
            self,  # noqa (needed for mock)
            http_method: str,  # noqa (needed for mock)
            url_path: str,  # noqa (needed for mock)
            payload: Dict[str, str],  # noqa (needed for mock)
        ) -> bytes:
            return orjson.loads(responses.pop())

        # Apply mock coroutine to client
        monkeypatch.setattr(
            target=BinanceHttpClient,
            name="send_request",
            value=mock_send_request,
        )

        # Act
        self.data_client.connect()
        await asyncio.sleep(1)

        # Assert
        assert self.data_client.is_connected

    @pytest.mark.asyncio
    async def test_disconnect(self, monkeypatch):
        # Arrange: prepare data for monkey patch
        response1 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_wallet_trading_fee.json",
        )

        response2 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_spot_market_exchange_info.json",
        )

        responses = [response2, response1]

        # Mock coroutine for patch
        async def mock_send_request(
            self,  # noqa (needed for mock)
            http_method: str,  # noqa (needed for mock)
            url_path: str,  # noqa (needed for mock)
            payload: Dict[str, str],  # noqa (needed for mock)
        ) -> bytes:
            return orjson.loads(responses.pop())

        # Apply mock coroutine to client
        monkeypatch.setattr(
            target=BinanceHttpClient,
            name="send_request",
            value=mock_send_request,
        )

        self.data_client.connect()
        await asyncio.sleep(1)

        # Act
        self.data_client.disconnect()
        await asyncio.sleep(1)

        # Assert
        assert not self.data_client.is_connected

    @pytest.mark.asyncio
    async def test_subscribe_instruments(self, monkeypatch):
        # Arrange: prepare data for monkey patch
        response1 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_wallet_trading_fee.json",
        )

        response2 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_spot_market_exchange_info.json",
        )

        responses = [response2, response1]

        # Mock coroutine for patch
        async def mock_send_request(
            self,  # noqa (needed for mock)
            http_method: str,  # noqa (needed for mock)
            url_path: str,  # noqa (needed for mock)
            payload: Dict[str, str],  # noqa (needed for mock)
        ) -> bytes:
            return orjson.loads(responses.pop())

        # Apply mock coroutine to client
        monkeypatch.setattr(
            target=BinanceHttpClient,
            name="send_request",
            value=mock_send_request,
        )

        self.data_client.connect()
        await asyncio.sleep(1)

        # Act
        self.data_client.subscribe_instruments()

        # Assert
        btcusdt = InstrumentId.from_str("BTCUSDT.BINANCE")
        ethusdt = InstrumentId.from_str("ETHUSDT.BINANCE")
        assert self.data_client.subscribed_instruments() == [btcusdt, ethusdt]

    @pytest.mark.asyncio
    async def test_subscribe_instrument(self, monkeypatch):
        # Arrange: prepare data for monkey patch
        response1 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_wallet_trading_fee.json",
        )

        response2 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_spot_market_exchange_info.json",
        )

        responses = [response2, response1]

        # Mock coroutine for patch
        async def mock_send_request(
            self,  # noqa (needed for mock)
            http_method: str,  # noqa (needed for mock)
            url_path: str,  # noqa (needed for mock)
            payload: Dict[str, str],  # noqa (needed for mock)
        ) -> bytes:
            return orjson.loads(responses.pop())

        # Apply mock coroutine to client
        monkeypatch.setattr(
            target=BinanceHttpClient,
            name="send_request",
            value=mock_send_request,
        )

        self.data_client.connect()
        await asyncio.sleep(1)

        ethusdt = InstrumentId.from_str("ETHUSDT.BINANCE")

        # Act
        self.data_client.subscribe_instrument(ethusdt)

        # Assert
        assert self.data_client.subscribed_instruments() == [ethusdt]

    @pytest.mark.asyncio
    async def test_subscribe_quote_ticks(self, monkeypatch):
        # Arrange: prepare data for monkey patch
        response1 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_wallet_trading_fee.json",
        )

        response2 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_spot_market_exchange_info.json",
        )

        responses = [response2, response1]

        # Mock coroutine for patch
        async def mock_send_request(
            self,  # noqa (needed for mock)
            http_method: str,  # noqa (needed for mock)
            url_path: str,  # noqa (needed for mock)
            payload: Dict[str, str],  # noqa (needed for mock)
        ) -> bytes:
            return orjson.loads(responses.pop())

        # Apply mock coroutine to client
        monkeypatch.setattr(
            target=BinanceHttpClient,
            name="send_request",
            value=mock_send_request,
        )

        ethusdt = InstrumentId.from_str("ETHUSDT.BINANCE")

        handler = []
        self.msgbus.subscribe(
            topic="data.quotes.BINANCE.ETHUSDT",
            handler=handler.append,
        )

        self.data_client.connect()
        await asyncio.sleep(1)

        # Act
        self.data_client.subscribe_quote_ticks(ethusdt)

        raw_book_tick = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_ticker_book.json",
        )

        # Assert
        self.data_client._handle_spot_ws_message(raw_book_tick)
        await asyncio.sleep(1)

        assert self.data_engine.data_count == 3
        assert len(handler) == 1  # <-- handler received tick

    @pytest.mark.asyncio
    async def test_subscribe_trade_ticks(self, monkeypatch):
        # Arrange: prepare data for monkey patch
        response1 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_wallet_trading_fee.json",
        )

        response2 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_spot_market_exchange_info.json",
        )

        responses = [response2, response1]

        # Mock coroutine for patch
        async def mock_send_request(
            self,  # noqa (needed for mock)
            http_method: str,  # noqa (needed for mock)
            url_path: str,  # noqa (needed for mock)
            payload: Dict[str, str],  # noqa (needed for mock)
        ) -> bytes:
            return orjson.loads(responses.pop())

        # Apply mock coroutine to client
        monkeypatch.setattr(
            target=BinanceHttpClient,
            name="send_request",
            value=mock_send_request,
        )

        ethusdt = InstrumentId.from_str("ETHUSDT.BINANCE")

        handler = []
        self.msgbus.subscribe(
            topic="data.trades.BINANCE.ETHUSDT",
            handler=handler.append,
        )

        self.data_client.connect()
        await asyncio.sleep(1)

        # Act
        self.data_client.subscribe_trade_ticks(ethusdt)

        raw_trade = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_trade.json",
        )

        # Assert
        self.data_client._handle_spot_ws_message(raw_trade)
        await asyncio.sleep(1)

        assert self.data_engine.data_count == 3
        assert len(handler) == 1  # <-- handler received tick
