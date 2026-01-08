# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.adapters.binance.common.enums import BinanceFuturesPositionSide
from nautilus_trader.adapters.binance.common.enums import BinanceOrderSide
from nautilus_trader.adapters.binance.common.enums import BinanceOrderType
from nautilus_trader.adapters.binance.common.enums import BinanceSecurityType
from nautilus_trader.adapters.binance.common.enums import BinanceTimeInForce
from nautilus_trader.adapters.binance.common.schemas.account import BinanceOrder
from nautilus_trader.adapters.binance.common.schemas.account import BinanceStatusCode
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesMarginType
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesAccountInfo
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesAlgoOrder
from nautilus_trader.adapters.binance.futures.schemas.account import (
    BinanceFuturesAlgoOrderCancelResponse,
)
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesDualSidePosition
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesLeverage
from nautilus_trader.adapters.binance.futures.schemas.account import (
    BinanceFuturesMarginTypeResponse,
)
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesPositionRisk
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesSymbolConfig
from nautilus_trader.adapters.binance.http.account import BinanceAccountHttpAPI
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.endpoint import BinanceHttpEndpoint
from nautilus_trader.adapters.binance.http.error import BinanceClientError
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.config import PositiveInt
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class BinanceFuturesPositionModeHttp(BinanceHttpEndpoint):
    """
    Endpoint of user's position mode for every FUTURES symbol.

    `GET /fapi/v1/positionSide/dual`
    `GET /dapi/v1/positionSide/dual`

    `POST /fapi/v1/positionSide/dual`
    `POST /dapi/v1/positionSide/dual`

    References
    ----------
    https://binance-docs.github.io/apidocs/futures/en/#change-position-mode-trade
    https://binance-docs.github.io/apidocs/delivery/en/#change-position-mode-trade

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.GET: BinanceSecurityType.USER_DATA,
            HttpMethod.POST: BinanceSecurityType.TRADE,
        }
        url_path = base_endpoint + "positionSide/dual"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(BinanceFuturesDualSidePosition)
        self._post_resp_decoder = msgspec.json.Decoder(BinanceStatusCode)

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters of positionSide/dual GET request.

        Parameters
        ----------
        timestamp : str
            The millisecond timestamp of the request.
        recvWindow : str, optional
            The response receive window for the request (cannot be greater than 60000).

        """

        timestamp: str
        recvWindow: str | None = None

    class PostParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters of positionSide/dual POST request.

        Parameters
        ----------
        timestamp : str
            The millisecond timestamp of the request.
        dualSidePosition : str ('true', 'false')
            The dual side position mode to set...
            `true`: Hedge Mode, `false`: One-way mode.
        recvWindow : str, optional
            The response receive window for the request (cannot be greater than 60000).

        """

        timestamp: str
        dualSidePosition: str
        recvWindow: str | None = None

    async def get(self, params: GetParameters) -> BinanceFuturesDualSidePosition:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._get_resp_decoder.decode(raw)

    async def post(self, params: PostParameters) -> BinanceStatusCode:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._post_resp_decoder.decode(raw)


class BinanceFuturesAllOpenOrdersHttp(BinanceHttpEndpoint):
    """
    Endpoint of all open FUTURES orders.

    `DELETE /fapi/v1/allOpenOrders`
    `DELETE /dapi/v1/allOpenOrders`

    References
    ----------
    https://binance-docs.github.io/apidocs/futures/en/#cancel-all-open-orders-trade
    https://binance-docs.github.io/apidocs/delivery/en/#cancel-all-open-orders-trade

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.DELETE: BinanceSecurityType.TRADE,
        }
        url_path = base_endpoint + "allOpenOrders"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._delete_resp_decoder = msgspec.json.Decoder(BinanceStatusCode)

    class DeleteParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters of allOpenOrders DELETE request.

        Parameters
        ----------
        timestamp : str
            The millisecond timestamp of the request.
        symbol : BinanceSymbol
            The symbol of the request
        recvWindow : str, optional
            The response receive window for the request (cannot be greater than 60000).

        """

        timestamp: str
        symbol: BinanceSymbol
        recvWindow: str | None = None

    async def delete(self, params: DeleteParameters) -> BinanceStatusCode:
        method_type = HttpMethod.DELETE
        raw = await self._method(method_type, params)
        return self._delete_resp_decoder.decode(raw)


class BinanceFuturesCancelMultipleOrdersHttp(BinanceHttpEndpoint):
    """
    Endpoint of cancel multiple FUTURES orders.

    `DELETE /fapi/v1/batchOrders`
    `DELETE /dapi/v1/batchOrders`

    References
    ----------
    https://binance-docs.github.io/apidocs/futures/en/#cancel-multiple-orders-trade
    https://binance-docs.github.io/apidocs/delivery/en/#cancel-multiple-orders-trade

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.DELETE: BinanceSecurityType.TRADE,
        }
        url_path = base_endpoint + "batchOrders"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._delete_resp_decoder = msgspec.json.Decoder(
            list[BinanceOrder] | dict[str, Any],
            strict=False,
        )

    class DeleteParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters of batchOrders DELETE request.

        Parameters
        ----------
        timestamp : str
            The millisecond timestamp of the request.
        symbol : BinanceSymbol
            The symbol of the request
        recvWindow : str, optional
            The response receive window for the request (cannot be greater than 60000).

        """

        timestamp: str
        symbol: BinanceSymbol
        orderIdList: str | None = None
        origClientOrderIdList: str | None = None
        recvWindow: str | None = None

    async def delete(self, params: DeleteParameters) -> list[BinanceOrder]:
        method_type = HttpMethod.DELETE
        raw = await self._method(method_type, params)
        return self._delete_resp_decoder.decode(raw)


class BinanceFuturesAccountHttp(BinanceHttpEndpoint):
    """
    Endpoint of current FUTURES account information.

    `GET /fapi/v2/account`
    `GET /dapi/v1/account`

    References
    ----------
    https://binance-docs.github.io/apidocs/futures/en/#account-information-v2-user_data
    https://binance-docs.github.io/apidocs/delivery/en/#account-information-user_data

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
        self._resp_decoder = msgspec.json.Decoder(BinanceFuturesAccountInfo)

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

    async def get(self, params: GetParameters) -> BinanceFuturesAccountInfo:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._resp_decoder.decode(raw)


