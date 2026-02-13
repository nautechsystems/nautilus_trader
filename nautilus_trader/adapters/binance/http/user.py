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

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceSecurityType
from nautilus_trader.adapters.binance.common.schemas.user import BinanceListenKey
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.endpoint import BinanceHttpEndpoint
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class BinanceListenKeyHttp(BinanceHttpEndpoint):
    """
    Endpoint for managing user data streams (listenKey).

    `POST /fapi/v1/listenKey`
    `PUT /fapi/v1/listenKey`
    `DELETE /fapi/v1/listenKey`

    References
    ----------
    https://developers.binance.com/docs/derivatives/usds-margined-futures/user-data-streams

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        url_path: str,
    ):
        methods = {
            HttpMethod.POST: BinanceSecurityType.USER_STREAM,
            HttpMethod.PUT: BinanceSecurityType.USER_STREAM,
            HttpMethod.DELETE: BinanceSecurityType.USER_STREAM,
        }
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._post_resp_decoder = msgspec.json.Decoder(BinanceListenKey)
        self._put_resp_decoder = msgspec.json.Decoder()
        self._delete_resp_decoder = msgspec.json.Decoder()

    class PostParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        POST parameters for creating listen keys.

        Parameters
        ----------
        symbol : BinanceSymbol, optional
            The trading pair. Only required for ISOLATED MARGIN accounts.

        """

        symbol: BinanceSymbol | None = None

    class PutDeleteParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        PUT & DELETE parameters for managing listen keys.

        Parameters
        ----------
        symbol : BinanceSymbol, optional
            The trading pair. Only required for ISOLATED MARGIN accounts.
        listenKey : str, optional
            The listen key to manage. Only required for SPOT/MARGIN accounts.

        """

        symbol: BinanceSymbol | None = None
        listenKey: str | None = None

    async def _post(self, params: PostParameters | None = None) -> BinanceListenKey:
        method_type = HttpMethod.POST
        raw = await self._method(method_type, params)
        return self._post_resp_decoder.decode(raw)

    async def _put(self, params: PutDeleteParameters | None = None) -> dict:
        method_type = HttpMethod.PUT
        raw = await self._method(method_type, params)
        return self._put_resp_decoder.decode(raw)

    async def _delete(self, params: PutDeleteParameters | None = None) -> dict:
        method_type = HttpMethod.DELETE
        raw = await self._method(method_type, params)
        return self._delete_resp_decoder.decode(raw)


class BinanceUserDataHttpAPI:
    """
    Provides access to the Binance User Data Stream HTTP REST API.

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
        account_type: BinanceAccountType,
    ):
        PyCondition.not_none(client, "client")
        self.client = client
        self.account_type = account_type

        if account_type == BinanceAccountType.SPOT:
            listen_key_url = "/api/v3/userDataStream"
        elif account_type == BinanceAccountType.MARGIN:
            listen_key_url = "/sapi/v1/userDataStream"
        elif account_type == BinanceAccountType.ISOLATED_MARGIN:
            listen_key_url = "/sapi/v1/userDataStream/isolated"
        elif account_type == BinanceAccountType.USDT_FUTURES:
            listen_key_url = "/fapi/v1/listenKey"
        elif account_type == BinanceAccountType.COIN_FUTURES:
            listen_key_url = "/dapi/v1/listenKey"
        else:
            raise RuntimeError(
                f"invalid `BinanceAccountType`, was {account_type}",
            )

        self._endpoint_listenkey = BinanceListenKeyHttp(client, listen_key_url)

    async def create_listen_key(
        self,
        symbol: str | None = None,
    ) -> BinanceListenKey:
        """
        Create a new Binance listenKey.
        """
        key = await self._endpoint_listenkey._post(
            params=self._endpoint_listenkey.PostParameters(
                symbol=BinanceSymbol(symbol) if symbol else None,
            ),
        )
        return key

    async def keepalive_listen_key(
        self,
        symbol: str | None = None,
        listen_key: str | None = None,
    ):
        """
        Keepalive an existing Binance listenKey.
        """
        await self._endpoint_listenkey._put(
            params=self._endpoint_listenkey.PutDeleteParameters(
                symbol=BinanceSymbol(symbol) if symbol else None,
                listenKey=listen_key,
            ),
        )

    async def close_listen_key(
        self,
        symbol: str | None = None,
        listen_key: str | None = None,
    ):
        """
        Close an existing Binance listenKey.
        """
        await self._endpoint_listenkey._delete(
            params=self._endpoint_listenkey.PutDeleteParameters(
                symbol=BinanceSymbol(symbol) if symbol else None,
                listenKey=listen_key,
            ),
        )
