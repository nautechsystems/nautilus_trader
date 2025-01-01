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

from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.spot.http.market import BinanceSpotMarketHttpAPI
from nautilus_trader.common.component import LiveClock


@pytest.mark.skip(reason="WIP")
class TestBinanceSpotMarketHttpAPI:
    def setup(self):
        # Fixture Setup
        clock = LiveClock()
        self.client = BinanceHttpClient(
            clock=clock,
            api_key="SOME_BINANCE_API_KEY",
            api_secret="SOME_BINANCE_API_SECRET",
            base_url="https://api.binance.com/",  # Spot/Margin
        )

        self.api = BinanceSpotMarketHttpAPI(self.client)
        self.test_symbol = "BTCUSDT"
        self.test_symbols = ["BTCUSDT", "ETHUSDT"]

    # COMMON tests

    @pytest.mark.asyncio()
    async def test_ping_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.ping()

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/ping"

    @pytest.mark.asyncio()
    async def test_request_server_time_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.request_server_time()

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/time"

    @pytest.mark.asyncio()
    async def test_query_depth_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_depth(symbol=self.test_symbol, limit=10)

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/depth"
        assert request["params"] == "symbol=BTCUSDT&limit=10"

    @pytest.mark.asyncio()
    async def test_query_trades_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_trades(symbol=self.test_symbol, limit=10)

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/trades"
        assert request["params"] == "symbol=BTCUSDT&limit=10"

    @pytest.mark.asyncio()
    async def test_query_historical_trades_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_historical_trades(symbol=self.test_symbol, limit=10, from_id=0)

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/historicalTrades"
        assert request["params"] == "symbol=BTCUSDT&limit=10&fromId=0"

    @pytest.mark.asyncio()
    async def test_query_agg_trades_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_agg_trades(
            symbol=self.test_symbol,
            from_id=0,
            start_time=0,
            end_time=1,
            limit=10,
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/aggTrades"
        assert request["params"] == "symbol=BTCUSDT&fromId=0&startTime=0&endTime=1&limit=10"

    @pytest.mark.asyncio()
    async def test_query_klines_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_klines(
            symbol=self.test_symbol,
            interval="1m",
            start_time=0,
            end_time=1,
            limit=1000,
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/klines"
        assert request["params"] == "symbol=BTCUSDT&interval=1m&startTime=0&endTime=1&limit=1000"

    @pytest.mark.asyncio()
    async def test_query_ticker_24hr_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_ticker_24hr(symbol=self.test_symbol)

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/ticker/24hr"
        assert request["params"] == "symbol=BTCUSDT"

    @pytest.mark.asyncio()
    async def test_query_ticker_price_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_ticker_price(symbol=self.test_symbol)

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/ticker/price"
        assert request["params"] == "symbol=BTCUSDT"

    @pytest.mark.asyncio()
    async def test_query_book_ticker_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_ticker_book(symbol=self.test_symbol)

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/ticker/bookTicker"
        assert request["params"] == "symbol=BTCUSDT"

    # SPOT/MARGIN tests

    @pytest.mark.asyncio()
    async def test_query_spot_exchange_info_with_symbol_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_spot_exchange_info(symbol=self.test_symbol)

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/exchangeInfo"
        assert request["params"] == "symbol=BTCUSDT"

    @pytest.mark.asyncio()
    async def test_query_spot_exchange_info_with_symbols_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_spot_exchange_info(symbols=self.test_symbols)

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/exchangeInfo"
        assert request["params"] == "symbols=%5B%22BTCUSDT%22%2C%22ETHUSDT%22%5D"

    @pytest.mark.asyncio()
    async def test_query_spot_avg_price_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_spot_average_price(symbol=self.test_symbol)

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/avgPrice"
        assert request["params"] == "symbol=BTCUSDT"