class BinanceFuturesPositionRiskHttp(BinanceHttpEndpoint):
    """
    Endpoint of information of all FUTURES positions.

    `GET /fapi/v3/positionRisk`
    `GET /dapi/v1/positionRisk`

    References
    ----------
    https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api/Position-Information-V3
    https://binance-docs.github.io/apidocs/delivery/en/#position-information-user_data

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.GET: BinanceSecurityType.USER_DATA,
        }
        url_path = base_endpoint + "positionRisk"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(list[BinanceFuturesPositionRisk])

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters of positionRisk GET request.

        Parameters
        ----------
        timestamp : str
            The millisecond timestamp of the request.
        symbol : BinanceSymbol, optional
            The symbol of the request.
        recvWindow : str, optional
            The response receive window for the request (cannot be greater than 60000).

        """

        timestamp: str
        symbol: BinanceSymbol | None = None
        recvWindow: str | None = None

    async def get(self, params: GetParameters) -> list[BinanceFuturesPositionRisk]:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._get_resp_decoder.decode(raw)


class BinanceFuturesSymbolConfigHttp(BinanceHttpEndpoint):
    """
    Endpoint for symbol configuration.

    `GET /fapi/v1/symbolConfig`

    References
    ----------
    https://developers.binance.com/docs/derivatives/usds-margined-futures/account/rest-api/Symbol-Config

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.GET: BinanceSecurityType.USER_DATA,
        }
        url_path = base_endpoint + "symbolConfig"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(list[BinanceFuturesSymbolConfig])

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters of symbolConfig GET request.

        Parameters
        ----------
        timestamp : str
            The millisecond timestamp of the request.
        symbol : BinanceSymbol, optional
            The symbol of the request.
        recvWindow : str, optional
            The response receive window for the request (cannot be greater than 60000).

        """

        timestamp: str
        symbol: BinanceSymbol | None = None
        recvWindow: str | None = None

    async def get(self, params: GetParameters) -> list[BinanceFuturesSymbolConfig]:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._get_resp_decoder.decode(raw)


