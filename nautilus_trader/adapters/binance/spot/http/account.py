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
from nautilus_trader.adapters.binance.common.enums import BinanceNewOrderRespType
from nautilus_trader.adapters.binance.common.enums import BinanceOrderSide
from nautilus_trader.adapters.binance.common.enums import BinanceSecurityType
from nautilus_trader.adapters.binance.common.enums import BinanceTimeInForce
from nautilus_trader.adapters.binance.common.schemas.market import BinanceRateLimit
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.http.account import BinanceAccountHttpAPI
from nautilus_trader.adapters.binance.http.account import BinanceOpenOrdersHttp
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.endpoint import BinanceHttpEndpoint
from nautilus_trader.adapters.binance.spot.schemas.account import BinanceSpotAccountInfo
from nautilus_trader.adapters.binance.spot.schemas.account import BinanceSpotOrderOco
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class BinanceSpotOpenOrdersHttp(BinanceOpenOrdersHttp):
    """
    Endpoint of all SPOT/MARGIN open orders on a symbol.

    `GET /api/v3/openOrders` (inherited)

    `DELETE /api/v3/openOrders`

    Warnings
    --------
    Care should be taken when accessing this endpoint with no symbol specified.
    The weight usage can be very large, which may cause rate limits to be hit.

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#current-open-orders-user_data
    https://binance-docs.github.io/apidocs/spot/en/#cancel-all-open-orders-on-a-symbol-trade

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.GET: BinanceSecurityType.USER_DATA,
            HttpMethod.DELETE: BinanceSecurityType.TRADE,
        }
        super().__init__(
            client,
            base_endpoint,
            methods,
        )
        self._delete_resp_decoder = msgspec.json.Decoder()

    class DeleteParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters of openOrders SPOT/MARGIN DELETE request. Includes OCO orders.

        Parameters
        ----------
        timestamp : str
            The millisecond timestamp of the request
        symbol : BinanceSymbol
            The symbol of the orders
        recvWindow : str, optional
            The response receive window for the request (cannot be greater than 60000).

        """

        timestamp: str
        symbol: BinanceSymbol
        recvWindow: str | None = None

    async def _delete(self, params: DeleteParameters) -> list[dict[str, Any]]:
        method_type = HttpMethod.DELETE
        raw = await self._method(method_type, params)
        return self._delete_resp_decoder.decode(raw)


class BinanceSpotOrderOcoHttp(BinanceHttpEndpoint):
    """
    Endpoint for creating SPOT/MARGIN OCO orders.

    `POST /api/v3/order/oco`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#new-oco-trade

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.POST: BinanceSecurityType.TRADE,
        }
        url_path = base_endpoint + "order/oco"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(BinanceSpotOrderOco)

    class PostParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        OCO order creation POST endpoint parameters.

        Parameters
        ----------
        symbol : BinanceSymbol
            The symbol of the order.
        timestamp : str
            The millisecond timestamp of the request.
        side : BinanceOrderSide
            The market side of the order (BUY, SELL).
        quantity : str
            The order quantity in base asset units for the request.
        price : str
            The order price for the request.
        stopPrice : str
            The order stop price for the request.
        listClientOrderId : str, optional
            A unique Id for the entire orderList
        limitClientOrderId : str, optional
            The client order ID for the limit request. A unique ID among open orders.
            Automatically generated if not provided.
        limitStrategyId : int,  optional
            The client strategy ID for the limit request.
        limitStrategyType : int, optional
            The client strategy type for the limit request. Cannot be less than 1000000
        limitIcebergQty : str, optional
            Create a limit iceberg order.
        trailingDelta : str, optional
            Can be used in addition to stopPrice.
            The order trailing delta of the request.
        stopClientOrderId : str, optional
            The client order ID for the stop request. A unique ID among open orders.
            Automatically generated if not provided.
        stopStrategyId : int,  optional
            The client strategy ID for the stop request.
        stopStrategyType : int, optional
            The client strategy type for the stop request. Cannot be less than 1000000.
        stopLimitPrice : str, optional
            Limit price for the stop order request.
            If provided, stopLimitTimeInForce is required.
        stopIcebergQty : str, optional
            Create a stop iceberg order.
        stopLimitTimeInForce : BinanceTimeInForce, optional
            The time in force of the stop limit order.
            Valid values: (GTC, FOK, IOC).
        newOrderRespType : BinanceNewOrderRespType, optional
            The response type for the order request.
        recvWindow : str, optional
            The response receive window in milliseconds for the request.
            Cannot exceed 60000.

        """

        symbol: BinanceSymbol
        timestamp: str
        side: BinanceOrderSide
        quantity: str
        price: str
        stopPrice: str
        listClientOrderId: str | None = None
        limitClientOrderId: str | None = None
        limitStrategyId: int | None = None
        limitStrategyType: int | None = None
        limitIcebergQty: str | None = None
        trailingDelta: str | None = None
        stopClientOrderId: str | None = None
        stopStrategyId: int | None = None
        stopStrategyType: int | None = None
        stopLimitPrice: str | None = None
        stopIcebergQty: str | None = None
        stopLimitTimeInForce: BinanceTimeInForce | None = None
        newOrderRespType: BinanceNewOrderRespType | None = None
        recvWindow: str | None = None

    async def _post(self, params: PostParameters) -> BinanceSpotOrderOco:
        method_type = HttpMethod.POST
        raw = await self._method(method_type, params)
        return self._resp_decoder.decode(raw)


