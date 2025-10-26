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
from nautilus_trader.adapters.binance.common.schemas.account import BinanceStatusCode
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesMarginType
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesAccountInfo
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesDualSidePosition
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesLeverage
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesMarginTypeResponse  # fmt: skip
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