class BinanceFuturesLeverageHttp(BinanceHttpEndpoint):
    """
    Initial leverage.

    `POST /fapi/v1/leverage`

    References
    ----------
    https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api/Change-Initial-Leverage

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.POST: BinanceSecurityType.TRADE,
        }
        url_path = base_endpoint + "leverage"

        super().__init__(client, methods, url_path)
        self._resp_decoder = msgspec.json.Decoder(BinanceFuturesLeverage)

    class PostParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Initial leverage POST endpoint parameters.

        Parameters
        ----------
        symbol : BinanceSymbol
        leverage : PositiveInt
            Target initial leverage: int from 1 to 125
        timestamp : str
            The millisecond timestamp of the request.
        recvWindow : str, optional
            The response receive window in milliseconds for the request.

        """

        symbol: BinanceSymbol
        leverage: PositiveInt
        timestamp: str
        recvWindow: str | None = None

    async def post(
        self,
        params: PostParameters,
    ) -> BinanceFuturesLeverage:
        method_type = HttpMethod.POST
        raw = await self._method(method_type, params)
        return self._resp_decoder.decode(raw)


class BinanceFuturesMarginTypeHttp(BinanceHttpEndpoint):
    """
    Margin type.

    `POST /fapi/v1/marginType`

    References
    ----------
    https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api/Change-Margin-Type

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.POST: BinanceSecurityType.TRADE,
        }
        url_path = base_endpoint + "marginType"
        super().__init__(client, methods, url_path)
        self._resp_decoder = msgspec.json.Decoder(BinanceFuturesMarginTypeResponse)

    class PostParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Margin type POST endpoint parameters.

        Parameters
        ----------
        symbol : BinanceSymbol
        marginType : str
            ISOLATED or CROSSED
        timestamp : str
            The millisecond timestamp of the request.
        recvWindow : str, optional
            The response receive window in milliseconds for the request.

        """

        symbol: BinanceSymbol
        marginType: str
        timestamp: str
        recvWindow: str | None = None

    async def post(
        self,
        params: PostParameters,
    ) -> BinanceFuturesMarginTypeResponse:
        try:
            raw = await self._method(HttpMethod.POST, params)
        except BinanceClientError as e:
            if e.message["msg"] == "No need to change margin type.":
                return BinanceFuturesMarginTypeResponse(code=200, msg="success")
            raise
        return self._resp_decoder.decode(raw)


