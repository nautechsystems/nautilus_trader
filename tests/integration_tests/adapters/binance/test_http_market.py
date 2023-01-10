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

from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.spot.http.market import BinanceSpotMarketHttpAPI
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


@pytest.mark.skip(reason="WIP")
class TestBinanceSpotMarketHttpAPI:
    def setup(self):
        # Fixture Setup
        clock = LiveClock()
        logger = Logger(clock=clock)
        self.client = BinanceHttpClient(  # noqa: S106 (no hardcoded password)
            loop=asyncio.get_event_loop(),
            clock=clock,
            logger=logger,
            key="SOME_BINANCE_API_KEY",
            secret="SOME_BINANCE_API_SECRET",
        )

        self.api = BinanceSpotMarketHttpAPI(self.client)

    @pytest.mark.asyncio
    async def test_ping_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.ping()

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/ping"

    @pytest.mark.asyncio
    async def test_time_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.time()

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/time"

    @pytest.mark.asyncio
    async def test_exchange_info_with_symbol_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.exchange_info(symbol="BTCUSDT")

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/exchangeInfo"
        assert request["params"] == "symbol=BTCUSDT"

    @pytest.mark.asyncio
    async def test_exchange_info_with_symbols_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.exchange_info(symbols=["BTCUSDT", "ETHUSDT"])

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/exchangeInfo"
        assert request["params"] == "symbols=%5B%22BTCUSDT%22%2C%22ETHUSDT%22%5D"

    @pytest.mark.asyncio
    async def test_depth_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.depth(symbol="BTCUSDT", limit=10)

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/depth"
        assert request["params"] == "symbol=BTCUSDT&limit=10"

    @pytest.mark.asyncio
    async def test_trades_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.trades(symbol="BTCUSDT", limit=10)

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/trades"
        assert request["params"] == "symbol=BTCUSDT&limit=10"

    @pytest.mark.asyncio
    async def test_historical_trades_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.historical_trades(symbol="BTCUSDT", from_id=0, limit=10)

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/historicalTrades"
        assert request["params"] == "symbol=BTCUSDT&limit=10&fromId=0"

    @pytest.mark.asyncio
    async def test_agg_trades_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.agg_trades(
            symbol="BTCUSDT",
            from_id=0,
            start_time_ms=0,
            end_time_ms=1,
            limit=10,
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/aggTrades"
        assert request["params"] == "symbol=BTCUSDT&fromId=0&startTime=0&endTime=1&limit=10"

    @pytest.mark.asyncio
    async def test_klines_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.klines(
            symbol="BTCUSDT",
            interval="1m",
            start_time_ms=0,
            end_time_ms=1,
            limit=1000,
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/klines"
        assert request["params"] == "symbol=BTCUSDT&interval=1m&startTime=0&endTime=1&limit=1000"

    @pytest.mark.asyncio
    async def test_avg_price_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.avg_price(symbol="BTCUSDT")

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/avgPrice"
        assert request["params"] == "symbol=BTCUSDT"

    @pytest.mark.asyncio
    async def test_ticker_24hr_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.ticker_24hr(symbol="BTCUSDT")

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/ticker/24hr"
        assert request["params"] == "symbol=BTCUSDT"

    @pytest.mark.asyncio
    async def test_ticker_price_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.ticker_price(symbol="BTCUSDT")

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/ticker/price"
        assert request["params"] == "symbol=BTCUSDT"

    @pytest.mark.asyncio
    async def test_book_ticker_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.book_ticker(symbol="BTCUSDT")

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/ticker/bookTicker"
        assert request["params"] == "symbol=BTCUSDT"
