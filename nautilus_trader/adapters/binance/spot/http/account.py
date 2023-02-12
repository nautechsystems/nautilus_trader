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

from typing import Any, Optional

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceMethodType
from nautilus_trader.adapters.binance.common.enums import BinanceNewOrderRespType
from nautilus_trader.adapters.binance.common.enums import BinanceOrderSide
from nautilus_trader.adapters.binance.common.enums import BinanceSecurityType
from nautilus_trader.adapters.binance.common.enums import BinanceTimeInForce
from nautilus_trader.adapters.binance.common.schemas.market import BinanceRateLimit
from nautilus_trader.adapters.binance.common.schemas.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.http.account import BinanceAccountHttpAPI
from nautilus_trader.adapters.binance.http.account import BinanceOpenOrdersHttp
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.endpoint import BinanceHttpEndpoint
from nautilus_trader.adapters.binance.spot.schemas.account import BinanceSpotAccountInfo
from nautilus_trader.adapters.binance.spot.schemas.account import BinanceSpotOrderOco
from nautilus_trader.common.clock import LiveClock


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
            BinanceMethodType.GET: BinanceSecurityType.USER_DATA,
            BinanceMethodType.DELETE: BinanceSecurityType.TRADE,
        }
        super().__init__(
            client,
            base_endpoint,
            methods,
        )
        self._delete_resp_decoder = msgspec.json.Decoder()

    class DeleteParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters of openOrders SPOT/MARGIN DELETE request.
        Includes OCO orders.

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
        recvWindow: Optional[str] = None

    async def _delete(self, parameters: DeleteParameters) -> list[dict[str, Any]]:
        method_type = BinanceMethodType.DELETE
        raw = await self._method(method_type, parameters)
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
            BinanceMethodType.POST: BinanceSecurityType.TRADE,
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
        listClientOrderId: Optional[str] = None
        limitClientOrderId: Optional[str] = None
        limitStrategyId: Optional[int] = None
        limitStrategyType: Optional[int] = None
        limitIcebergQty: Optional[str] = None
        trailingDelta: Optional[str] = None
        stopClientOrderId: Optional[str] = None
        stopStrategyId: Optional[int] = None
        stopStrategyType: Optional[int] = None
        stopLimitPrice: Optional[str] = None
        stopIcebergQty: Optional[str] = None
        stopLimitTimeInForce: Optional[BinanceTimeInForce] = None
        newOrderRespType: Optional[BinanceNewOrderRespType] = None
        recvWindow: Optional[str] = None

    async def _post(self, parameters: PostParameters) -> BinanceSpotOrderOco:
        method_type = BinanceMethodType.POST
        raw = await self._method(method_type, parameters)
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
            BinanceMethodType.GET: BinanceSecurityType.USER_DATA,
            BinanceMethodType.DELETE: BinanceSecurityType.TRADE,
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
        orderList (OCO) GET endpoint parameters.

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
        orderListId: Optional[str] = None
        origClientOrderId: Optional[str] = None
        recvWindow: Optional[str] = None

    class DeleteParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        orderList (OCO) DELETE endpoint parameters.

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
        orderListId: Optional[str] = None
        listClientOrderId: Optional[str] = None
        newClientOrderId: Optional[str] = None
        recvWindow: Optional[str] = None

    async def _get(self, parameters: GetParameters) -> BinanceSpotOrderOco:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
        return self._resp_decoder.decode(raw)

    async def _delete(self, parameters: DeleteParameters) -> BinanceSpotOrderOco:
        method_type = BinanceMethodType.DELETE
        raw = await self._method(method_type, parameters)
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
            BinanceMethodType.GET: BinanceSecurityType.USER_DATA,
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
        fromId : str, optional
            The order ID for the request.
            If included, request will return orders from this orderId INCLUSIVE.
        startTime : str, optional
            The start time (UNIX milliseconds) filter for the request.
        endTime : str, optional
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
        fromId: Optional[str] = None
        startTime: Optional[str] = None
        endTime: Optional[str] = None
        limit: Optional[int] = None
        recvWindow: Optional[str] = None

    async def _get(self, parameters: GetParameters) -> list[BinanceSpotOrderOco]:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
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
            BinanceMethodType.GET: BinanceSecurityType.USER_DATA,
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
        recvWindow: Optional[str] = None

    async def _get(self, parameters: GetParameters) -> list[BinanceSpotOrderOco]:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
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
            BinanceMethodType.GET: BinanceSecurityType.USER_DATA,
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
        recvWindow: Optional[str] = None

    async def _get(self, parameters: GetParameters) -> BinanceSpotAccountInfo:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
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
            BinanceMethodType.GET: BinanceSecurityType.TRADE,
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
        recvWindow: Optional[str] = None

    async def _get(self, parameters: GetParameters) -> list[BinanceRateLimit]:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
        return self._resp_decoder.decode(raw)