class BinanceFuturesAlgoOrderHttp(BinanceHttpEndpoint):
    """
    Endpoint for managing Binance Futures algo (conditional) orders.

    `POST /fapi/v1/algoOrder` - Place an algo order
    `DELETE /fapi/v1/algoOrder` - Cancel an algo order
    `GET /fapi/v1/algoOrder` - Query an algo order

    References
    ----------
    https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api/New-Algo-Order

    Notes
    -----
    Effective 2025-12-09, Binance migrated conditional orders (STOP_MARKET,
    TAKE_PROFIT_MARKET, STOP, TAKE_PROFIT, TRAILING_STOP_MARKET) to the Algo
    Service. The traditional `/fapi/v1/order` endpoint now returns error -4120
    for these order types.

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.GET: BinanceSecurityType.USER_DATA,
            HttpMethod.POST: BinanceSecurityType.TRADE,
            HttpMethod.DELETE: BinanceSecurityType.TRADE,
        }
        url_path = base_endpoint + "algoOrder"
        super().__init__(
            client,
            methods,
            url_path,
        )

        self._post_resp_decoder = msgspec.json.Decoder(BinanceFuturesAlgoOrder)
        self._delete_resp_decoder = msgspec.json.Decoder(BinanceFuturesAlgoOrderCancelResponse)
        self._get_resp_decoder = msgspec.json.Decoder(BinanceFuturesAlgoOrder)

    class GetDeleteParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters for algo order GET & DELETE requests.

        Parameters
        ----------
        timestamp : str
            The millisecond timestamp of the request.
        algoId : int, optional
            The algo order identifier.
        clientAlgoId : str, optional
            The client-specified algo order identifier.
        recvWindow : str, optional
            The response receive window for the request (cannot be greater than 60000).

        Notes
        -----
        Either `algoId` or `clientAlgoId` must be sent.

        """

        timestamp: str
        algoId: int | None = None
        clientAlgoId: str | None = None
        recvWindow: str | None = None

    class PostParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters for algo order POST (create) request.

        Parameters
        ----------
        symbol : BinanceSymbol
            The trading pair symbol.
        side : BinanceOrderSide
            The order side (BUY or SELL).
        type : BinanceOrderType
            The order type (STOP_MARKET, TAKE_PROFIT_MARKET, STOP, TAKE_PROFIT,
            TRAILING_STOP_MARKET).
        algoType : str
            The algo type. Only "CONDITIONAL" is supported.
        timestamp : str
            The millisecond timestamp of the request.
        positionSide : BinanceFuturesPositionSide, optional
            Position side for hedge mode.
        quantity : str, optional
            Order quantity. Cannot be used with closePosition=true.
        price : str, optional
            Order price for STOP/TAKE_PROFIT orders.
        triggerPrice : str, optional
            The trigger price for conditional orders.
        timeInForce : BinanceTimeInForce, optional
            Time in force (GTC, IOC, FOK). Default is GTC.
        workingType : str, optional
            Trigger type: MARK_PRICE or CONTRACT_PRICE (default).
        priceMatch : str, optional
            Price match mode for BBO matching.
        closePosition : str, optional
            Close all position with STOP_MARKET or TAKE_PROFIT_MARKET.
        priceProtect : str, optional
            Price protection. Default is false.
        reduceOnly : str, optional
            Reduce only flag. Cannot be used in Hedge Mode.
        activationPrice : str, optional
            Activation price for TRAILING_STOP_MARKET orders.
        callbackRate : str, optional
            Callback rate for TRAILING_STOP_MARKET (0.1-10, where 1 = 1%).
        clientAlgoId : str, optional
            Client-specified algo order ID.
        goodTillDate : int, optional
            GTD expiration timestamp in milliseconds (only second-level precision retained).
        recvWindow : str, optional
            The response receive window for the request.

        """

        symbol: BinanceSymbol
        side: BinanceOrderSide
        type: BinanceOrderType
        algoType: str
        timestamp: str
        positionSide: BinanceFuturesPositionSide | None = None
        quantity: str | None = None
        price: str | None = None
        triggerPrice: str | None = None
        timeInForce: BinanceTimeInForce | None = None
        workingType: str | None = None
        priceMatch: str | None = None
        closePosition: str | None = None
        priceProtect: str | None = None
        reduceOnly: str | None = None
        activationPrice: str | None = None
        callbackRate: str | None = None
        clientAlgoId: str | None = None
        goodTillDate: int | None = None
        recvWindow: str | None = None

    async def get(self, params: GetDeleteParameters) -> BinanceFuturesAlgoOrder:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._get_resp_decoder.decode(raw)

    async def delete(self, params: GetDeleteParameters) -> BinanceFuturesAlgoOrderCancelResponse:
        method_type = HttpMethod.DELETE
        raw = await self._method(method_type, params)
        return self._delete_resp_decoder.decode(raw)

    async def post(self, params: PostParameters) -> BinanceFuturesAlgoOrder:
        method_type = HttpMethod.POST
        raw = await self._method(method_type, params)
        return self._post_resp_decoder.decode(raw)


class BinanceFuturesOpenAlgoOrdersHttp(BinanceHttpEndpoint):
    """
    Endpoint for fetching all open algo (conditional) orders.

    `GET /fapi/v1/openAlgoOrders`

    References
    ----------
    https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api/Current-All-Algo-Open-Orders

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.GET: BinanceSecurityType.USER_DATA,
        }
        url_path = base_endpoint + "openAlgoOrders"
        super().__init__(
            client,
            methods,
            url_path,
        )

        self._get_resp_decoder = msgspec.json.Decoder(list[BinanceFuturesAlgoOrder])

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters for open algo orders GET request.

        Parameters
        ----------
        timestamp : str
            The millisecond timestamp of the request.
        algoType : str, optional
            Filter by algo type.
        symbol : BinanceSymbol, optional
            Filter by symbol. If omitted, orders for all symbols returned.
        algoId : int, optional
            Filter by specific algo order ID.
        recvWindow : str, optional
            The response receive window for the request (cannot be greater than 60000).

        """

        timestamp: str
        algoType: str | None = None
        symbol: BinanceSymbol | None = None
        algoId: int | None = None
        recvWindow: str | None = None

    async def get(self, params: GetParameters) -> list[BinanceFuturesAlgoOrder]:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._get_resp_decoder.decode(raw)


class BinanceFuturesAllAlgoOrdersHttp(BinanceHttpEndpoint):
    """
    Endpoint for querying all algo (conditional) orders including historical.

    `GET /fapi/v1/allAlgoOrders`

    References
    ----------
    https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api/Query-All-Algo-Orders

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.GET: BinanceSecurityType.USER_DATA,
        }
        url_path = base_endpoint + "allAlgoOrders"
        super().__init__(
            client,
            methods,
            url_path,
        )

        self._get_resp_decoder = msgspec.json.Decoder(list[BinanceFuturesAlgoOrder])

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters for all algo orders GET request.

        Parameters
        ----------
        timestamp : str
            The millisecond timestamp of the request.
        symbol : BinanceSymbol
            The symbol to query (required).
        algoId : int, optional
            If set, retrieves orders >= that algoId; otherwise returns most recent.
        startTime : str, optional
            Query start timestamp in milliseconds.
        endTime : str, optional
            Query end timestamp in milliseconds.
        page : int, optional
            Pagination index.
        limit : int, optional
            Result limit (default 500, max 1000).
        recvWindow : str, optional
            The response receive window for the request (cannot be greater than 60000).

        """

        timestamp: str
        symbol: BinanceSymbol
        algoId: int | None = None
        startTime: str | None = None
        endTime: str | None = None
        page: int | None = None
        limit: int | None = None
        recvWindow: str | None = None

    async def get(self, params: GetParameters) -> list[BinanceFuturesAlgoOrder]:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._get_resp_decoder.decode(raw)


class BinanceFuturesCancelAllAlgoOrdersHttp(BinanceHttpEndpoint):
    """
    Endpoint for canceling all open algo (conditional) orders for a symbol.

    `DELETE /fapi/v1/algoOpenOrders`

    References
    ----------
    https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api/Cancel-All-Algo-Open-Orders

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.DELETE: BinanceSecurityType.TRADE,
        }
        url_path = base_endpoint + "algoOpenOrders"
        super().__init__(
            client,
            methods,
            url_path,
        )

        self._delete_resp_decoder = msgspec.json.Decoder(BinanceStatusCode)

    class DeleteParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Parameters for cancel all algo open orders DELETE request.

        Parameters
        ----------
        timestamp : str
            The millisecond timestamp of the request.
        symbol : BinanceSymbol
            The symbol to cancel all algo orders for.
        recvWindow : str, optional
            The response receive window for the request (cannot be greater than 60000).

        """

        timestamp: str
        symbol: BinanceSymbol
        recvWindow: str | None = None

    async def delete(self, params: DeleteParameters) -> BinanceStatusCode:
        method_type = HttpMethod.DELETE
        raw = await self._method(method_type, params)
        return self._delete_resp_decoder.decode(raw)


class BinanceFuturesAccountHttpAPI(BinanceAccountHttpAPI):
    """
    Provides access to the Binance Futures Account/Trade HTTP REST API.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance REST API client.
    account_type : BinanceAccountType
        The Binance account type.

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        clock: LiveClock,
        account_type: BinanceAccountType = BinanceAccountType.USDT_FUTURES,
    ):
        super().__init__(
            client=client,
            clock=clock,
            account_type=account_type,
        )
        if not account_type.is_futures:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"`BinanceAccountType` not USDT_FUTURES or COIN_FUTURES, was {account_type}",  # pragma: no cover
            )
        v2_endpoint_base = self.base_endpoint
        v3_endpoint_base = self.base_endpoint
        if account_type == BinanceAccountType.USDT_FUTURES:
            v2_endpoint_base = "/fapi/v2/"
            v3_endpoint_base = "/fapi/v3/"

        # Create endpoints
        self._endpoint_futures_position_mode = BinanceFuturesPositionModeHttp(
            client,
            self.base_endpoint,
        )
        self._endpoint_futures_all_open_orders = BinanceFuturesAllOpenOrdersHttp(
            client,
            self.base_endpoint,
        )
        self._endpoint_futures_cancel_multiple_orders = BinanceFuturesCancelMultipleOrdersHttp(
            client,
            self.base_endpoint,
        )
        self._endpoint_futures_account = BinanceFuturesAccountHttp(client, v2_endpoint_base)
        self._endpoint_futures_position_risk = BinanceFuturesPositionRiskHttp(
            client,
            v3_endpoint_base,
        )
        self._endpoint_futures_leverage = BinanceFuturesLeverageHttp(client, self.base_endpoint)
        self._endpoint_futures_margin_type = BinanceFuturesMarginTypeHttp(
            client,
            self.base_endpoint,
        )
        self._endpoint_futures_symbol_config = BinanceFuturesSymbolConfigHttp(
            client,
            self.base_endpoint,
        )
        self._endpoint_futures_algo_order = BinanceFuturesAlgoOrderHttp(
            client,
            self.base_endpoint,
        )
        self._endpoint_futures_open_algo_orders = BinanceFuturesOpenAlgoOrdersHttp(
            client,
            self.base_endpoint,
        )
        self._endpoint_futures_all_algo_orders = BinanceFuturesAllAlgoOrdersHttp(
            client,
            self.base_endpoint,
        )
        self._endpoint_futures_cancel_all_algo_orders = BinanceFuturesCancelAllAlgoOrdersHttp(
            client,
            self.base_endpoint,
        )

    async def query_futures_hedge_mode(
        self,
        recv_window: str | None = None,
    ) -> BinanceFuturesDualSidePosition:
        """
        Check Binance Futures hedge mode (dualSidePosition).
        """
        return await self._endpoint_futures_position_mode.get(
            params=self._endpoint_futures_position_mode.GetParameters(
                timestamp=self._timestamp(),
                recvWindow=recv_window,
            ),
        )

    async def set_leverage(
        self,
        symbol: BinanceSymbol,
        leverage: PositiveInt,
        recv_window: str | None = None,
    ) -> BinanceFuturesLeverage:
        """
        Set Binance Futures initial leverage.
        """
        return await self._endpoint_futures_leverage.post(
            self._endpoint_futures_leverage.PostParameters(
                symbol=symbol,
                leverage=leverage,
                timestamp=self._timestamp(),
                recvWindow=recv_window,
            ),
        )

    async def set_margin_type(
        self,
        symbol: BinanceSymbol,
        margin_type: BinanceFuturesMarginType,
        recv_window: str | None = None,
    ) -> BinanceFuturesMarginTypeResponse:
        """
        Change symbol level margin type.

        :param symbol : BinanceSymbol
        :param margin_type : BinanceFuturesMarginType
        :param recv_window : str, optional
        :return: BinanceFuturesMarginTypeResponse

        """
        return await self._endpoint_futures_margin_type.post(
            self._endpoint_futures_margin_type.PostParameters(
                symbol=symbol,
                marginType=margin_type.value,
                timestamp=self._timestamp(),
                recvWindow=recv_window,
            ),
        )

    async def set_futures_hedge_mode(
        self,
        dual_side_position: bool,
        recv_window: str | None = None,
    ) -> BinanceStatusCode:
        """
        Set Binance Futures hedge mode (dualSidePosition).
        """
        return await self._endpoint_futures_position_mode.post(
            params=self._endpoint_futures_position_mode.PostParameters(
                timestamp=self._timestamp(),
                dualSidePosition=str(dual_side_position).lower(),
                recvWindow=recv_window,
            ),
        )

    async def cancel_all_open_orders(
        self,
        symbol: str,
        recv_window: str | None = None,
    ) -> bool:
        """
        Delete all Futures open orders.

        Returns whether successful.

        """
        response = await self._endpoint_futures_all_open_orders.delete(
            params=self._endpoint_futures_all_open_orders.DeleteParameters(
                timestamp=self._timestamp(),
                symbol=BinanceSymbol(symbol),
                recvWindow=recv_window,
            ),
        )
        return response.code == 200

    async def cancel_multiple_orders(
        self,
        symbol: str,
        client_order_ids: list[str],
        recv_window: str | None = None,
    ) -> bool:
        """
        Delete multiple Futures orders.

        Returns whether successful.

        """
        stringified_client_order_ids = str(client_order_ids).replace(" ", "").replace("'", '"')
        await self._endpoint_futures_cancel_multiple_orders.delete(
            params=self._endpoint_futures_cancel_multiple_orders.DeleteParameters(
                timestamp=self._timestamp(),
                symbol=BinanceSymbol(symbol),
                origClientOrderIdList=stringified_client_order_ids,
                recvWindow=recv_window,
            ),
        )
        return True

    async def query_futures_account_info(
        self,
        recv_window: str | None = None,
    ) -> BinanceFuturesAccountInfo:
        """
        Check Binance Futures account information.
        """
        return await self._endpoint_futures_account.get(
            params=self._endpoint_futures_account.GetParameters(
                timestamp=self._timestamp(),
                recvWindow=recv_window,
            ),
        )

    async def query_futures_position_risk(
        self,
        symbol: str | None = None,
        recv_window: str | None = None,
    ) -> list[BinanceFuturesPositionRisk]:
        """
        Check all Futures position's info for a symbol.
        """
        return await self._endpoint_futures_position_risk.get(
            params=self._endpoint_futures_position_risk.GetParameters(
                timestamp=self._timestamp(),
                symbol=BinanceSymbol(symbol) if symbol else None,
                recvWindow=recv_window,
            ),
        )

    async def query_futures_symbol_config(
        self,
        symbol: str | None = None,
        recv_window: str | None = None,
    ) -> list[BinanceFuturesSymbolConfig]:
        """
        Check Futures symbol configuration including leverage settings.
        """
        return await self._endpoint_futures_symbol_config.get(
            params=self._endpoint_futures_symbol_config.GetParameters(
                timestamp=self._timestamp(),
                symbol=BinanceSymbol(symbol) if symbol else None,
                recvWindow=recv_window,
            ),
        )

    async def new_algo_order(
        self,
        symbol: str,
        side: BinanceOrderSide,
        order_type: BinanceOrderType,
        position_side: BinanceFuturesPositionSide | None = None,
        quantity: str | None = None,
        price: str | None = None,
        trigger_price: str | None = None,
        time_in_force: BinanceTimeInForce | None = None,
        working_type: str | None = None,
        price_match: str | None = None,
        close_position: str | None = None,
        price_protect: str | None = None,
        reduce_only: str | None = None,
        activation_price: str | None = None,
        callback_rate: str | None = None,
        client_algo_id: str | None = None,
        good_till_date: int | None = None,
        recv_window: str | None = None,
    ) -> BinanceFuturesAlgoOrder:
        """
        Send a new conditional (algo) order to Binance Futures.

        This endpoint is required for STOP_MARKET, TAKE_PROFIT_MARKET, STOP,
        TAKE_PROFIT, and TRAILING_STOP_MARKET orders as of 2025-12-09.

        """
        return await self._endpoint_futures_algo_order.post(
            params=self._endpoint_futures_algo_order.PostParameters(
                symbol=BinanceSymbol(symbol),
                side=side,
                type=order_type,
                algoType="CONDITIONAL",
                timestamp=self._timestamp(),
                positionSide=position_side,
                quantity=quantity,
                price=price,
                triggerPrice=trigger_price,
                timeInForce=time_in_force,
                workingType=working_type,
                priceMatch=price_match,
                closePosition=close_position,
                priceProtect=price_protect,
                reduceOnly=reduce_only,
                activationPrice=activation_price,
                callbackRate=callback_rate,
                clientAlgoId=client_algo_id,
                goodTillDate=good_till_date,
                recvWindow=recv_window,
            ),
        )

    async def cancel_algo_order(
        self,
        algo_id: int | None = None,
        client_algo_id: str | None = None,
        recv_window: str | None = None,
    ) -> BinanceFuturesAlgoOrderCancelResponse:
        """
        Cancel an active algo order.
        """
        if algo_id is None and client_algo_id is None:
            raise RuntimeError(
                "Either algoId or clientAlgoId must be sent.",
            )
        return await self._endpoint_futures_algo_order.delete(
            params=self._endpoint_futures_algo_order.GetDeleteParameters(
                timestamp=self._timestamp(),
                algoId=algo_id,
                clientAlgoId=client_algo_id,
                recvWindow=recv_window,
            ),
        )

    async def query_algo_order(
        self,
        algo_id: int | None = None,
        client_algo_id: str | None = None,
        recv_window: str | None = None,
    ) -> BinanceFuturesAlgoOrder:
        """
        Query an algo order status.
        """
        if algo_id is None and client_algo_id is None:
            raise RuntimeError(
                "Either algoId or clientAlgoId must be sent.",
            )
        return await self._endpoint_futures_algo_order.get(
            params=self._endpoint_futures_algo_order.GetDeleteParameters(
                timestamp=self._timestamp(),
                algoId=algo_id,
                clientAlgoId=client_algo_id,
                recvWindow=recv_window,
            ),
        )

    async def query_open_algo_orders(
        self,
        symbol: str | None = None,
        recv_window: str | None = None,
    ) -> list[BinanceFuturesAlgoOrder]:
        """
        Query all currently open algo orders.

        Parameters
        ----------
        symbol : str, optional
            Filter by symbol. If omitted, orders for all symbols returned.
        recv_window : str, optional
            The response receive window for the request.

        Returns
        -------
        list[BinanceFuturesAlgoOrder]

        """
        return await self._endpoint_futures_open_algo_orders.get(
            params=self._endpoint_futures_open_algo_orders.GetParameters(
                timestamp=self._timestamp(),
                symbol=BinanceSymbol(symbol) if symbol else None,
                recvWindow=recv_window,
            ),
        )

    async def query_all_algo_orders(
        self,
        symbol: str,
        start_time: int | None = None,
        end_time: int | None = None,
        page: int | None = None,
        limit: int | None = None,
        recv_window: str | None = None,
    ) -> list[BinanceFuturesAlgoOrder]:
        """
        Query all algo orders including historical (triggered, cancelled, finished).

        Parameters
        ----------
        symbol : str
            The symbol to query (required).
        start_time : int, optional
            Query start timestamp in milliseconds.
        end_time : int, optional
            Query end timestamp in milliseconds.
        page : int, optional
            Pagination index (1-based).
        limit : int, optional
            Result limit (default 500, max 1000).
        recv_window : str, optional
            The response receive window for the request.

        Returns
        -------
        list[BinanceFuturesAlgoOrder]

        """
        return await self._endpoint_futures_all_algo_orders.get(
            params=self._endpoint_futures_all_algo_orders.GetParameters(
                timestamp=self._timestamp(),
                symbol=BinanceSymbol(symbol),
                startTime=str(start_time) if start_time else None,
                endTime=str(end_time) if end_time else None,
                page=page,
                limit=limit,
                recvWindow=recv_window,
            ),
        )

    async def cancel_all_open_algo_orders(
        self,
        symbol: str,
        recv_window: str | None = None,
    ) -> bool:
        """
        Cancel all open algo orders for a specific symbol.

        Parameters
        ----------
        symbol : str
            The symbol to cancel all algo orders for.
        recv_window : str, optional
            The response receive window for the request.

        Returns
        -------
        bool
            True if successful.

        """
        response = await self._endpoint_futures_cancel_all_algo_orders.delete(
            params=self._endpoint_futures_cancel_all_algo_orders.DeleteParameters(
                timestamp=self._timestamp(),
                symbol=BinanceSymbol(symbol),
                recvWindow=recv_window,
            ),
        )
        return response.code == 200
