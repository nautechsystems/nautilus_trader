# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Dict, Optional

from nautilus_trader.adapters.binance.common import format_symbol
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.enums import NewOrderRespType
from nautilus_trader.core.correctness import PyCondition


class BinanceSpotAccountHttpAPI:
    """
    Provides access to the `Binance SPOT Account/Trade` HTTP REST API.
    """

    BASE_ENDPOINT = "/api/v3/"

    def __init__(self, client: BinanceHttpClient):
        """
        Initialize a new instance of the ``BinanceSpotAccountHttpAPI`` class.

        Parameters
        ----------
        client : BinanceHttpClient
            The Binance REST API client.

        """
        PyCondition.not_none(client, "client")

        self.client = client

    def new_order_test(
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
    ):
        """
        Test New Order (TRADE).

        Test new order creation and signature/recvWindow. Creates and validates
        a new order but does not send it into the matching engine.

        `POST /api/v3/order/test`.

        Args:
            symbol (str)
            side (str)
            type (str)
            timeInForce (str, optional)
            quantity (float, optional)
            quoteOrderQty (float, optional)
            price (float, optional)
            newClientOrderId (str, optional): A unique id among open orders. Automatically generated if not sent.
            stopPrice (float, optional): Used with STOP_LOSS, STOP_LOSS_LIMIT, TAKE_PROFIT, and TAKE_PROFIT_LIMIT orders.
            icebergQty (float, optional): Used with LIMIT, STOP_LOSS_LIMIT, and TAKE_PROFIT_LIMIT to create an iceberg order.
            newOrderRespType (str, optional): Set the response JSON. ACK, RESULT, or FULL;
                    MARKET and LIMIT order types default to FULL, all other orders default to ACK.
            recvWindow (int, optional): The value cannot be greater than 60000

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#test-new-order-trade

        """
        payload: Dict[str, str] = {
            "symbol": format_symbol(symbol).upper(),
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

        return self.client.sign_request(
            http_method="POST",
            url_path=self.BASE_ENDPOINT + "order/test",
            payload=payload,
        )

    def new_order(
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
    ):
        """
        Submit New Order (TRADE).

        Post a new order.

        `POST /api/v3/order`.

        Args:
            symbol (str)
            side (str)
            type (str)
            timeInForce (str, optional)
            quantity (float, optional)
            quoteOrderQty (float, optional)
            price (float, optional)
            newClientOrderId (str, optional): A unique id among open orders. Automatically generated if not sent.
            stopPrice (float, optional): Used with STOP_LOSS, STOP_LOSS_LIMIT, TAKE_PROFIT, and TAKE_PROFIT_LIMIT orders.
            icebergQty (float, optional): Used with LIMIT, STOP_LOSS_LIMIT, and TAKE_PROFIT_LIMIT to create an iceberg order.
            newOrderRespType (str, optional): Set the response JSON. ACK, RESULT, or FULL;
                    MARKET and LIMIT order types default to FULL, all other orders default to ACK.
            recvWindow (int, optional): The value cannot be greater than 60000

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#new-order-trade

        """
        payload: Dict[str, str] = {
            "symbol": format_symbol(symbol).upper(),
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

        return self.client.sign_request(
            http_method="POST",
            url_path=self.BASE_ENDPOINT + "order",
            payload=payload,
        )

    def cancel_order(
        self,
        symbol: str,
        order_id: Optional[str] = None,
        orig_client_order_id: Optional[str] = None,
        new_client_order_id: Optional[str] = None,
        recv_window: Optional[int] = None,
    ):
        """
        Cancel Order (TRADE).

        Cancel an active order.

        `DELETE /api/v3/order`.

        Args:
            symbol (str)
            orderId (int, optional)
            origClientOrderId (str, optional)
            newClientOrderId (str, optional)
            recvWindow (int, optional): The value cannot be greater than 60000

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#cancel-order-trade

        """
        payload: Dict[str, str] = {"symbol": format_symbol(symbol).upper()}
        if order_id is not None:
            payload["orderId"] = str(order_id)
        if orig_client_order_id is not None:
            payload["origClientOrderId"] = str(orig_client_order_id)
        if new_client_order_id is not None:
            payload["newClientOrderId"] = str(new_client_order_id)
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return self.client.sign_request(
            http_method="DELETE",
            url_path=self.BASE_ENDPOINT + "order",
            payload=payload,
        )

    def cancel_open_orders(
        self,
        symbol: str,
        recv_window: Optional[int] = None,
    ):
        """
        Cancel all Open Orders on a Symbol (TRADE).

        Cancels all active orders on a symbol. This includes OCO orders.

        `DELETE api/v3/openOrders`.

        Args:
            symbol (str)
            recvWindow (int, optional): The value cannot be greater than 60000

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#cancel-all-open-orders-on-a-symbol-trade

        """
        payload: Dict[str, str] = {"symbol": format_symbol(symbol).upper()}
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return self.client.sign_request(
            http_method="DELETE",
            url_path=self.BASE_ENDPOINT + "openOrders",
            payload=payload,
        )

    def get_order(
        self,
        symbol: str,
        order_id: Optional[str] = None,
        orig_client_order_id: Optional[str] = None,
        recv_window: Optional[int] = None,
    ):
        """
        Query Order (USER_DATA).

        Check an order's status.

        `GET /api/v3/order`.

        Args:
            symbol (str)
            orderId (int, optional)
            origClientOrderId (str, optional)
            recvWindow (int, optional): The value cannot be greater than 60000

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#query-order-user_data

        """
        payload: Dict[str, str] = {"symbol": format_symbol(symbol).upper()}
        if order_id is not None:
            payload["orderId"] = order_id
        if orig_client_order_id is not None:
            payload["origClientOrderId"] = orig_client_order_id
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "order",
            payload=payload,
        )

    def get_open_orders(
        self,
        symbol: Optional[str] = None,
        recv_window: Optional[int] = None,
    ):
        """
        Query Current Open Orders (USER_DATA).

        Get all open orders on a symbol.

        `GET /api/v3/openOrders`.

        Args:
            symbol (str, optional)
            recvWindow (int, optional): The value cannot be greater than 60000

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#current-open-orders-user_data

        """
        payload: Dict[str, str] = {}
        if symbol is not None:
            payload["symbol"] = format_symbol(symbol).upper()
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "openOrders",
            payload=payload,
        )

    def get_orders(
        self,
        symbol: str,
        order_id: Optional[str] = None,
        start_time: Optional[int] = None,
        end_time: Optional[int] = None,
        limit: Optional[int] = None,
        recv_window: Optional[int] = None,
    ):
        """
        All Orders (USER_DATA).

        Get all account orders; active, canceled, or filled.

        `GET /api/v3/allOrders`.

        Args:
            symbol (str)
            orderId (int, optional)
            startTime (int, optional)
            endTime (int, optional)
            limit (int, optional): Default 500; max 1000.
            recvWindow (int, optional): The value cannot be greater than 60000

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#all-orders-user_data

        """
        payload: Dict[str, str] = {"symbol": format_symbol(symbol).upper()}
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

        return self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "allOrders",
            payload=payload,
        )

    def new_oco_order(
        self,
        symbol: str,
        side: str,
        quantity: str,
        price: str,
        stop_price: str,
        recv_window: Optional[int] = None,
    ):
        """
        Submit New OCO (TRADE).

        Post a new oco order.

        `POST /api/v3/order/oco`.

        Args:
            symbol (str)
            side (str)
            quantity (float)
            price (float)
            stopPrice (float)
        Keyword Args:
            listClientOrderId (str, optional): A unique Id for the entire orderList
            limitClientOrderId (str, optional)
            limitIcebergQty (float, optional)
            stopClientOrderId (str, optional)
            stopLimitPrice (float, optional)
            stopIcebergQty (float, optional)
            stopLimitTimeInForce (str, optional)
            newOrderRespType (str, optional): Set the response JSON.
            recvWindow (int, optional): The value cannot be greater than 60000

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#new-oco-trade

        """
        payload: Dict[str, str] = {
            "symbol": format_symbol(symbol).upper(),
            "side": side,
            "quantity": quantity,
            "price": price,
            "stopPrice": stop_price,
        }
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return self.client.sign_request(
            http_method="POST",
            url_path=self.BASE_ENDPOINT + "order/oco",
            payload=payload,
        )

    def cancel_oco_order(
        self,
        symbol: str,
        recv_window: Optional[int] = None,
    ):
        """
        Cancel OCO (TRADE).

        Cancel an entire Order List.

        `DELETE /api/v3/orderList`.

        Args:
            symbol (str)
            orderListId (int, optional): Either orderListId or listClientOrderId must be provided
            listClientOrderId (str, optional): Either orderListId or listClientOrderId must be provided
            newClientOrderId (str, optional): Used to uniquely identify this cancel. Automatically generated by default.
            recvWindow (int, optional): The value cannot be greater than 60000

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#cancel-oco-trade

        """
        payload: Dict[str, str] = {"symbol": format_symbol(symbol).upper()}
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return self.client.sign_request(
            http_method="DELETE",
            url_path=self.BASE_ENDPOINT + "orderList",
            payload=payload,
        )

    def get_oco_order(
        self,
        recv_window: Optional[int] = None,
    ):
        """
        Query OCO (USER_DATA).

        Retrieves a specific OCO based on provided optional parameters.

        `GET /api/v3/orderList`.

        Keyword Args:
            orderListId (int, optional): Either orderListId or listClientOrderId must be provided
            origClientOrderId (str, optional): Either orderListId or listClientOrderId must be provided.
            recvWindow (int, optional): The value cannot be greater than 60000

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#query-oco-user_data

        """
        payload: Dict[str, str] = {}
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "orderList",
            payload=payload,
        )

    def get_oco_orders(
        self,
        recv_window: Optional[int] = None,
    ):
        """
        Query all OCO (USER_DATA).

        Retrieves all OCO based on provided optional parameters.

        `GET /api/v3/allOrderList`.

        Keyword Args:
            fromId (int, optional): If supplied, neither startTime or endTime can be provided
            startTime (int, optional)
            endTime (int, optional)
            limit (int, optional): Default Value: 500; Max Value: 1000
            recvWindow (int, optional): The value cannot be greater than 60000

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#query-all-oco-user_data

        """
        payload: Dict[str, str] = {}
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "allOrderList",
            payload=payload,
        )

    def get_oco_open_orders(
        self,
        recv_window: Optional[int] = None,
    ):
        """
        Query Open OCO (USER_DATA).

        GET /api/v3/openOrderList.

        Keyword Args:
            recvWindow (int, optional): The value cannot be greater than 60000

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#query-open-oco-user_data

        """
        payload: Dict[str, str] = {}
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "openOrderList",
            payload=payload,
        )

    def account(
        self,
        recv_window: Optional[int] = None,
    ):
        """
        Account Information (USER_DATA).

        Get current account information.

        `GET /api/v3/account`.

        Keyword Args:
            recvWindow (int, optional): The value cannot be greater than 60000

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#account-information-user_data

        """
        payload: Dict[str, str] = {}
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "account",
            payload=payload,
        )

    def my_trades(
        self,
        symbol: str,
        recv_window: Optional[int] = None,
    ):
        """
        Account Trade List (USER_DATA)

        Get trades for a specific account and symbol.

        `GET /api/v3/myTrades`.

        Args:
            symbol (str)
            fromId (int, optional): TradeId to fetch from. Default gets most recent trades.
            orderId (int, optional): This can only be used in combination with symbol
            startTime (int, optional)
            endTime (int, optional)
            limit (int, optional): Default Value: 500; Max Value: 1000
            recvWindow (int, optional): The value cannot be greater than 60000

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#account-trade-list-user_data


        """
        payload: Dict[str, str] = {"symbol": format_symbol(symbol).upper()}
        if recv_window is not None:
            payload["recvWindow"] = str(recv_window)

        return self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "myTrades",
            payload=payload,
        )

    async def get_order_rate_limit(self, recv_window: Optional[int] = None):
        """
        Query Current Order Count Usage (TRADE).

        Displays the user's current order count usage for all intervals.

        `GET /api/v3/rateLimit/order`.

        Parameters
        ----------
        recv_window : int, optional
            The response receive window for the request (cannot be greater than 60000).

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