class BinanceSpotAccountHttpAPI(BinanceAccountHttpAPI):
    """
    Provides access to the `Binance Spot/Margin` Account/Trade HTTP REST API.

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
                f"`BinanceAccountType` not SPOT, MARGIN_CROSS or MARGIN_ISOLATED, was {account_type}",  # pragma: no cover
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
        list_client_order_id: Optional[str] = None,
        limit_client_order_id: Optional[str] = None,
        limit_strategy_id: Optional[int] = None,
        limit_strategy_type: Optional[int] = None,
        limit_iceberg_qty: Optional[str] = None,
        trailing_delta: Optional[str] = None,
        stop_client_order_id: Optional[str] = None,
        stop_strategy_id: Optional[int] = None,
        stop_strategy_type: Optional[int] = None,
        stop_limit_price: Optional[str] = None,
        stop_iceberg_qty: Optional[str] = None,
        stop_limit_time_in_force: Optional[BinanceTimeInForce] = None,
        new_order_resp_type: Optional[BinanceNewOrderRespType] = None,
        recv_window: Optional[str] = None,
    ) -> BinanceSpotOrderOco:
        """Send in a new spot OCO order to Binance."""
        if stop_limit_price is not None and stop_limit_time_in_force is None:
            raise RuntimeError(
                "stopLimitPrice cannot be provided without stopLimitTimeInForce.",
            )
        if stop_limit_time_in_force == BinanceTimeInForce.GTX:
            raise RuntimeError(
                "stopLimitTimeInForce, Good Till Crossing (GTX) not supported.",
            )
        return await self._endpoint_spot_order_oco._post(
            parameters=self._endpoint_spot_order_oco.PostParameters(
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
        order_list_id: Optional[str] = None,
        orig_client_order_id: Optional[str] = None,
        recv_window: Optional[str] = None,
    ) -> BinanceSpotOrderOco:
        """Check single spot OCO order information."""
        if order_list_id is None and orig_client_order_id is None:
            raise RuntimeError(
                "Either orderListId or origClientOrderId must be provided.",
            )
        return await self._endpoint_spot_order_list._get(
            parameters=self._endpoint_spot_order_list.GetParameters(
                timestamp=self._timestamp(),
                orderListId=order_list_id,
                origClientOrderId=orig_client_order_id,
                recvWindow=recv_window,
            ),
        )

    async def cancel_all_open_orders(
        self,
        symbol: str,
        recv_window: Optional[str] = None,
    ) -> bool:
        """Cancel all active orders on a symbol, including OCO. Returns whether successful."""
        await self._endpoint_spot_open_orders._delete(
            parameters=self._endpoint_spot_open_orders.DeleteParameters(
                timestamp=self._timestamp(),
                symbol=BinanceSymbol(symbol),
                recvWindow=recv_window,
            ),
        )
        return True

    async def cancel_spot_oco(
        self,
        symbol: str,
        order_list_id: Optional[str] = None,
        list_client_order_id: Optional[str] = None,
        new_client_order_id: Optional[str] = None,
        recv_window: Optional[str] = None,
    ) -> BinanceSpotOrderOco:
        """Delete spot OCO order from Binance."""
        if order_list_id is None and list_client_order_id is None:
            raise RuntimeError(
                "Either orderListId or listClientOrderId must be provided.",
            )
        return await self._endpoint_spot_order_list._delete(
            parameters=self._endpoint_spot_order_list.DeleteParameters(
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
        from_id: Optional[str] = None,
        start_time: Optional[str] = None,
        end_time: Optional[str] = None,
        limit: Optional[int] = None,
        recv_window: Optional[str] = None,
    ) -> list[BinanceSpotOrderOco]:
        """Check all spot OCO orders' information, matching provided filter parameters."""
        if from_id is not None and (start_time or end_time) is not None:
            raise RuntimeError(
                "Cannot specify both fromId and a startTime/endTime.",
            )
        return await self._endpoint_spot_all_order_list._get(
            parameters=self._endpoint_spot_all_order_list.GetParameters(
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
        recv_window: Optional[str] = None,
    ) -> list[BinanceSpotOrderOco]:
        """Check all OPEN spot OCO orders' information."""
        return await self._endpoint_spot_open_order_list._get(
            parameters=self._endpoint_spot_open_order_list.GetParameters(
                timestamp=self._timestamp(),
                recvWindow=recv_window,
            ),
        )

    async def query_spot_account_info(
        self,
        recv_window: Optional[str] = None,
    ) -> BinanceSpotAccountInfo:
        """Check SPOT/MARGIN Binance account information."""
        return await self._endpoint_spot_account._get(
            parameters=self._endpoint_spot_account.GetParameters(
                timestamp=self._timestamp(),
                recvWindow=recv_window,
            ),
        )

    async def query_spot_order_rate_limit(
        self,
        recv_window: Optional[str] = None,
    ) -> list[BinanceRateLimit]:
        """Check SPOT/MARGIN order count/rateLimit."""
        return await self._endpoint_spot_order_rate_limit._get(
            parameters=self._endpoint_spot_order_rate_limit.GetParameters(
                timestamp=self._timestamp(),
                recvWindow=recv_window,
            ),
        )
