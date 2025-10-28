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

from nautilus_trader.adapters.binance.common.enums import BinanceOrderSide
from nautilus_trader.adapters.binance.common.enums import BinanceOrderType
from nautilus_trader.adapters.binance.common.enums import BinanceTimeInForce
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.futures.http.account import BinanceFuturesAccountHttpAPI
from nautilus_trader.adapters.binance.http.account import BinanceOrderHttp
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.spot.http.account import BinanceSpotAccountHttpAPI
from nautilus_trader.common.component import LiveClock


@pytest.mark.skip(reason="WIP")
class TestBinanceSpotAccountHttpAPI:
    def setup(self):
        # Fixture Setup
        self.clock = LiveClock()
        self.client = BinanceHttpClient(
            clock=self.clock,
            api_key="SOME_BINANCE_API_KEY",
            api_secret="SOME_BINANCE_API_SECRET",
            base_url="https://api.binance.com/",  # Spot/Margin
        )

        self.api = BinanceSpotAccountHttpAPI(self.client, self.clock)

        self.futures_client = BinanceHttpClient(
            clock=self.clock,
            api_key="SOME_BINANCE_FUTURES_API_KEY",
            api_secret="SOME_BINANCE_FUTURES_API_SECRET",
            base_url="https://fapi.binance.com/",  # Futures
        )

        self.futures_api = BinanceFuturesAccountHttpAPI(self.futures_client, self.clock)

    # COMMON tests

    @pytest.mark.asyncio()
    async def test_new_order_test_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        endpoint = BinanceOrderHttp(
            client=self.client,
            base_endpoint="/api/v3",
            testing_endpoint=True,
        )

        # Act
        await endpoint.post(
            params=endpoint.PostParameters(
                symbol=BinanceSymbol("ETHUSDT"),
                side=BinanceOrderSide.SELL,
                type=BinanceOrderType.LIMIT,
                timeInForce=BinanceTimeInForce.GTC,
                quantity="0.01",
                price="5000",
                recvWindow=str(5000),
                timestamp=str(self.clock.timestamp_ms()),
            ),
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "POST"
        assert request["url"] == "https://api.binance.com/api/v3/order/test"
        assert request["params"].startswith(
            "symbol=ETHUSDT&side=SELL&type=LIMIT&timeInForce=GTC&quantity=0.01&price=5000&recvWindow=5000&timestamp=",
        )

    @pytest.mark.asyncio()
    async def test_new_order_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.new_order(
            symbol="ETHUSDT",
            side=BinanceOrderSide.SELL,
            order_type=BinanceOrderType.LIMIT,
            time_in_force=BinanceTimeInForce.GTC,
            quantity="0.01",
            price="5000",
            recv_window="5000",
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "POST"
        assert request["url"] == "https://api.binance.com/api/v3/order"
        assert request["params"].startswith(
            "symbol=ETHUSDT&side=SELL&type=LIMIT&timeInForce=GTC&quantity=0.01&price=5000&recvWindow=5000&timestamp=",
        )

    @pytest.mark.asyncio()
    async def test_cancel_order_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.cancel_order(
            symbol="ETHUSDT",
            order_id=1,
            recv_window="5000",
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "DELETE"
        assert request["url"] == "https://api.binance.com/api/v3/order"
        assert request["params"].startswith("symbol=ETHUSDT&orderId=1&recvWindow=5000&timestamp=")

    @pytest.mark.asyncio()
    async def test_cancel_all_open_orders_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.cancel_all_open_orders(
            symbol="ETHUSDT",
            recv_window="5000",
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "DELETE"
        assert request["url"] == "https://api.binance.com/api/v3/openOrders"
        assert request["params"].startswith("symbol=ETHUSDT&recvWindow=5000&timestamp=")

    @pytest.mark.asyncio()
    async def test_query_order_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_order(
            symbol="ETHUSDT",
            order_id=1,
            recv_window="5000",
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/order"
        assert request["params"].startswith("symbol=ETHUSDT&orderId=1&recvWindow=5000&timestamp=")

    @pytest.mark.asyncio()
    async def test_query_open_orders_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_open_orders(
            symbol="ETHUSDT",
            recv_window="5000",
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/openOrders"
        assert request["params"].startswith("symbol=ETHUSDT&recvWindow=5000&timestamp=")

    @pytest.mark.asyncio()
    async def test_query_all_orders_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_all_orders(
            symbol="ETHUSDT",
            recv_window="5000",
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/allOrders"
        assert request["params"].startswith("symbol=ETHUSDT&recvWindow=5000&timestamp=")

    @pytest.mark.asyncio()
    async def test_query_user_trades_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_user_trades(
            symbol="ETHUSDT",
            start_time=str(1600000000),
            end_time=str(1637355823),
            limit=1000,
            recv_window=str(5000),
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/myTrades"
        assert request["params"].startswith(
            "symbol=ETHUSDT&fromId=1&orderId=1&startTime=1600000000&endTime=1637355823&limit=1000&recvWindow=5000&timestamp=",
        )

    # SPOT/MARGIN tests

    @pytest.mark.asyncio()
    async def test_new_spot_oco_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.new_spot_oco(
            symbol="ETHUSDT",
            side=BinanceOrderSide.BUY,
            quantity="100",
            price="5000.00",
            stop_price="4000.00",
            list_client_order_id="1",
            limit_client_order_id="O-001",
            limit_iceberg_qty="50",
            stop_client_order_id="O-002",
            stop_limit_price="3500.00",
            stop_iceberg_qty="50",
            stop_limit_time_in_force=BinanceTimeInForce.GTC,
            recv_window="5000",
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "POST"
        assert request["url"] == "https://api.binance.com/api/v3/order/oco"
        assert request["params"].startswith(
            "symbol=ETHUSDT&side=BUY&quantity=100&price=5000.00&stopPrice=4000.00&listClientOrderId=1&limitClientOrderId=O-001&limitIcebergQty=50&stopClientOrderId=O-002&stopLimitPrice=3500.00&stopIcebergQty=50&stopLimitTimeInForce=GTC&recvWindow=5000&timestamp=",
        )

    @pytest.mark.asyncio()
    async def test_cancel_spot_oco_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.cancel_spot_oco(
            symbol="ETHUSDT",
            order_list_id="1",
            list_client_order_id="1",
            new_client_order_id="2",
            recv_window="5000",
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "DELETE"
        assert request["url"] == "https://api.binance.com/api/v3/orderList"
        assert request["params"].startswith(
            "symbol=ETHUSDT&orderListId=1&listClientOrderId=1&newClientOrderId=2&recvWindow=5000&timestamp=",
        )

    @pytest.mark.asyncio()
    async def test_query_spot_oco_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_spot_oco(
            order_list_id="1",
            orig_client_order_id="1",
            recv_window="5000",
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/orderList"
        assert request["params"].startswith(
            "orderListId=1&origClientOrderId=1&recvWindow=5000&timestamp=",
        )

    @pytest.mark.asyncio()
    async def test_query_spot_all_oco_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_spot_all_oco(
            start_time=str(1600000000),
            end_time=str(1637355823),
            limit=10,
            recv_window=str(5000),
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/allOrderList"
        assert request["params"].startswith(
            "startTime=1600000000&endTime=1637355823&limit=10&recvWindow=5000&timestamp=",
        )

    @pytest.mark.asyncio()
    async def test_query_spot_all_open_oco_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_spot_all_open_oco(recv_window=5000)

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/openOrderList"
        assert request["params"].startswith("recvWindow=5000&timestamp=")

    @pytest.mark.asyncio()
    async def test_query_spot_account_info_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.query_spot_account_info(recv_window=5000)

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/api/v3/account"
        assert request["params"].startswith("recvWindow=5000&timestamp=")

    @pytest.mark.asyncio()
    async def test_query_futures_hedge_mode_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.futures_api.query_futures_hedge_mode(recv_window=5000)

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://fapi.binance.com/fapi/v1/positionSide/dual"
        assert request["params"].startswith("recvWindow=5000&timestamp=")

    @pytest.mark.asyncio()
    async def test_query_futures_symbol_config_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.futures_api.query_futures_symbol_config(recv_window="5000")

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://fapi.binance.com/fapi/v1/symbolConfig"
        assert request["params"].startswith("recvWindow=5000&timestamp=")

    @pytest.mark.asyncio()
    async def test_query_futures_symbol_config_with_symbol_sends_expected_request(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.futures_api.query_futures_symbol_config(symbol="ETHUSDT", recv_window="5000")

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://fapi.binance.com/fapi/v1/symbolConfig"
        assert request["params"].startswith("symbol=ETHUSDT&recvWindow=5000&timestamp=")
