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
from nautilus_trader.adapters.binance.common.schemas.user import BinanceListenKey
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.endpoint import BinanceHttpEndpoint
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class BinancePortfolioMarginListenKeyEndpoint(BinanceHttpEndpoint):
    """
    Endpoint for managing Portfolio Margin user data stream listen keys.
    """

    def __init__(self, client: BinanceHttpClient):
        methods = {
            HttpMethod.POST: BinanceSecurityType.USER_STREAM,
            HttpMethod.PUT: BinanceSecurityType.USER_STREAM,
            HttpMethod.DELETE: BinanceSecurityType.USER_STREAM,
        }
        url_path = "/papi/v1/listenKey"
        super().__init__(client, methods, url_path)
        self._resp_decoder = msgspec.json.Decoder(BinanceListenKey)

    class GetParameters(msgspec.Struct):
        """
        GET /papi/v1/listenKey parameters.
        """
        pass

    async def post(self, parameters: GetParameters) -> BinanceListenKey:
        method_type = HttpMethod.POST
        raw: bytes = await self._method(method_type, parameters)
        return self._resp_decoder.decode(raw)

    async def put(self, parameters: GetParameters) -> None:
        method_type = HttpMethod.PUT
        await self._method(method_type, parameters)

    async def delete(self, parameters: GetParameters) -> None:
        method_type = HttpMethod.DELETE
        await self._method(method_type, parameters)


class BinancePortfolioMarginUserDataHttpAPI:
    """
    Provides access to the Binance Portfolio Margin HTTP user data endpoints.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance HTTP client.
    account_type : BinanceAccountType
        The account type for the client.

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        account_type: BinanceAccountType,
    ) -> None:
        self.client = client
        self._account_type = account_type

        if account_type != BinanceAccountType.PORTFOLIO_MARGIN:
            raise ValueError(f"Invalid account_type: {account_type}")

        # Initialize endpoints
        self._listen_key = BinancePortfolioMarginListenKeyEndpoint(client)

    async def create_listen_key(self) -> BinanceListenKey:
        """
        Create a new Portfolio Margin user data stream listen key.

        Returns
        -------
        BinanceListenKey
            The listen key response.
        """
        response = await self._listen_key.post(
            BinancePortfolioMarginListenKeyEndpoint.GetParameters(),
        )
        return response

    async def keepalive_listen_key(self) -> None:
        """
        Keep alive the Portfolio Margin user data stream listen key.
        """
        await self._listen_key.put(
            BinancePortfolioMarginListenKeyEndpoint.GetParameters(),
        )

    async def delete_listen_key(self) -> None:
        """
        Delete the Portfolio Margin user data stream listen key.
        """
        await self._listen_key.delete(
            BinancePortfolioMarginListenKeyEndpoint.GetParameters(),
        )
