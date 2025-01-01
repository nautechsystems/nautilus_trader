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

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceSecurityType
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbols
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.endpoint import BinanceHttpEndpoint
from nautilus_trader.adapters.binance.http.market import BinanceMarketHttpAPI
from nautilus_trader.adapters.binance.spot.enums import BinanceSpotPermissions
from nautilus_trader.adapters.binance.spot.schemas.market import BinanceSpotAvgPrice
from nautilus_trader.adapters.binance.spot.schemas.market import BinanceSpotExchangeInfo
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class BinanceSpotExchangeInfoHttp(BinanceHttpEndpoint):
    """
    Endpoint of SPOT/MARGIN exchange trading rules and symbol information.

    `GET /api/v3/exchangeInfo`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#exchange-information

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "exchangeInfo"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(BinanceSpotExchangeInfo)

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET exchangeInfo parameters.

        Parameters
        ----------
        symbol : BinanceSymbol, optional
            The specify trading pair to get exchange info for.
        symbols : BinanceSymbols, optional
            The specify list of trading pairs to get exchange info for.
        permissions : BinanceSpotPermissions, optional
            The filter symbols list by supported permissions.

        """

        symbol: BinanceSymbol | None = None
        symbols: BinanceSymbols | None = None
        permissions: BinanceSpotPermissions | None = None

    async def get(self, params: GetParameters | None = None) -> BinanceSpotExchangeInfo:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._get_resp_decoder.decode(raw)


class BinanceSpotAvgPriceHttp(BinanceHttpEndpoint):
    """
    Endpoint of current average price of a symbol.

    `GET /api/v3/avgPrice`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#current-average-price

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "avgPrice"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(BinanceSpotAvgPrice)

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET avgPrice parameters.

        Parameters
        ----------
        symbol : BinanceSymbol
            Specify trading pair to get average price for.

        """

        symbol: BinanceSymbol = None

    async def get(self, params: GetParameters) -> BinanceSpotAvgPrice:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._get_resp_decoder.decode(raw)


class BinanceSpotMarketHttpAPI(BinanceMarketHttpAPI):
    """
    Provides access to the Binance Spot Market HTTP REST API.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance REST API client.
    account_type : BinanceAccountType
        The Binance account type, used to select the endpoint.

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        account_type: BinanceAccountType = BinanceAccountType.SPOT,
    ):
        super().__init__(
            client=client,
            account_type=account_type,
        )

        if not account_type.is_spot_or_margin:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"`BinanceAccountType` not SPOT, MARGIN or ISOLATED_MARGIN, was {account_type}",  # pragma: no cover
            )

        self._endpoint_spot_exchange_info = BinanceSpotExchangeInfoHttp(client, self.base_endpoint)
        self._endpoint_spot_average_price = BinanceSpotAvgPriceHttp(client, self.base_endpoint)

    async def query_spot_exchange_info(
        self,
        symbol: str | None = None,
        symbols: list[str] | None = None,
        permissions: BinanceSpotPermissions | None = None,
    ) -> BinanceSpotExchangeInfo:
        """
        Check Binance Spot exchange information.
        """
        if symbol and symbols:
            raise ValueError("`symbol` and `symbols` cannot be sent together")
        return await self._endpoint_spot_exchange_info.get(
            params=self._endpoint_spot_exchange_info.GetParameters(
                symbol=BinanceSymbol(symbol) if symbol else None,
                symbols=BinanceSymbols(symbols) if symbols else None,
                permissions=permissions,
            ),
        )

    async def query_spot_average_price(self, symbol: str) -> BinanceSpotAvgPrice:
        """
        Check average price for a provided symbol on the Spot exchange.
        """
        return await self._endpoint_spot_average_price.get(
            params=self._endpoint_spot_average_price.GetParameters(
                symbol=BinanceSymbol(symbol),
            ),
        )