class BinanceSpotOrderListHttp(BinanceHttpEndpoint):
    """
    Endpoint for querying and deleting SPOT/MARGIN OCO orders.

    `GET /api/v3/orderList`
    `DELETE /api/v3/orderList`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#query-oco-user_data
    https://binance-docs.github.io/apidocs/spot/en/#cancel-oco-trade

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.GET: BinanceSecurityType.USER_DATA,
            HttpMethod.DELETE: BinanceSecurityType.TRADE,
        }
        url_path = base_endpoint + "orderList"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(BinanceSpotOrderOco)

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        OrderList (OCO) GET endpoint parameters.

        Parameters
        ----------
        timestamp : str
            The millisecond timestamp of the request.
        orderListId : str, optional
            The unique identifier of the order list to retrieve.
        origClientOrderId : str, optional
            The client specified identifier of the order list to retrieve.
        recvWindow : str, optional
            The response receive window in milliseconds for the request.
            Cannot exceed 60000.

        NOTE: Either orderListId or origClientOrderId must be provided.

        """

        timestamp: str
        orderListId: str | None = None
        origClientOrderId: str | None = None
        recvWindow: str | None = None

    class DeleteParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        OrderList (OCO) DELETE endpoint parameters.

        Parameters
        ----------
        timestamp : str
            The millisecond timestamp of the request.
        symbol : BinanceSymbol
            The symbol of the order.
        orderListId : str, optional
            The unique identifier of the order list to retrieve.
        listClientOrderId : str, optional
            The client specified identifier of the order list to retrieve.
        newClientOrderId : str, optional
            Used to uniquely identify this cancel. Automatically generated
            by default.
        recvWindow : str, optional
            The response receive window in milliseconds for the request.
            Cannot exceed 60000.

        NOTE: Either orderListId or listClientOrderId must be provided.

        """

        timestamp: str
        symbol: BinanceSymbol
        orderListId: str | None = None
        listClientOrderId: str | None = None
        newClientOrderId: str | None = None
        recvWindow: str | None = None

    async def get(self, params: GetParameters) -> BinanceSpotOrderOco:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._resp_decoder.decode(raw)

    async def delete(self, params: DeleteParameters) -> BinanceSpotOrderOco:
        method_type = HttpMethod.DELETE
        raw = await self._method(method_type, params)
        return self._resp_decoder.decode(raw)


class BinanceSpotAllOrderListHttp(BinanceHttpEndpoint):
    """
    Endpoint for querying all SPOT/MARGIN OCO orders.

    `GET /api/v3/allOrderList`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#query-all-oco-user_data

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.GET: BinanceSecurityType.USER_DATA,
        }
        url_path = base_endpoint + "allOrderList"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(list[BinanceSpotOrderOco])

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters of allOrderList GET request.

        Parameters
        ----------
        timestamp : str
            The millisecond timestamp of the request.
        fromId : int, optional
            The order ID for the request.
            If included, request will return orders from this orderId INCLUSIVE.
        startTime : int, optional
            The start time (UNIX milliseconds) filter for the request.
        endTime : int, optional
            The end time (UNIX milliseconds) filter for the request.
        limit : int, optional
            The limit for the response.
            Default 500, max 1000
        recvWindow : str, optional
            The response receive window for the request (cannot be greater than 60000).

        Warnings
        --------
        If fromId is specified, neither startTime endTime can be provided.

        """

        timestamp: str
        fromId: int | None = None
        startTime: int | None = None
        endTime: int | None = None
        limit: int | None = None
        recvWindow: str | None = None

    async def get(self, params: GetParameters) -> list[BinanceSpotOrderOco]:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._resp_decoder.decode(raw)


class BinanceSpotOpenOrderListHttp(BinanceHttpEndpoint):
    """
    Endpoint for querying all SPOT/MARGIN OPEN OCO orders.

    `GET /api/v3/openOrderList`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#query-open-oco-user_data

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.GET: BinanceSecurityType.USER_DATA,
        }
        url_path = base_endpoint + "openOrderList"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(list[BinanceSpotOrderOco])

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters of allOrderList GET request.

        Parameters
        ----------
        timestamp : str
            The millisecond timestamp of the request.
        recvWindow : str, optional
            The response receive window for the request (cannot be greater than 60000).

        """

        timestamp: str
        recvWindow: str | None = None

    async def get(self, params: GetParameters) -> list[BinanceSpotOrderOco]:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._resp_decoder.decode(raw)


class BinanceSpotAccountHttp(BinanceHttpEndpoint):
    """
    Endpoint of current SPOT/MARGIN account information.

    `GET /api/v3/account`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#account-information-user_data

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.GET: BinanceSecurityType.USER_DATA,
        }
        url_path = base_endpoint + "account"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(BinanceSpotAccountInfo)

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters of account GET request.

        Parameters
        ----------
        timestamp : str
            The millisecond timestamp of the request.
        recvWindow : str, optional
            The response receive window for the request (cannot be greater than 60000).

        """

        timestamp: str
        recvWindow: str | None = None

    async def get(self, params: GetParameters) -> BinanceSpotAccountInfo:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._resp_decoder.decode(raw)


class BinanceSpotOrderRateLimitHttp(BinanceHttpEndpoint):
    """
    Endpoint of current SPOT/MARGIN order count usage for all intervals.

    `GET /api/v3/rateLimit/order`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#query-current-order-count-usage-trade

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.GET: BinanceSecurityType.TRADE,
        }
        url_path = base_endpoint + "rateLimit/order"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(list[BinanceRateLimit])

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters of rateLimit/order GET request.

        Parameters
        ----------
        timestamp : str
            The millisecond timestamp of the request.
        recvWindow : str, optional
            The response receive window for the request (cannot be greater than 60000).

        """

        timestamp: str
        recvWindow: str | None = None

    async def get(self, params: GetParameters) -> list[BinanceRateLimit]:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._resp_decoder.decode(raw)


class BinanceSpotAccountHttpAPI(BinanceAccountHttpAPI):
    """
    Provides access to the Binance Spot/Margin Account/Trade HTTP REST API.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance REST API client.
    clock : LiveClock,
        The clock for the API client.
    account_type : BinanceAccountType
        The Binance account type, used to select the endpoint prefix.

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        clock: LiveClock,
        account_type: BinanceAccountType = BinanceAccountType.SPOT,
    ):
        super().__init__(
            client=client,
            clock=clock,
            account_type=account_type,
        )

        if not account_type.is_spot_or_margin:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"`BinanceAccountType` not SPOT, MARGIN or ISOLATED_MARGIN, was {account_type}",  # pragma: no cover
            )

        # Create endpoints
        self._endpoint_spot_open_orders = BinanceSpotOpenOrdersHttp(client, self.base_endpoint)
        self._endpoint_spot_order_oco = BinanceSpotOrderOcoHttp(client, self.base_endpoint)
        self._endpoint_spot_order_list = BinanceSpotOrderListHttp(client, self.base_endpoint)
        self._endpoint_spot_all_order_list = BinanceSpotAllOrderListHttp(client, self.base_endpoint)
        self._endpoint_spot_open_order_list = BinanceSpotOpenOrderListHttp(
            client,
            self.base_endpoint,
        )
        self._endpoint_spot_account = BinanceSpotAccountHttp(client, self.base_endpoint)
        self._endpoint_spot_order_rate_limit = BinanceSpotOrderRateLimitHttp(
            client,
            self.base_endpoint,
        )

    async def new_spot_oco(
        self,
        symbol: str,
        side: BinanceOrderSide,
        quantity: str,
        price: str,
        stop_price: str,
        list_client_order_id: str | None = None,
        limit_client_order_id: str | None = None,
        limit_strategy_id: int | None = None,
        limit_strategy_type: int | None = None,
        limit_iceberg_qty: str | None = None,
        trailing_delta: str | None = None,
        stop_client_order_id: str | None = None,
        stop_strategy_id: int | None = None,
        stop_strategy_type: int | None = None,
        stop_limit_price: str | None = None,
        stop_iceberg_qty: str | None = None,
        stop_limit_time_in_force: BinanceTimeInForce | None = None,
        new_order_resp_type: BinanceNewOrderRespType | None = None,
        recv_window: str | None = None,
    ) -> BinanceSpotOrderOco:
        """
        Send in a new spot OCO order to Binance.
        """
        if stop_limit_price is not None and stop_limit_time_in_force is None:
            raise RuntimeError(
                "stopLimitPrice cannot be provided without stopLimitTimeInForce.",
            )
        if stop_limit_time_in_force == BinanceTimeInForce.GTX:
            raise RuntimeError(
                "stopLimitTimeInForce, Good Till Crossing (GTX) not supported.",
            )
        return await self._endpoint_spot_order_oco._post(
            params=self._endpoint_spot_order_oco.PostParameters(
                symbol=BinanceSymbol(symbol),
                timestamp=self._timestamp(),
                side=side,
                quantity=quantity,
                price=price,
                stopPrice=stop_price,
                listClientOrderId=list_client_order_id,
                limitClientOrderId=limit_client_order_id,
                limitStrategyId=limit_strategy_id,
                limitStrategyType=limit_strategy_type,
                limitIcebergQty=limit_iceberg_qty,
                trailingDelta=trailing_delta,
                stopClientOrderId=stop_client_order_id,
                stopStrategyId=stop_strategy_id,
                stopStrategyType=stop_strategy_type,
                stopLimitPrice=stop_limit_price,
                stopIcebergQty=stop_iceberg_qty,
                stopLimitTimeInForce=stop_limit_time_in_force,
                newOrderRespType=new_order_resp_type,
                recvWindow=recv_window,
            ),
        )

    async def query_spot_oco(
        self,
        order_list_id: str | None = None,
        orig_client_order_id: str | None = None,
        recv_window: str | None = None,
    ) -> BinanceSpotOrderOco:
        """
        Check single spot OCO order information.
        """
        if order_list_id is None and orig_client_order_id is None:
            raise RuntimeError(
                "Either orderListId or origClientOrderId must be provided.",
            )
        return await self._endpoint_spot_order_list.get(
            params=self._endpoint_spot_order_list.GetParameters(
                timestamp=self._timestamp(),
                orderListId=order_list_id,
                origClientOrderId=orig_client_order_id,
                recvWindow=recv_window,
            ),
        )

    async def cancel_all_open_orders(
        self,
        symbol: str,
        recv_window: str | None = None,
    ) -> bool:
        """
        Cancel all active orders on a symbol, including OCO.

        Returns whether successful.

        """
        await self._endpoint_spot_open_orders._delete(
            params=self._endpoint_spot_open_orders.DeleteParameters(
                timestamp=self._timestamp(),
                symbol=BinanceSymbol(symbol),
                recvWindow=recv_window,
            ),
        )
        return True

    async def cancel_spot_oco(
        self,
        symbol: str,
        order_list_id: str | None = None,
        list_client_order_id: str | None = None,
        new_client_order_id: str | None = None,
        recv_window: str | None = None,
    ) -> BinanceSpotOrderOco:
        """
        Delete spot OCO order from Binance.
        """
        if order_list_id is None and list_client_order_id is None:
            raise RuntimeError(
                "Either orderListId or listClientOrderId must be provided.",
            )
        return await self._endpoint_spot_order_list.delete(
            params=self._endpoint_spot_order_list.DeleteParameters(
                timestamp=self._timestamp(),
                symbol=BinanceSymbol(symbol),
                orderListId=order_list_id,
                listClientOrderId=list_client_order_id,
                newClientOrderId=new_client_order_id,
                recvWindow=recv_window,
            ),
        )

    async def query_spot_all_oco(
        self,
        from_id: int | None = None,
        start_time: int | None = None,
        end_time: int | None = None,
        limit: int | None = None,
        recv_window: str | None = None,
    ) -> list[BinanceSpotOrderOco]:
        """
        Check all spot OCO orders' information, matching provided filter parameters.
        """
        if from_id is not None and (start_time or end_time) is not None:
            raise RuntimeError(
                "Cannot specify both fromId and a startTime/endTime.",
            )
        return await self._endpoint_spot_all_order_list.get(
            params=self._endpoint_spot_all_order_list.GetParameters(
                timestamp=self._timestamp(),
                fromId=from_id,
                startTime=start_time,
                endTime=end_time,
                limit=limit,
                recvWindow=recv_window,
            ),
        )

    async def query_spot_all_open_oco(
        self,
        recv_window: str | None = None,
    ) -> list[BinanceSpotOrderOco]:
        """
        Check all OPEN spot OCO orders' information.
        """
        return await self._endpoint_spot_open_order_list.get(
            params=self._endpoint_spot_open_order_list.GetParameters(
                timestamp=self._timestamp(),
                recvWindow=recv_window,
            ),
        )

    async def query_spot_account_info(
        self,
        recv_window: str | None = None,
    ) -> BinanceSpotAccountInfo:
        """
        Check SPOT/MARGIN Binance account information.
        """
        return await self._endpoint_spot_account.get(
            params=self._endpoint_spot_account.GetParameters(
                timestamp=self._timestamp(),
                recvWindow=recv_window,
            ),
        )

    async def query_spot_order_rate_limit(
        self,
        recv_window: str | None = None,
    ) -> list[BinanceRateLimit]:
        """
        Check SPOT/MARGIN order count/rateLimit.
        """
        return await self._endpoint_spot_order_rate_limit.get(
            params=self._endpoint_spot_order_rate_limit.GetParameters(
                timestamp=self._timestamp(),
                recvWindow=recv_window,
            ),
        )
