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

from typing import Any

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceSecurityType
from nautilus_trader.adapters.binance.common.schemas.account import BinanceOrder
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.endpoint import BinanceHttpEndpoint
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class BinancePortfolioMarginOrderEndpoint(BinanceHttpEndpoint):
    """
    Endpoint for Portfolio Margin order operations.
    """

    def __init__(self, client: BinanceHttpClient, order_type: str):
        """
        Parameters
        ----------
        client : BinanceHttpClient
            The HTTP client.
        order_type : str
            Either "um" for USD-M futures, "cm" for COIN-M futures, or "margin" for margin orders.
        """
        methods = {
            HttpMethod.POST: BinanceSecurityType.TRADE,
            HttpMethod.PUT: BinanceSecurityType.TRADE,
            HttpMethod.DELETE: BinanceSecurityType.TRADE,
            HttpMethod.GET: BinanceSecurityType.USER_DATA,
        }
        url_path = f"/papi/v1/{order_type}/order"
        super().__init__(client, methods, url_path)
        self._resp_decoder = msgspec.json.Decoder(BinanceOrder)

    class PostParameters(msgspec.Struct, omit_defaults=True):
        """
        POST /papi/v1/{type}/order parameters.
        """

        symbol: BinanceSymbol
        side: str
        type: str
        timeInForce: str | None = None
        quantity: str | None = None
        quoteOrderQty: str | None = None
        price: str | None = None
        newClientOrderId: str | None = None
        stopPrice: str | None = None
        icebergQty: str | None = None
        newOrderRespType: str | None = None
        closePosition: str | None = None
        activationPrice: str | None = None
        callbackRate: str | None = None
        workingType: str | None = None
        priceProtect: str | None = None
        reduceOnly: str | None = None
        positionSide: str | None = None
        selfTradePreventionMode: str | None = None
        goodTillDate: int | None = None
        recvWindow: int | None = None
        timestamp: int | None = None

    class PutParameters(msgspec.Struct, omit_defaults=True):
        """
        PUT /papi/v1/{type}/order parameters.
        """

        symbol: BinanceSymbol
        side: str
        quantity: str | None = None
        price: str | None = None
        orderId: int | None = None
        origClientOrderId: str | None = None
        priceMatch: str | None = None
        recvWindow: int | None = None
        timestamp: int | None = None

    class DeleteParameters(msgspec.Struct, omit_defaults=True):
        """
        DELETE /papi/v1/{type}/order parameters.
        """

        symbol: BinanceSymbol
        orderId: int | None = None
        origClientOrderId: str | None = None
        newClientOrderId: str | None = None
        recvWindow: int | None = None
        timestamp: int | None = None

    class GetParameters(msgspec.Struct, omit_defaults=True):
        """
        GET /papi/v1/{type}/order parameters.
        """

        symbol: BinanceSymbol
        orderId: int | None = None
        origClientOrderId: str | None = None
        recvWindow: int | None = None
        timestamp: int | None = None

    async def post(self, parameters: PostParameters) -> BinanceOrder:
        method_type = HttpMethod.POST
        raw: bytes = await self._method(method_type, parameters)
        return self._resp_decoder.decode(raw)

    async def put(self, parameters: PutParameters) -> BinanceOrder:
        method_type = HttpMethod.PUT
        raw: bytes = await self._method(method_type, parameters)
        return self._resp_decoder.decode(raw)

    async def delete(self, parameters: DeleteParameters) -> BinanceOrder:
        method_type = HttpMethod.DELETE
        raw: bytes = await self._method(method_type, parameters)
        return self._resp_decoder.decode(raw)

    async def get(self, parameters: GetParameters) -> BinanceOrder:
        method_type = HttpMethod.GET
        raw: bytes = await self._method(method_type, parameters)
        return self._resp_decoder.decode(raw)


