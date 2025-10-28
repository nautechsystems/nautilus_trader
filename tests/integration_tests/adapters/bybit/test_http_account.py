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

import json
import pkgutil

import msgspec
import pytest

from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.http.account import BybitAccountHttpAPI
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.account.fee_rate import BybitFeeRateResponse
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.nautilus_pyo3 import HttpClient
from nautilus_trader.core.nautilus_pyo3 import HttpResponse
from tests.integration_tests.adapters.bybit.utils.get_mock import get_mock


class TestBybitAccountHttpApi:
    def setup(self):
        clock = LiveClock()
        self.client = BybitHttpClient(
            clock=clock,
            api_key="SOME_BYBIT_API_KEY",
            api_secret="SOME_BYBIT_API_SECRET",
            base_url="https://api-testnet.bybit.com",
        )
        self.http_api = BybitAccountHttpAPI(
            client=self.client,
            clock=clock,
        )

    @pytest.mark.asyncio()
    async def test_fee_rate(self, monkeypatch):
        response = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.http_responses",
            "fee_rate.json",
        )
        response_decoded = msgspec.json.Decoder(BybitFeeRateResponse).decode(response)

        monkeypatch.setattr(HttpClient, "request", get_mock(response))
        fee_rate = await self.http_api.fetch_fee_rate(
            product_type=BybitProductType.SPOT,
        )
        assert fee_rate == response_decoded.result.list

    @pytest.mark.asyncio()
    async def test_query_open_orders_pagination(self, monkeypatch):
        # Create mock responses for two pages
        page1_response = {
            "retCode": 0,
            "retMsg": "OK",
            "result": {
                "list": [
                    {
                        "orderId": "order1",
                        "orderLinkId": "link1",
                        "blockTradeId": "",
                        "symbol": "ETHUSDT",
                        "price": "4400.00",
                        "qty": "0.01",
                        "side": "Buy",
                        "isLeverage": "0",
                        "positionIdx": 0,
                        "orderStatus": "New",
                        "cancelType": "UNKNOWN",
                        "rejectReason": "EC_NoError",
                        "avgPrice": "",
                        "leavesQty": "0.01",
                        "leavesValue": "44.00",
                        "cumExecQty": "0",
                        "cumExecValue": "0",
                        "cumExecFee": "0",
                        "timeInForce": "GTC",
                        "orderType": "Limit",
                        "stopOrderType": "",
                        "orderIv": "",
                        "triggerPrice": "",
                        "takeProfit": "",
                        "stopLoss": "",
                        "tpTriggerBy": "",
                        "slTriggerBy": "",
                        "triggerDirection": 0,
                        "triggerBy": "",
                        "lastPriceOnCreated": "",
                        "reduceOnly": False,
                        "closeOnTrigger": False,
                        "smpType": "None",
                        "smpGroup": 0,
                        "smpOrderId": "",
                        "tpLimitPrice": "",
                        "slLimitPrice": "",
                        "placeType": "",
                        "createdTime": "1759977121915",
                        "updatedTime": "1759977121916",
                    },
                ],
                "nextPageCursor": "cursor123",
            },
            "time": 1759977121916,
        }

        page2_response = {
            "retCode": 0,
            "retMsg": "OK",
            "result": {
                "list": [
                    {
                        "orderId": "order2",
                        "orderLinkId": "link2",
                        "blockTradeId": "",
                        "symbol": "ETHUSDT",
                        "price": "4500.00",
                        "qty": "0.02",
                        "side": "Sell",
                        "isLeverage": "0",
                        "positionIdx": 0,
                        "orderStatus": "New",
                        "cancelType": "UNKNOWN",
                        "rejectReason": "EC_NoError",
                        "avgPrice": "",
                        "leavesQty": "0.02",
                        "leavesValue": "90.00",
                        "cumExecQty": "0",
                        "cumExecValue": "0",
                        "cumExecFee": "0",
                        "timeInForce": "GTC",
                        "orderType": "Limit",
                        "stopOrderType": "",
                        "orderIv": "",
                        "triggerPrice": "",
                        "takeProfit": "",
                        "stopLoss": "",
                        "tpTriggerBy": "",
                        "slTriggerBy": "",
                        "triggerDirection": 0,
                        "triggerBy": "",
                        "lastPriceOnCreated": "",
                        "reduceOnly": False,
                        "closeOnTrigger": False,
                        "smpType": "None",
                        "smpGroup": 0,
                        "smpOrderId": "",
                        "tpLimitPrice": "",
                        "slLimitPrice": "",
                        "placeType": "",
                        "createdTime": "1759977121918",
                        "updatedTime": "1759977121919",
                    },
                ],
                "nextPageCursor": "",
            },
            "time": 1759977121919,
        }

        # Track call count to return different responses
        call_count = 0

        async def mock_paginated_request(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            if call_count == 1:
                return HttpResponse(status=200, body=json.dumps(page1_response).encode())
            return HttpResponse(status=200, body=json.dumps(page2_response).encode())

        monkeypatch.setattr(HttpClient, "request", mock_paginated_request)

        # Execute query_open_orders which should paginate
        orders = await self.http_api.query_open_orders(
            product_type=BybitProductType.LINEAR,
        )

        # Verify both pages were fetched
        assert len(orders) == 2
        assert orders[0].orderId == "order1"
        assert orders[1].orderId == "order2"
        assert call_count == 2

    @pytest.mark.asyncio()
    async def test_query_trade_history_pagination(self, monkeypatch):
        # Create mock responses for two pages
        page1_response = {
            "retCode": 0,
            "retMsg": "OK",
            "result": {
                "list": [
                    {
                        "symbol": "ETHUSDT",
                        "orderId": "order1",
                        "orderLinkId": "link1",
                        "side": "Buy",
                        "orderPrice": "4400.00",
                        "orderQty": "0.01",
                        "leavesQty": "0",
                        "orderType": "Limit",
                        "stopOrderType": "",
                        "execFee": "0.0044",
                        "execId": "exec1",
                        "execPrice": "4400.00",
                        "execQty": "0.01",
                        "execType": "Trade",
                        "execValue": "44.00",
                        "execTime": "1759977121915",
                        "feeCurrency": "USDT",
                        "isMaker": True,
                        "feeRate": "0.0001",
                        "tradeIv": "",
                        "markIv": "",
                        "markPrice": "4400.00",
                        "indexPrice": "4400.00",
                        "underlyingPrice": "",
                        "blockTradeId": "",
                        "closedSize": "0",
                        "seq": 1,
                    },
                ],
                "nextPageCursor": "cursor456",
            },
            "time": 1759977121916,
        }

        page2_response = {
            "retCode": 0,
            "retMsg": "OK",
            "result": {
                "list": [
                    {
                        "symbol": "ETHUSDT",
                        "orderId": "order2",
                        "orderLinkId": "link2",
                        "side": "Sell",
                        "orderPrice": "4500.00",
                        "orderQty": "0.02",
                        "leavesQty": "0",
                        "orderType": "Limit",
                        "stopOrderType": "",
                        "execFee": "0.009",
                        "execId": "exec2",
                        "execPrice": "4500.00",
                        "execQty": "0.02",
                        "execType": "Trade",
                        "execValue": "90.00",
                        "execTime": "1759977121918",
                        "feeCurrency": "USDT",
                        "isMaker": False,
                        "feeRate": "0.0001",
                        "tradeIv": "",
                        "markIv": "",
                        "markPrice": "4500.00",
                        "indexPrice": "4500.00",
                        "underlyingPrice": "",
                        "blockTradeId": "",
                        "closedSize": "0",
                        "seq": 2,
                    },
                ],
                "nextPageCursor": "",
            },
            "time": 1759977121919,
        }

        # Track call count to return different responses
        call_count = 0

        async def mock_paginated_request(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            if call_count == 1:
                return HttpResponse(status=200, body=json.dumps(page1_response).encode())
            return HttpResponse(status=200, body=json.dumps(page2_response).encode())

        monkeypatch.setattr(HttpClient, "request", mock_paginated_request)

        # Execute query_trade_history which should paginate
        executions = await self.http_api.query_trade_history(
            product_type=BybitProductType.LINEAR,
        )

        # Verify both pages were fetched
        assert len(executions) == 2
        assert executions[0].execId == "exec1"
        assert executions[1].execId == "exec2"
        assert call_count == 2
