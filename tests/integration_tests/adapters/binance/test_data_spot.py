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

import asyncio
import pkgutil

import msgspec
import pytest

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.factories import BinanceLiveDataClientFactory
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.spot.providers import BinanceSpotInstrumentProvider
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.functions import eventually
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


@pytest.mark.skip(reason="WIP")
class TestBinanceSpotDataClient:
    @pytest.fixture(autouse=True)
    def setup(self, request):
        # Fixture Setup
        self.loop = request.getfixturevalue("event_loop")
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.trader_id = TestIdStubs.trader_id()
        self.venue = BINANCE_VENUE
        self.account_id = AccountId(f"{self.venue.value}-001")

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.http_client = BinanceHttpClient(
            clock=self.clock,
            api_key="SOME_BINANCE_API_KEY",
            api_secret="SOME_BINANCE_API_SECRET",
            base_url="https://api.binance.com/",  # Spot/Margin
        )

        self.provider = BinanceSpotInstrumentProvider(
            client=self.http_client,
            clock=self.clock,
            config=InstrumentProviderConfig(load_all=True),
        )

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.data_client = BinanceLiveDataClientFactory.create(
            loop=self.loop,
            client=self.http_client,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=self.provider,
        )

        yield

    @pytest.mark.asyncio()
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
            self,  # (needed for mock)
            http_method: str,  # (needed for mock)
            url_path: str,  # (needed for mock)
            payload: dict[str, str],  # (needed for mock)
        ) -> bytes:
            return msgspec.json.decode(responses.pop())

        # Apply mock coroutine to client
        monkeypatch.setattr(
            target=BinanceHttpClient,
            name="send_request",
            value=mock_send_request,
        )

        # Act
        self.data_client.connect()

        # Assert
        await eventually(lambda: self.data_client.is_connected)

    @pytest.mark.asyncio()
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
            self,  # (needed for mock)
            http_method: str,  # (needed for mock)
            url_path: str,  # (needed for mock)
            payload: dict[str, str],  # (needed for mock)
        ) -> bytes:
            return msgspec.json.decode(responses.pop())

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

    @pytest.mark.asyncio()
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
            self,  # (needed for mock)
            http_method: str,  # (needed for mock)
            url_path: str,  # (needed for mock)
            payload: dict[str, str],  # (needed for mock)
        ) -> bytes:
            return msgspec.json.decode(responses.pop())

        # Apply mock coroutine to client
        monkeypatch.setattr(
            target=BinanceHttpClient,
            name="send_request",
            value=mock_send_request,
        )

        self.data_client.connect()
        await eventually(lambda: self.data_client.is_connected)

        # Act
        self.data_client.subscribe_instruments()

        # Assert
        btcusdt = InstrumentId.from_str("BTCUSDT.BINANCE")
        ethusdt = InstrumentId.from_str("ETHUSDT.BINANCE")
        assert self.data_client.subscribed_instruments() == [btcusdt, ethusdt]

    @pytest.mark.asyncio()
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
            self,  # (needed for mock)
            http_method: str,  # (needed for mock)
            url_path: str,  # (needed for mock)
            payload: dict[str, str],  # (needed for mock)
        ) -> bytes:
            return msgspec.json.decode(responses.pop())

        # Apply mock coroutine to client
        monkeypatch.setattr(
            target=BinanceHttpClient,
            name="send_request",
            value=mock_send_request,
        )

        self.data_client.connect()
        await eventually(lambda: self.data_client.is_connected)

        ethusdt = InstrumentId.from_str("ETHUSDT.BINANCE")

        # Act
        self.data_client.subscribe_instrument(ethusdt)

        # Assert
        assert self.data_client.subscribed_instruments() == [ethusdt]

    @pytest.mark.asyncio()
    async def test_subscribe_quote_ticks(self, monkeypatch):
        handler = []
        self.msgbus.subscribe(
            topic="data.quotes.BINANCE.ETHUSDT",
            handler=handler.append,
        )

        # Act
        self.data_client.subscribe_quote_ticks(ETHUSDT_BINANCE.id)

        raw_book_tick = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_ticker_book.json",
        )

        # Assert
        self.data_client._handle_ws_message(raw_book_tick)
        await eventually(lambda: self.data_engine.data_count)

        assert self.data_engine.data_count == 1
        assert len(handler) == 1  # <-- handler received tick
        assert handler[0] == QuoteTick(
            instrument_id=ETHUSDT_BINANCE.id,
            bid_price=Price.from_str("4507.24000000"),
            ask_price=Price.from_str("4507.25000000"),
            bid_size=Quantity.from_str("2.35950000"),
            ask_size=Quantity.from_str("2.84570000"),
            ts_event=handler[0].ts_init,  # TODO: WIP
            ts_init=handler[0].ts_init,
        )

    @pytest.mark.asyncio()
    async def test_subscribe_trade_ticks(self, monkeypatch):
        handler = []
        self.msgbus.subscribe(
            topic="data.trades.BINANCE.ETHUSDT",
            handler=handler.append,
        )

        # Act
        self.data_client.subscribe_trade_ticks(ETHUSDT_BINANCE.id)

        raw_trade = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_trade.json",
        )

        # Assert
        self.data_client._handle_ws_message(raw_trade)
        await eventually(lambda: self.data_engine.data_count)

        assert self.data_engine.data_count == 1
        assert len(handler) == 1  # <-- handler received tick
        assert handler[0] == TradeTick(
            instrument_id=ETHUSDT_BINANCE.id,
            price=Price.from_str("4149.74000000"),
            size=Quantity.from_str("0.43870000"),
            aggressor_side=AggressorSide.SELLER,
            trade_id=TradeId("705291099"),
            ts_event=1639351062243000064,
            ts_init=handler[0].ts_init,
        )

    @pytest.mark.asyncio()
    async def test_subscribe_agg_trade_ticks(self, monkeypatch):
        handler = []
        self.msgbus.subscribe(
            topic="data.trades.BINANCE.ETHUSDT",
            handler=handler.append,
        )

        # Act
        self.data_client._use_agg_trade_ticks = True
        self.data_client.subscribe_trade_ticks(ETHUSDT_BINANCE.id)
        self.data_client._use_agg_trade_ticks = False

        raw_trade = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_agg_trade.json",
        )

        # Assert
        self.data_client._handle_ws_message(raw_trade)
        await eventually(lambda: self.data_engine.data_count)

        assert self.data_engine.data_count == 1
        assert len(handler) == 1  # <-- handler received tick
        assert handler[0] == TradeTick(
            instrument_id=ETHUSDT_BINANCE.id,
            price=Price.from_str("1632.46000000"),
            size=Quantity.from_str("0.34305000"),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("226532"),
            ts_event=1675759520847,
            ts_init=handler[0].ts_init,
        )