class BinancePortfolioMarginExecutionHttpAPI:
    """
    Provides access to the Binance Portfolio Margin HTTP execution endpoints.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance HTTP client.
    clock : LiveClock
        The clock for the client.
    account_type : BinanceAccountType
        The account type for the client.

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        clock: LiveClock,
        account_type: BinanceAccountType,
    ) -> None:
        self.client = client
        self._clock = clock
        self._account_type = account_type

        if account_type != BinanceAccountType.PORTFOLIO_MARGIN:
            raise ValueError(f"Invalid account_type: {account_type}")

        # Initialize endpoints
        self._um_order = BinancePortfolioMarginOrderEndpoint(client, "um")
        self._cm_order = BinancePortfolioMarginOrderEndpoint(client, "cm")
        self._margin_order = BinancePortfolioMarginOrderEndpoint(client, "margin")

    async def new_um_order(self, **kwargs) -> BinanceOrder:
        """
        Submit a new USD-M futures order.
        
        Returns
        -------
        BinanceOrder
            The order response.
        """
        timestamp = self._clock.timestamp_ns() // 1_000_000
        kwargs["timestamp"] = timestamp
        
        response = await self._um_order.post(
            BinancePortfolioMarginOrderEndpoint.PostParameters(**kwargs),
        )
        return response

    async def modify_um_order(self, **kwargs) -> BinanceOrder:
        """
        Modify an existing USD-M futures order.
        
        Returns
        -------
        BinanceOrder
            The order response.
        """
        timestamp = self._clock.timestamp_ns() // 1_000_000
        kwargs["timestamp"] = timestamp
        
        response = await self._um_order.put(
            BinancePortfolioMarginOrderEndpoint.PutParameters(**kwargs),
        )
        return response

    async def cancel_um_order(self, **kwargs) -> BinanceOrder:
        """
        Cancel a USD-M futures order.
        
        Returns
        -------
        BinanceOrder
            The order response.
        """
        timestamp = self._clock.timestamp_ns() // 1_000_000
        kwargs["timestamp"] = timestamp
        
        response = await self._um_order.delete(
            BinancePortfolioMarginOrderEndpoint.DeleteParameters(**kwargs),
        )
        return response

    async def new_cm_order(self, **kwargs) -> BinanceOrder:
        """
        Submit a new COIN-M futures order.
        
        Returns
        -------
        BinanceOrder
            The order response.
        """
        timestamp = self._clock.timestamp_ns() // 1_000_000
        kwargs["timestamp"] = timestamp
        
        response = await self._cm_order.post(
            BinancePortfolioMarginOrderEndpoint.PostParameters(**kwargs),
        )
        return response

    async def modify_cm_order(self, **kwargs) -> BinanceOrder:
        """
        Modify an existing COIN-M futures order.
        
        Returns
        -------
        BinanceOrder
            The order response.
        """
        timestamp = self._clock.timestamp_ns() // 1_000_000
        kwargs["timestamp"] = timestamp
        
        response = await self._cm_order.put(
            BinancePortfolioMarginOrderEndpoint.PutParameters(**kwargs),
        )
        return response

    async def cancel_cm_order(self, **kwargs) -> BinanceOrder:
        """
        Cancel a COIN-M futures order.
        
        Returns
        -------
        BinanceOrder
            The order response.
        """
        timestamp = self._clock.timestamp_ns() // 1_000_000
        kwargs["timestamp"] = timestamp
        
        response = await self._cm_order.delete(
            BinancePortfolioMarginOrderEndpoint.DeleteParameters(**kwargs),
        )
        return response

    async def new_margin_order(self, **kwargs) -> BinanceOrder:
        """
        Submit a new margin order.
        
        Returns
        -------
        BinanceOrder
            The order response.
        """
        timestamp = self._clock.timestamp_ns() // 1_000_000
        kwargs["timestamp"] = timestamp
        
        response = await self._margin_order.post(
            BinancePortfolioMarginOrderEndpoint.PostParameters(**kwargs),
        )
        return response

    async def cancel_margin_order(self, **kwargs) -> BinanceOrder:
        """
        Cancel a margin order.
        
        Returns
        -------
        BinanceOrder
            The order response.
        """
        timestamp = self._clock.timestamp_ns() // 1_000_000
        kwargs["timestamp"] = timestamp
        
        response = await self._margin_order.delete(
            BinancePortfolioMarginOrderEndpoint.DeleteParameters(**kwargs),
        )
        return response
