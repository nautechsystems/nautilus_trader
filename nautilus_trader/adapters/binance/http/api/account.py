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
#
#  Heavily refactored from MIT licensed github.com/binance/binance-connector-python
#  Original author: Jeremy https://github.com/2pd
# -------------------------------------------------------------------------------------------------

from typing import Any, Dict, Optional

from nautilus_trader.adapters.binance.core.enums import BinanceAccountType
from nautilus_trader.adapters.binance.core.functions import format_symbol
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.enums import NewOrderRespType
from nautilus_trader.core.correctness import PyCondition


class BinanceAccountHttpAPI:
    """
    Provides access to the `Binance Account/Trade` HTTP REST API.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance REST API client.
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        account_type: BinanceAccountType = BinanceAccountType.SPOT,
    ):
        PyCondition.not_none(client, "client")

        self.client = client

        if account_type in (BinanceAccountType.SPOT, BinanceAccountType.MARGIN):
            self.BASE_ENDPOINT = "/api/v3/"
        elif account_type == BinanceAccountType.FUTURES_USDT:
            self.BASE_ENDPOINT = "/fapi/v1/"
        elif account_type == BinanceAccountType.FUTURES_COIN:
            self.BASE_ENDPOINT = "/dapi/v1/"
        else:  # pragma: no cover (design-time error)
            raise RuntimeError(f"invalid Binance account type, was {account_type}")

    async def change_position_mode(
        self,
        is_dual_side_position: bool,
        recv_window: Optional[int] = None,
    ):
        """
        Change Position Mode (TRADE).

        `POST /fapi/v1/positionSide/dual (HMAC SHA256)`.

        Parameters
        ----------
        is_dual_side_position : bool
            If `Hedge Mode` will be set, otherwise `One-way` Mode.
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/futures/en/#change-position-mode-trade

        """
        payload: Dict[str, str] = {
            "dualSidePosition": str(is_dual_side_position).lower(),
        }
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return await self.client.sign_request(
            http_method="POST",
            url_path=self.BASE_ENDPOINT + "positionSide/dual",
            payload=payload,
        )

    async def get_position_mode(
        self,
        recv_window: Optional[int] = None,
    ):
        """
        Get Current Position Mode (USER_DATA).

        `GET /fapi/v1/positionSide/dual (HMAC SHA256)`.

        Parameters
        ----------
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

        References
        ----------
        https://binance-docs.github.io/apidocs/futures/en/#get-current-position-mode-user_data
        """
        payload: Dict[str, str] = {}
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return await self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "positionSide/dual",
            payload=payload,
        )

    async def new_order_test_spot(
        self,
        symbol: str,
        side: str,
        type: str,
        time_in_force: Optional[str] = None,
        quantity: Optional[str] = None,
        quote_order_qty: Optional[str] = None,
        price: Optional[str] = None,
        new_client_order_id: Optional[str] = None,
        stop_price: Optional[str] = None,
        iceberg_qty: Optional[str] = None,
        new_order_resp_type: NewOrderRespType = None,
        recv_window: Optional[int] = None,
    ) -> Dict[str, Any]:
        """
        Test new order creation and signature/recvWindow.

        Creates and validates a new order but does not send it into the matching engine.

        Test New Order (TRADE).
        `POST /api/v3/order/test`.

        Parameters
        ----------
        symbol : str
            The symbol for the request.
        side : str
            The order side for the request.
        type : str
            The order type for the request.
        time_in_force : str, optional
            The order time in force for the request.
        quantity : str, optional
            The order quantity in base asset units for the request.
        quote_order_qty : str, optional
            The order quantity in quote asset units for the request.
        price : str, optional
            The order price for the request.
        new_client_order_id : str, optional
            The client order ID for the request. A unique ID among open orders.
            Automatically generated if not provided.
        stop_price : str, optional
            The order stop price for the request.
            Used with STOP_LOSS, STOP_LOSS_LIMIT, TAKE_PROFIT, and TAKE_PROFIT_LIMIT orders.
        iceberg_qty : str, optional
            The order iceberg (display) quantity for the request.
            Used with LIMIT, STOP_LOSS_LIMIT, and TAKE_PROFIT_LIMIT to create an iceberg order.
        new_order_resp_type : NewOrderRespType, optional
            The response type for the order request.
            MARKET and LIMIT order types default to FULL, all other orders default to ACK.
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#test-new-order-trade

        """
        payload: Dict[str, str] = {
            "symbol": format_symbol(symbol),
            "side": side,
            "type": type,
        }
        if time_in_force is not None:
            payload["timeInForce"] = time_in_force
        if quantity is not None:
            payload["quantity"] = quantity
        if quote_order_qty is not None:
            payload["quoteOrderQty"] = quote_order_qty
        if price is not None:
            payload["price"] = price
        if new_client_order_id is not None:
            payload["newClientOrderId"] = new_client_order_id
        if stop_price is not None:
            payload["stopPrice"] = stop_price
        if iceberg_qty is not None:
            payload["icebergQty"] = iceberg_qty
        if new_order_resp_type is not None:
            payload["newOrderRespType"] = new_order_resp_type.value
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return await self.client.sign_request(
            http_method="POST",
            url_path=self.BASE_ENDPOINT + "order/test",
            payload=payload,
        )

    async def new_order_spot(
        self,
        symbol: str,
        side: str,
        type: str,
        time_in_force: Optional[str] = None,
        quantity: Optional[str] = None,
        quote_order_qty: Optional[str] = None,
        price: Optional[str] = None,
        new_client_order_id: Optional[str] = None,
        stop_price: Optional[str] = None,
        iceberg_qty: Optional[str] = None,
        new_order_resp_type: NewOrderRespType = None,
        recv_window: Optional[int] = None,
    ) -> Dict[str, Any]:
        """
        Submit a new order.

        Submit New Order (TRADE).
        `POST /api/v3/order`.

        Parameters
        ----------
        symbol : str
            The symbol for the request.
        side : str
            The order side for the request.
        type : str
            The order type for the request.
        time_in_force : str, optional
            The order time in force for the request.
        quantity : str, optional
            The order quantity in base asset units for the request.
        quote_order_qty : str, optional
            The order quantity in quote asset units for the request.
        price : str, optional
            The order price for the request.
        new_client_order_id : str, optional
            The client order ID for the request. A unique ID among open orders.
            Automatically generated if not provided.
        stop_price : str, optional
            The order stop price for the request.
            Used with STOP_LOSS, STOP_LOSS_LIMIT, TAKE_PROFIT, and TAKE_PROFIT_LIMIT orders.
        iceberg_qty : str, optional
            The order iceberg (display) quantity for the request.
            Used with LIMIT, STOP_LOSS_LIMIT, and TAKE_PROFIT_LIMIT to create an iceberg order.
        new_order_resp_type : NewOrderRespType, optional
            The response type for the order request.
            MARKET and LIMIT order types default to FULL, all other orders default to ACK.
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#new-order-trade

        """
        payload: Dict[str, str] = {
            "symbol": format_symbol(symbol),
            "side": side,
            "type": type,
        }
        if time_in_force is not None:
            payload["timeInForce"] = time_in_force
        if quantity is not None:
            payload["quantity"] = quantity
        if quote_order_qty is not None:
            payload["quoteOrderQty"] = quote_order_qty
        if price is not None:
            payload["price"] = price
        if new_client_order_id is not None:
            payload["newClientOrderId"] = new_client_order_id
        if stop_price is not None:
            payload["stopPrice"] = stop_price
        if iceberg_qty is not None:
            payload["icebergQty"] = iceberg_qty
        if new_order_resp_type is not None:
            payload["newOrderRespType"] = new_order_resp_type.value
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return await self.client.sign_request(
            http_method="POST",
            url_path=self.BASE_ENDPOINT + "order",
            payload=payload,
        )

    async def new_order_futures(  # noqa (too complex)
        self,
        symbol: str,
        side: str,
        type: str,
        position_side: Optional[str] = "BOTH",
        time_in_force: Optional[str] = None,
        quantity: Optional[str] = None,
        reduce_only: Optional[bool] = False,
        price: Optional[str] = None,
        new_client_order_id: Optional[str] = None,
        stop_price: Optional[str] = None,
        close_position: Optional[bool] = None,
        activation_price: Optional[str] = None,
        callback_rate: Optional[str] = None,
        working_type: Optional[str] = None,
        price_protect: Optional[bool] = None,
        new_order_resp_type: NewOrderRespType = None,
        recv_window: Optional[int] = None,
    ) -> Dict[str, Any]:
        """
        Submit a new order.

        Submit New Order (TRADE).
        `POST /api/v3/order`.

        Parameters
        ----------
        symbol : str
            The symbol for the request.
        side : str
            The order side for the request.
        type : str
            The order type for the request.
        position_side : str, {'BOTH', 'LONG', 'SHORT'}, default BOTH
            The position side for the order.
        time_in_force : str, optional
            The order time in force for the request.
        quantity : str, optional
            The order quantity in base asset units for the request.
        reduce_only : bool, optional
            If the order will only reduce a position.
        price : str, optional
            The order price for the request.
        new_client_order_id : str, optional
            The client order ID for the request. A unique ID among open orders.
            Automatically generated if not provided.
        stop_price : str, optional
            The order stop price for the request.
            Used with STOP_LOSS, STOP_LOSS_LIMIT, TAKE_PROFIT, and TAKE_PROFIT_LIMIT orders.
        close_position : bool, optional
            If close all open positions for the given symbol.
        activation_price : str, optional.
            The price to activate a trailing stop.
            Used with TRAILING_STOP_MARKET orders, default as the latest price(supporting different workingType).
        callback_rate : str, optional
            The percentage to trail the stop.
            Used with TRAILING_STOP_MARKET orders, min 0.1, max 5 where 1 for 1%.
        working_type : str {'MARK_PRICE', 'CONTRACT_PRICE'}, optional
            The trigger type for the order. API default "CONTRACT_PRICE".
        price_protect : bool, optional
            If price protection is active.
        new_order_resp_type : NewOrderRespType, optional
            The response type for the order request.
            MARKET and LIMIT order types default to FULL, all other orders default to ACK.
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#new-order-trade

        """
        payload: Dict[str, str] = {
            "symbol": format_symbol(symbol),
            "side": side,
            "type": type,
        }
        if position_side is not None:
            payload["positionSide"] = position_side
        if time_in_force is not None:
            payload["timeInForce"] = time_in_force
        if quantity is not None:
            payload["quantity"] = quantity
        if reduce_only is not None:
            payload["reduce_only"] = str(reduce_only).lower()
        if price is not None:
            payload["price"] = price
        if new_client_order_id is not None:
            payload["newClientOrderId"] = new_client_order_id
        if stop_price is not None:
            payload["stopPrice"] = stop_price
        if close_position is not None:
            payload["closePosition"] = str(close_position).lower()
        if activation_price is not None:
            payload["activationPrice"] = activation_price
        if callback_rate is not None:
            payload["callbackRate"] = callback_rate
        if working_type is not None:
            payload["workingType"] = working_type
        if price_protect is not None:
            payload["priceProtect"] = str(price_protect).lower()
        if new_order_resp_type is not None:
            payload["newOrderRespType"] = new_order_resp_type.value
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return await self.client.sign_request(
            http_method="POST",
            url_path=self.BASE_ENDPOINT + "order",
            payload=payload,
        )

    async def cancel_order(
        self,
        symbol: str,
        order_id: Optional[str] = None,
        orig_client_order_id: Optional[str] = None,
        new_client_order_id: Optional[str] = None,
        recv_window: Optional[int] = None,
    ) -> Dict[str, Any]:
        """
        Cancel an open order.

        Cancel Order (TRADE).
        `DELETE /api/v3/order`.

        Parameters
        ----------
        symbol : str
            The symbol for the request.
        order_id : str, optional
            The order ID to cancel.
        orig_client_order_id : str, optional
            The original client order ID to cancel.
        new_client_order_id : str, optional
            The new client order ID to uniquely identify this request.
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#cancel-order-trade

        """
        payload: Dict[str, str] = {"symbol": format_symbol(symbol)}
        if order_id is not None:
            payload["orderId"] = str(order_id)
        if orig_client_order_id is not None:
            payload["origClientOrderId"] = str(orig_client_order_id)
        if new_client_order_id is not None:
            payload["newClientOrderId"] = str(new_client_order_id)
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return await self.client.sign_request(
            http_method="DELETE",
            url_path=self.BASE_ENDPOINT + "order",
            payload=payload,
        )

    async def cancel_open_orders(
        self,
        symbol: str,
        recv_window: Optional[int] = None,
    ) -> Dict[str, Any]:
        """
        Cancel all open orders for a symbol. This includes OCO orders.

        Cancel all Open Orders for a Symbol (TRADE).
        `DELETE api/v3/openOrders`.

        Parameters
        ----------
        symbol : str
            The symbol for the request.
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#cancel-all-open-orders-on-a-symbol-trade

        """
        payload: Dict[str, str] = {"symbol": format_symbol(symbol)}
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return await self.client.sign_request(
            http_method="DELETE",
            url_path=self.BASE_ENDPOINT + "openOrders",
            payload=payload,
        )

    async def get_order(
        self,
        symbol: str,
        order_id: Optional[str] = None,
        orig_client_order_id: Optional[str] = None,
        recv_window: Optional[int] = None,
    ) -> Dict[str, Any]:
        """
        Check an order's status.

        Query Order (USER_DATA).
        `GET /api/v3/order`.

        Parameters
        ----------
        symbol : str
            The symbol for the request.
        order_id : str, optional
            The order ID for the request.
        orig_client_order_id : str, optional
            The original client order ID for the request.
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#query-order-user_data

        """
        payload: Dict[str, str] = {"symbol": format_symbol(symbol)}
        if order_id is not None:
            payload["orderId"] = order_id
        if orig_client_order_id is not None:
            payload["origClientOrderId"] = orig_client_order_id
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return await self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "order",
            payload=payload,
        )

    async def get_open_orders(
        self,
        symbol: Optional[str] = None,
        recv_window: Optional[int] = None,
    ) -> Dict[str, Any]:
        """
        Get all open orders on a symbol.

        Query Current Open Orders (USER_DATA).
        `GET /api/v3/openOrders`.

        Parameters
        ----------
        symbol : str, optional
            The symbol for the request.
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#current-open-orders-user_data

        """
        payload: Dict[str, str] = {}
        if symbol is not None:
            payload["symbol"] = format_symbol(symbol)
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return await self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "openOrders",
            payload=payload,
        )

    async def get_orders(
        self,
        symbol: str,
        order_id: Optional[str] = None,
        start_time: Optional[int] = None,
        end_time: Optional[int] = None,
        limit: Optional[int] = None,
        recv_window: Optional[int] = None,
    ) -> Dict[str, Any]:
        """
        Get all account orders; open, or closed.

        All Orders (USER_DATA).
        `GET /api/v3/allOrders`.

        Parameters
        ----------
        symbol : str
            The symbol for the request.
        order_id : str, optional
            The order ID for the request.
        start_time : int, optional
            The start time (UNIX milliseconds) filter for the request.
        end_time : int, optional
            The end time (UNIX milliseconds) filter for the request.
        limit : int, optional
            The limit for the response.
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#all-orders-user_data

        """
        payload: Dict[str, str] = {"symbol": format_symbol(symbol)}
        if order_id is not None:
            payload["orderId"] = order_id
        if start_time is not None:
            payload["startTime"] = str(start_time)
        if end_time is not None:
            payload["endTime"] = str(end_time)
        if limit is not None:
            payload["limit"] = str(limit)
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return await self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "allOrders",
            payload=payload,
        )

    async def new_oco_order(
        self,
        symbol: str,
        side: str,
        quantity: str,
        price: str,
        stop_price: str,
        list_client_order_id: Optional[str] = None,
        limit_client_order_id: Optional[str] = None,
        limit_iceberg_qty: Optional[str] = None,
        stop_client_order_id: Optional[str] = None,
        stop_limit_price: Optional[str] = None,
        stop_iceberg_qty: Optional[str] = None,
        stop_limit_time_in_force: Optional[str] = None,
        new_order_resp_type: NewOrderRespType = None,
        recv_window: Optional[int] = None,
    ) -> Dict[str, Any]:
        """
        Submit a new OCO order.

        Submit New OCO (TRADE).
        `POST /api/v3/order/oco`.

        Parameters
        ----------
        symbol : str
            The symbol for the request.
        side : str
            The order side for the request.
        quantity : str
            The order quantity for the request.
        price : str
            The order price for the request.
        stop_price : str
            The order stop price for the request.
        list_client_order_id : str, optional
            The list client order ID for the request.
        limit_client_order_id : str, optional
            The LIMIT client order ID for the request.
        limit_iceberg_qty : str, optional
            The LIMIT order display quantity for the request.
        stop_client_order_id : str, optional
            The STOP order client order ID for the request.
        stop_limit_price : str, optional
            The STOP_LIMIT price for the request.
        stop_iceberg_qty : str, optional
            The STOP order display quantity for the request.
        stop_limit_time_in_force : str, optional
            The STOP_LIMIT time_in_force for the request.
        new_order_resp_type : NewOrderRespType, optional
            The response type for the order request.
            MARKET and LIMIT order types default to FULL, all other orders default to ACK.
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#new-oco-trade

        """
        payload: Dict[str, str] = {
            "symbol": format_symbol(symbol),
            "side": side,
            "quantity": quantity,
            "price": price,
            "stopPrice": stop_price,
        }
        if list_client_order_id is not None:
            payload["listClientOrderId"] = list_client_order_id
        if limit_client_order_id is not None:
            payload["limitClientOrderId"] = limit_client_order_id
        if limit_iceberg_qty is not None:
            payload["limitIcebergQty"] = limit_iceberg_qty
        if stop_client_order_id is not None:
            payload["stopClientOrderId"] = stop_client_order_id
        if stop_limit_price is not None:
            payload["stopLimitPrice"] = stop_limit_price
        if stop_iceberg_qty is not None:
            payload["stopIcebergQty"] = stop_iceberg_qty
        if stop_limit_time_in_force is not None:
            payload["stopLimitTimeInForce"] = stop_limit_time_in_force
        if new_order_resp_type is not None:
            payload["new_order_resp_type"] = new_order_resp_type.value
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return await self.client.sign_request(
            http_method="POST",
            url_path=self.BASE_ENDPOINT + "order/oco",
            payload=payload,
        )

    async def cancel_oco_order(
        self,
        symbol: str,
        order_list_id: Optional[str] = None,
        list_client_order_id: Optional[str] = None,
        new_client_order_id: Optional[str] = None,
        recv_window: Optional[int] = None,
    ) -> Dict[str, Any]:
        """
        Cancel an entire Order List.

        Either `order_list_id` or `list_client_order_id` must be provided.

        Cancel OCO (TRADE).
        `DELETE /api/v3/orderList`.

        Parameters
        ----------
        symbol : str
            The symbol for the request.
        order_list_id : str, optional
            The order list ID for the request.
        list_client_order_id : str, optional
            The list client order ID for the request.
        new_client_order_id : str, optional
            The new client order ID to uniquely identify this request.
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#cancel-oco-trade

        """
        payload: Dict[str, str] = {"symbol": format_symbol(symbol)}
        if order_list_id is not None:
            payload["orderListId"] = order_list_id
        if list_client_order_id is not None:
            payload["listClientOrderId"] = list_client_order_id
        if new_client_order_id is not None:
            payload["newClientOrderId"] = new_client_order_id
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return await self.client.sign_request(
            http_method="DELETE",
            url_path=self.BASE_ENDPOINT + "orderList",
            payload=payload,
        )

    async def get_oco_order(
        self,
        order_list_id: Optional[str],
        orig_client_order_id: Optional[str],
        recv_window: Optional[int] = None,
    ) -> Dict[str, Any]:
        """
        Retrieve a specific OCO based on provided optional parameters.

        Either `order_list_id` or `orig_client_order_id` must be provided.

        Query OCO (USER_DATA).
        `GET /api/v3/orderList`.

        Parameters
        ----------
        order_list_id : str, optional
            The order list ID for the request.
        orig_client_order_id : str, optional
            The original client order ID for the request.
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#query-oco-user_data

        """
        payload: Dict[str, str] = {}
        if order_list_id is not None:
            payload["orderListId"] = order_list_id
        if orig_client_order_id is not None:
            payload["origClientOrderId"] = orig_client_order_id
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return await self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "orderList",
            payload=payload,
        )

    async def get_oco_orders(
        self,
        from_id: Optional[str] = None,
        start_time: Optional[int] = None,
        end_time: Optional[int] = None,
        limit: Optional[int] = None,
        recv_window: Optional[int] = None,
    ) -> Dict[str, Any]:
        """
        Retrieve all OCO based on provided optional parameters.

        If `from_id` is provided then neither `start_time` nor `end_time` can be
        provided.

        Query all OCO (USER_DATA).
        `GET /api/v3/allOrderList`.

        Parameters
        ----------
        from_id : int, optional
            The order ID filter for the request.
        start_time : int, optional
            The start time (UNIX milliseconds) filter for the request.
        end_time : int, optional
            The end time (UNIX milliseconds) filter for the request.
        limit : int, optional
            The limit for the response.
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#query-all-oco-user_data

        """
        payload: Dict[str, str] = {}
        if from_id is not None:
            payload["fromId"] = from_id
        if start_time is not None:
            payload["startTime"] = str(start_time)
        if end_time is not None:
            payload["endTime"] = str(end_time)
        if limit is not None:
            payload["limit"] = str(limit)
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return await self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "allOrderList",
            payload=payload,
        )

    async def get_oco_open_orders(self, recv_window: Optional[int] = None) -> Dict[str, Any]:
        """
        Get all open OCO orders.

        Query Open OCO (USER_DATA).
        GET /api/v3/openOrderList.

        Parameters
        ----------
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#query-open-oco-user_data

        """
        payload: Dict[str, str] = {}
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return await self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "openOrderList",
            payload=payload,
        )

    async def account(self, recv_window: Optional[int] = None) -> Dict[str, Any]:
        """
        Get current account information.

        Account Information (USER_DATA).
        `GET /api/v3/account`.

        Parameters
        ----------
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#account-information-user_data

        """
        payload: Dict[str, str] = {}
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return await self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "account",
            payload=payload,
        )

    async def my_trades(
        self,
        symbol: str,
        from_id: Optional[str] = None,
        order_id: Optional[str] = None,
        start_time: Optional[int] = None,
        end_time: Optional[int] = None,
        limit: Optional[int] = None,
        recv_window: Optional[int] = None,
    ) -> Dict[str, Any]:
        """
        Get trades for a specific account and symbol.

        Account Trade List (USER_DATA)
        `GET /api/v3/myTrades`.

        Parameters
        ----------
        symbol : str
            The symbol for the request.
        from_id : str, optional
            The trade match ID to query from.
        order_id : str, optional
            The order ID for the trades. This can only be used in combination with symbol.
        start_time : int, optional
            The start time (UNIX milliseconds) filter for the request.
        end_time : int, optional
            The end time (UNIX milliseconds) filter for the request.
        limit : int, optional
            The limit for the response.
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#account-trade-list-user_data


        """
        payload: Dict[str, str] = {"symbol": format_symbol(symbol)}
        if from_id is not None:
            payload["fromId"] = from_id
        if order_id is not None:
            payload["orderId"] = order_id
        if start_time is not None:
            payload["startTime"] = str(start_time)
        if end_time is not None:
            payload["endTime"] = str(end_time)
        if limit is not None:
            payload["limit"] = str(limit)
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return await self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "myTrades",
            payload=payload,
        )

    async def get_order_rate_limit(self, recv_window: Optional[int] = None) -> Dict[str, Any]:
        """
        Get the user's current order count usage for all intervals.

        Query Current Order Count Usage (TRADE).
        `GET /api/v3/rateLimit/order`.

        Parameters
        ----------
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#query-current-order-count-usage-trade

        """
        payload: Dict[str, str] = {}
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return await self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "rateLimit/order",
            payload=payload,
        )
