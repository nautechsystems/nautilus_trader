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

from typing import Optional

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceMethodType
from nautilus_trader.adapters.binance.common.enums import BinanceNewOrderRespType
from nautilus_trader.adapters.binance.common.enums import BinanceOrderSide
from nautilus_trader.adapters.binance.common.enums import BinanceOrderType
from nautilus_trader.adapters.binance.common.enums import BinanceSecurityType
from nautilus_trader.adapters.binance.common.enums import BinanceTimeInForce
from nautilus_trader.adapters.binance.common.schemas.account import BinanceOrder
from nautilus_trader.adapters.binance.common.schemas.account import BinanceUserTrade
from nautilus_trader.adapters.binance.common.schemas.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.endpoint import BinanceHttpEndpoint
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.core.correctness import PyCondition


class BinanceOrderHttp(BinanceHttpEndpoint):
    """
    Endpoint for managing orders.

    `GET /api/v3/order`
    `GET /api/v3/order/test`
    `GET /fapi/v1/order`
    `GET /dapi/v1/order`

    `POST /api/v3/order`
    `POST /fapi/v1/order`
    `POST /dapi/v1/order`

    `DELETE /api/v3/order`
    `DELETE /fapi/v1/order`
    `DELETE /dapi/v1/order`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#new-order-trade
    https://binance-docs.github.io/apidocs/futures/en/#new-order-trade
    https://binance-docs.github.io/apidocs/delivery/en/#new-order-trade
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
        testing_endpoint: Optional[bool] = False,
    ):
        methods = {
            BinanceMethodType.GET: BinanceSecurityType.USER_DATA,
            BinanceMethodType.POST: BinanceSecurityType.TRADE,
            BinanceMethodType.DELETE: BinanceSecurityType.TRADE,
        }
        url_path = base_endpoint + "order"
        if testing_endpoint:
            url_path = url_path + "/test"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(BinanceOrder)

    class GetDeleteParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Order management GET & DELETE endpoint parameters

        Parameters
        ----------
        symbol : BinanceSymbol
            The symbol of the order
        timestamp : str
            The millisecond timestamp of the request
        orderId : str, optional
            the order identifier
        origClientOrderId : str, optional
            the client specified order identifier
        recvWindow : str, optional
            the millisecond timeout window.

        Warnings
        --------
        Either orderId or origClientOrderId must be sent.
        """

        symbol: BinanceSymbol
        timestamp: str
        orderId: Optional[str] = None
        origClientOrderId: Optional[str] = None
        recvWindow: Optional[str] = None

    class PostParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Order creation POST endpoint parameters.

        Parameters
        ----------
        symbol : BinanceSymbol
            The symbol of the order
        timestamp : str
            The millisecond timestamp of the request
        side : BinanceOrderSide
            The market side of the order (BUY, SELL)
        type : BinanceOrderType
            The type of the order (LIMIT, STOP_LOSS..)
        timeInForce : BinanceTimeInForce, optional
            Mandatory for LIMIT, STOP_LOSS_LIMIT, TAKE_PROFIT_LIMIT orders.
            The time in force of the order (GTC, IOC..)
        quantity : str, optional
            Mandatory for all order types, except STOP_MARKET/TAKE_PROFIT_MARKET
            and TRAILING_STOP_MARKET orders
            The order quantity in base asset units for the request
        quoteOrderQty : str, optional
            Only for SPOT/MARGIN orders.
            Can be used alternatively to `quantity` for MARKET orders
            The order quantity in quote asset units for the request
        price : str, optional
            Mandatory for LIMIT, STOP_LOSS_LIMIT, TAKE_PROFIT_LIMIT, LIMIT_MAKER,
            STOP, TAKE_PROFIT orders
            The order price for the request
        newClientOrderId : str, optional
            The client order ID for the request. A unique ID among open orders.
            Automatically generated if not provided.
        strategyId : int,  optional
            Only for SPOT/MARGIN orders.
            The client strategy ID for the request.
        strategyType : int, optional
            Only for SPOT/MARGIN orders
            The client strategy type for thr request. Cannot be less than 1000000
        stopPrice : str, optional
            Mandatory for STOP_LOSS, STOP_LOSS_LIMIT, TAKE_PROFIT, TAKE_PROFIT_LIMIT,
            STOP, STOP_MARKET, TAKE_PROFIT_MARKET.
            The order stop price for the request.
        trailingDelta : str, optional
            Only for SPOT/MARGIN orders
            Can be used instead of or in addition to stopPrice for STOP_LOSS,
            STOP_LOSS_LIMIT, TAKE_PROFIT, TAKE_PROFIT_LIMIT orders.
            The order trailing delta of the request.
        icebergQty : str, optional
            Only for SPOT/MARGIN orders
            Can be used with LIMIT, STOP_LOSS_LIMIT, and TAKE_PROFIT_LIMIT to
            create an iceberg order.
        reduceOnly : str ('true', 'false'), optional
            Only for FUTURES orders
            Cannot be sent in Hedge Mode, cannot be sent with closePosition = 'true'
        closePosition : str ('true', 'false'), optional
            Only for FUTURES orders
            Can be used with STOP_MARKET or TAKE_PROFIT_MARKET orders
            Whether to close all open positions for the given symbol.
        activationPrice : str, optional
            Only for FUTURES orders
            Can be used with TRAILING_STOP_MARKET orders.
            Defaults to the latest price.
        callbackRate : str, optional
            Only for FUTURES orders
            Mandatory for TRAILING_STOP_MARKET orders.
            The order trailing delta of the request.
        workingType : str ("MARK_PRICE", "CONTRACT_PRICE"), optional
            Only for FUTURES orders
            The trigger type for the order.
            Defaults to "CONTRACT_PRICE"
        priceProtect : str ('true', 'false'), optional
            Only for FUTURES orders
            Whether price protection is active.
            Defaults to 'false'
        newOrderRespType : NewOrderRespType, optional
            The response type for the order request.
            SPOT/MARGIN MARKET, LIMIT orders default to FULL.
            All others default to ACK.
            FULL response only for SPOT/MARGIN orders.
        recvWindow : str, optional
            The response receive window in milliseconds for the request.
            Cannot exceed 60000.
        """

        symbol: BinanceSymbol
        timestamp: str
        side: BinanceOrderSide
        type: BinanceOrderType
        timeInForce: Optional[BinanceTimeInForce] = None
        quantity: Optional[str] = None
        quoteOrderQty: Optional[str] = None
        price: Optional[str] = None
        newClientOrderId: Optional[str] = None
        strategyId: Optional[int] = None
        strategyType: Optional[int] = None
        stopPrice: Optional[str] = None
        trailingDelta: Optional[str] = None
        icebergQty: Optional[str] = None
        reduceOnly: Optional[str] = None
        closePosition: Optional[str] = None
        activationPrice: Optional[str] = None
        callbackRate: Optional[str] = None
        workingType: Optional[str] = None
        priceProtect: Optional[str] = None
        newOrderRespType: Optional[BinanceNewOrderRespType] = None
        recvWindow: Optional[str] = None

    async def _get(self, parameters: GetDeleteParameters) -> BinanceOrder:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
        return self._resp_decoder.decode(raw)

    async def _delete(self, parameters: GetDeleteParameters) -> BinanceOrder:
        method_type = BinanceMethodType.DELETE
        raw = await self._method(method_type, parameters)
        return self._resp_decoder.decode(raw)

    async def _post(self, parameters: PostParameters) -> BinanceOrder:
        method_type = BinanceMethodType.POST
        raw = await self._method(method_type, parameters)
        return self._resp_decoder.decode(raw)


class BinanceAllOrdersHttp(BinanceHttpEndpoint):
    """
    Endpoint of all account orders, active, cancelled or filled.

    `GET /api/v3/allOrders`
    `GET /fapi/v1/allOrders`
    `GET /dapi/v1/allOrders`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#all-orders-user_data
    https://binance-docs.github.io/apidocs/futures/en/#all-orders-user_data
    https://binance-docs.github.io/apidocs/delivery/en/#all-orders-user_data
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            BinanceMethodType.GET: BinanceSecurityType.USER_DATA,
        }
        url_path = base_endpoint + "allOrders"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(list[BinanceOrder])

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters of allOrders GET request.

        Parameters
        ----------
        symbol : BinanceSymbol
            The symbol of the orders
        timestamp : str
            The millisecond timestamp of the request
        orderId : str, optional
            The order ID for the request.
            If included, request will return orders from this orderId INCLUSIVE
        startTime : str, optional
            The start time (UNIX milliseconds) filter for the request.
        endTime : str, optional
            The end time (UNIX milliseconds) filter for the request.
        limit : int, optional
            The limit for the response.
            Default 500, max 1000
        recvWindow : str, optional
            The response receive window for the request (cannot be greater than 60000).
        """

        symbol: BinanceSymbol
        timestamp: str
        orderId: Optional[str] = None
        startTime: Optional[str] = None
        endTime: Optional[str] = None
        limit: Optional[int] = None
        recvWindow: Optional[str] = None

    async def _get(self, parameters: GetParameters) -> list[BinanceOrder]:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
        return self._get_resp_decoder.decode(raw)


class BinanceOpenOrdersHttp(BinanceHttpEndpoint):
    """
    Endpoint of all open orders on a symbol.

    `GET /api/v3/openOrders`
    `GET /fapi/v1/openOrders`
    `GET /dapi/v1/openOrders`

    Warnings
    --------
    Care should be taken when accessing this endpoint with no symbol specified.
    The weight usage can be very large, which may cause rate limits to be hit.

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#current-open-orders-user_data
    https://binance-docs.github.io/apidocs/futures/en/#current-all-open-orders-user_data
    https://binance-docs.github.io/apidocs/futures/en/#current-all-open-orders-user_data
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
        methods: Optional[dict[BinanceMethodType, BinanceSecurityType]] = None,
    ):
        if methods is None:
            methods = {
                BinanceMethodType.GET: BinanceSecurityType.USER_DATA,
            }
        url_path = base_endpoint + "openOrders"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(list[BinanceOrder])

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters of openOrders GET request.

        Parameters
        ----------
        timestamp : str
            The millisecond timestamp of the request
        symbol : BinanceSymbol, optional
            The symbol of the orders
        recvWindow : str, optional
            The response receive window for the request (cannot be greater than 60000).
        """

        timestamp: str
        symbol: Optional[BinanceSymbol] = None
        recvWindow: Optional[str] = None

    async def _get(self, parameters: GetParameters) -> list[BinanceOrder]:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
        return self._get_resp_decoder.decode(raw)


class BinanceUserTradesHttp(BinanceHttpEndpoint):
    """
    Endpoint of trades for a specific account and symbol.

    `GET /api/v3/myTrades`
    `GET /fapi/v1/userTrades`
    `GET /dapi/v1/userTrades`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#account-trade-list-user_data
    https://binance-docs.github.io/apidocs/futures/en/#account-trade-list-user_data
    https://binance-docs.github.io/apidocs/delivery/en/#account-trade-list-user_data
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        url_path: str,
    ):
        methods = {
            BinanceMethodType.GET: BinanceSecurityType.USER_DATA,
        }
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(list[BinanceUserTrade])

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters of allOrders GET request.

        Parameters
        ----------
        symbol : BinanceSymbol
            The symbol of the orders
        timestamp : str
            The millisecond timestamp of the request
        orderId : str, optional
            The order ID for the request.
            If included, request will return orders from this orderId INCLUSIVE
        startTime : str, optional
            The start time (UNIX milliseconds) filter for the request.
        endTime : str, optional
            The end time (UNIX milliseconds) filter for the request.
        fromId : str, optional
            TradeId to fetch from. Default gets most recent trades.
        limit : int, optional
            The limit for the response.
            Default 500, max 1000
        recvWindow : str, optional
            The response receive window for the request (cannot be greater than 60000).
        """

        symbol: BinanceSymbol
        timestamp: str
        orderId: Optional[str] = None
        startTime: Optional[str] = None
        endTime: Optional[str] = None
        fromId: Optional[str] = None
        limit: Optional[int] = None
        recvWindow: Optional[str] = None

    async def _get(self, parameters: GetParameters) -> list[BinanceUserTrade]:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
        return self._get_resp_decoder.decode(raw)


class BinanceAccountHttpAPI:
    """
    Provides access to the Binance Account/Trade HTTP REST API.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance REST API client.
    account_type : BinanceAccountType
        The Binance account type, used to select the endpoint prefix

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        clock: LiveClock,
        account_type: BinanceAccountType,
    ):
        PyCondition.not_none(client, "client")
        self.client = client
        self._clock = clock

        if account_type.is_spot_or_margin:
            self.base_endpoint = "/api/v3/"
            user_trades_url = self.base_endpoint + "myTrades"
        elif account_type == BinanceAccountType.FUTURES_USDT:
            self.base_endpoint = "/fapi/v1/"
            user_trades_url = self.base_endpoint + "userTrades"
        elif account_type == BinanceAccountType.FUTURES_COIN:
            self.base_endpoint = "/dapi/v1/"
            user_trades_url = self.base_endpoint + "userTrades"
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"invalid `BinanceAccountType`, was {account_type}",  # pragma: no cover
            )

        # Create endpoints
        self._endpoint_order = BinanceOrderHttp(client, self.base_endpoint)
        self._endpoint_all_orders = BinanceAllOrdersHttp(client, self.base_endpoint)
        self._endpoint_open_orders = BinanceOpenOrdersHttp(client, self.base_endpoint)
        self._endpoint_user_trades = BinanceUserTradesHttp(client, user_trades_url)

    def _timestamp(self) -> str:
        """Create Binance timestamp from internal clock."""
        return str(self._clock.timestamp_ms())

    async def query_order(
        self,
        symbol: str,
        order_id: Optional[str] = None,
        orig_client_order_id: Optional[str] = None,
        recv_window: Optional[str] = None,
    ) -> BinanceOrder:
        """Check an order status."""
        if order_id is None and orig_client_order_id is None:
            raise RuntimeError(
                "Either orderId or origClientOrderId must be sent.",
            )
        binance_order = await self._endpoint_order._get(
            parameters=self._endpoint_order.GetDeleteParameters(
                symbol=BinanceSymbol(symbol),
                timestamp=self._timestamp(),
                orderId=order_id,
                origClientOrderId=orig_client_order_id,
                recvWindow=recv_window,
            ),
        )
        return binance_order

    async def cancel_all_open_orders(
        self,
        symbol: str,
        recv_window: Optional[str] = None,
    ) -> bool:
        # Implement in child class
        raise NotImplementedError

    async def cancel_order(
        self,
        symbol: str,
        order_id: Optional[str] = None,
        orig_client_order_id: Optional[str] = None,
        recv_window: Optional[str] = None,
    ) -> BinanceOrder:
        """Cancel an active order."""
        if order_id is None and orig_client_order_id is None:
            raise RuntimeError(
                "Either orderId or origClientOrderId must be sent.",
            )
        binance_order = await self._endpoint_order._delete(
            parameters=self._endpoint_order.GetDeleteParameters(
                symbol=BinanceSymbol(symbol),
                timestamp=self._timestamp(),
                orderId=order_id,
                origClientOrderId=orig_client_order_id,
                recvWindow=recv_window,
            ),
        )
        return binance_order

    async def new_order(
        self,
        symbol: str,
        side: BinanceOrderSide,
        order_type: BinanceOrderType,
        time_in_force: Optional[BinanceTimeInForce] = None,
        quantity: Optional[str] = None,
        quote_order_qty: Optional[str] = None,
        price: Optional[str] = None,
        new_client_order_id: Optional[str] = None,
        strategy_id: Optional[int] = None,
        strategy_type: Optional[int] = None,
        stop_price: Optional[str] = None,
        trailing_delta: Optional[str] = None,
        iceberg_qty: Optional[str] = None,
        reduce_only: Optional[str] = None,
        close_position: Optional[str] = None,
        activation_price: Optional[str] = None,
        callback_rate: Optional[str] = None,
        working_type: Optional[str] = None,
        price_protect: Optional[str] = None,
        new_order_resp_type: Optional[BinanceNewOrderRespType] = None,
        recv_window: Optional[str] = None,
    ) -> BinanceOrder:
        """Send in a new order to Binance."""
        binance_order = await self._endpoint_order._post(
            parameters=self._endpoint_order.PostParameters(
                symbol=BinanceSymbol(symbol),
                timestamp=self._timestamp(),
                side=side,
                type=order_type,
                timeInForce=time_in_force,
                quantity=quantity,
                quoteOrderQty=quote_order_qty,
                price=price,
                newClientOrderId=new_client_order_id,
                strategyId=strategy_id,
                strategyType=strategy_type,
                stopPrice=stop_price,
                trailingDelta=trailing_delta,
                icebergQty=iceberg_qty,
                reduceOnly=reduce_only,
                closePosition=close_position,
                activationPrice=activation_price,
                callbackRate=callback_rate,
                workingType=working_type,
                priceProtect=price_protect,
                newOrderRespType=new_order_resp_type,
                recvWindow=recv_window,
            ),
        )
        return binance_order

    async def query_all_orders(
        self,
        symbol: str,
        order_id: Optional[str] = None,
        start_time: Optional[str] = None,
        end_time: Optional[str] = None,
        limit: Optional[int] = None,
        recv_window: Optional[str] = None,
    ) -> list[BinanceOrder]:
        """Query all orders, active or filled."""
        return await self._endpoint_all_orders._get(
            parameters=self._endpoint_all_orders.GetParameters(
                symbol=BinanceSymbol(symbol),
                timestamp=self._timestamp(),
                orderId=order_id,
                startTime=start_time,
                endTime=end_time,
                limit=limit,
                recvWindow=recv_window,
            ),
        )

    async def query_open_orders(
        self,
        symbol: Optional[str] = None,
        recv_window: Optional[str] = None,
    ) -> list[BinanceOrder]:
        """Query open orders."""
        return await self._endpoint_open_orders._get(
            parameters=self._endpoint_open_orders.GetParameters(
                symbol=BinanceSymbol(symbol),
                timestamp=self._timestamp(),
                recvWindow=recv_window,
            ),
        )

    async def query_user_trades(
        self,
        symbol: str,
        order_id: Optional[str] = None,
        start_time: Optional[str] = None,
        end_time: Optional[str] = None,
        from_id: Optional[str] = None,
        limit: Optional[int] = None,
        recv_window: Optional[str] = None,
    ) -> list[BinanceUserTrade]:
        """Query user's trade history for a symbol, with provided filters."""
        if (order_id or from_id) is not None and (start_time or end_time) is not None:
            raise RuntimeError(
                "Cannot specify both order_id/from_id AND start_time/end_time parameters.",
            )
        return await self._endpoint_user_trades._get(
            parameters=self._endpoint_user_trades.GetParameters(
                symbol=BinanceSymbol(symbol),
                timestamp=self._timestamp(),
                orderId=order_id,
                startTime=start_time,
                endTime=end_time,
                fromId=from_id,
                limit=limit,
                recvWindow=recv_window,
            ),
        )
