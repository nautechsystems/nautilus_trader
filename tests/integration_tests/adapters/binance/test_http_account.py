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

import pytest

from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.spot.http.account import BinanceSpotAccountHttpAPI
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


@pytest.mark.skip(reason="WIP")
class TestBinanceSpotAccountHttpAPI:
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

        self.api = BinanceSpotAccountHttpAPI(self.client)

    @pytest.mark.asyncio
    async def test_new_order_test_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.new_order_test(
            symbol="ETHUSDT",
            side="SELL",
            type="LIMIT",
            time_in_force="GTC",
            quantity="0.01",
            price="5000",
            recv_window=5000,
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "POST"
        assert request["url"] == "https://api.binance.com/api/v3/order/test"
        assert request["params"].startswith(
            "symbol=ETHUSDT&side=SELL&type=LIMIT&timeInForce=GTC&quantity=0.01&price=5000&recvWindow=5000&timestamp=",
        )

    @pytest.mark.asyncio
    async def test_order_test_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.new_order(
            symbol="ETHUSDT",
            side="SELL",
            type="LIMIT",
            time_in_force="GTC",
            quantity="0.01",
            price="5000",
            recv_window=5000,
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "POST"
        assert request["url"] == "https://api.binance.com/api/v3/order"
        assert request["params"].startswith(
            "symbol=ETHUSDT&side=SELL&type=LIMIT&timeInForce=GTC&quantity=0.01&price=5000&recvWindow=5000&timestamp=",
        )

    @pytest.mark.asyncio
    async def test_cancel_order_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.cancel_order(
            symbol="ETHUSDT",
            order_id="1",
            recv_window=5000,
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "DELETE"
        assert request["url"] == "https://api.binance.com/api/v3/order"
        assert request["params"].startswith("symbol=ETHUSDT&orderId=1&recvWindow=5000&timestamp=")

    @pytest.mark.asyncio
    async def test_cancel_open_orders_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.cancel_open_orders(
            symbol="ETHUSDT",
            recv_window=5000,
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "DELETE"
        assert request["url"] == "https://api.binance.com/api/v3/openOrders"
        assert request["params"].startswith("symbol=ETHUSDT&recvWindow=5000&timestamp=")

    @pytest.mark.asyncio
    async def test_get_order_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.get_order(
            symbol="ETHUSDT",
            order_id="1",
            recv_window=5000,
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/order"
        assert request["params"].startswith("symbol=ETHUSDT&orderId=1&recvWindow=5000&timestamp=")

    @pytest.mark.asyncio
    async def test_get_open_orders_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.get_open_orders(
            symbol="ETHUSDT",
            recv_window=5000,
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/openOrders"
        assert request["params"].startswith("symbol=ETHUSDT&recvWindow=5000&timestamp=")

    @pytest.mark.asyncio
    async def test_get_orders_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.get_orders(
            symbol="ETHUSDT",
            recv_window=5000,
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/allOrders"
        assert request["params"].startswith("symbol=ETHUSDT&recvWindow=5000&timestamp=")

    @pytest.mark.asyncio
    async def test_new_oco_order_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.new_oco_order(
            symbol="ETHUSDT",
            side="BUY",
            quantity="100",
            price="5000.00",
            stop_price="4000.00",
            list_client_order_id="1",
            limit_client_order_id="O-001",
            limit_iceberg_qty="50",
            stop_client_order_id="O-002",
            stop_limit_price="3500.00",
            stop_iceberg_qty="50",
            stop_limit_time_in_force="GTC",
            recv_window=5000,
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "POST"
        assert request["url"] == "https://api.binance.com/api/v3/order/oco"
        assert request["params"].startswith(
            "symbol=ETHUSDT&side=BUY&quantity=100&price=5000.00&stopPrice=4000.00&listClientOrderId=1&limitClientOrderId=O-001&limitIcebergQty=50&stopClientOrderId=O-002&stopLimitPrice=3500.00&stopIcebergQty=50&stopLimitTimeInForce=GTC&recvWindow=5000&timestamp=",  # noqa
        )

    @pytest.mark.asyncio
    async def test_cancel_oco_order_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.cancel_oco_order(
            symbol="ETHUSDT",
            order_list_id="1",
            list_client_order_id="1",
            new_client_order_id="2",
            recv_window=5000,
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "DELETE"
        assert request["url"] == "https://api.binance.com/api/v3/orderList"
        assert request["params"].startswith(
            "symbol=ETHUSDT&orderListId=1&listClientOrderId=1&newClientOrderId=2&recvWindow=5000&timestamp=",
        )

    @pytest.mark.asyncio
    async def test_get_oco_order_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.get_oco_order(
            order_list_id="1",
            orig_client_order_id="1",
            recv_window=5000,
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/orderList"
        assert request["params"].startswith(
            "orderListId=1&origClientOrderId=1&recvWindow=5000&timestamp=",
        )

    @pytest.mark.asyncio
    async def test_get_oco_orders_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.get_oco_orders(
            from_id="1",
            start_time=1600000000,
            end_time=1637355823,
            limit=10,
            recv_window=5000,
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/allOrderList"
        assert request["params"].startswith(
            "fromId=1&startTime=1600000000&endTime=1637355823&limit=10&recvWindow=5000&timestamp=",
        )

    @pytest.mark.asyncio
    async def test_get_open_oco_orders_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.get_oco_open_orders(recv_window=5000)

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/openOrderList"
        assert request["params"].startswith("recvWindow=5000&timestamp=")

    @pytest.mark.asyncio
    async def test_account_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.account(recv_window=5000)

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/account"
        assert request["params"].startswith("recvWindow=5000&timestamp=")

    @pytest.mark.asyncio
    async def test_my_trades_sends_expected_request(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.get_account_trades(
            symbol="ETHUSDT",
            from_id="1",
            order_id="1",
            start_time=1600000000,
            end_time=1637355823,
            limit=1000,
            recv_window=5000,
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/myTrades"
        assert request["params"].startswith(
            "symbol=ETHUSDT&fromId=1&orderId=1&startTime=1600000000&endTime=1637355823&limit=1000&recvWindow=5000&timestamp=",
        )
