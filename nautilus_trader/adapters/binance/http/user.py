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
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.endpoint import BinanceHttpEndpoint
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class BinanceListenKeyHttp(BinanceHttpEndpoint):
    """
    Endpoint for managing user data streams (listenKey).

    `POST /api/v3/userDataStream`
    `POST /sapi/v3/userDataStream`
    `POST /sapi/v3/userDataStream/isolated`
    `POST /fapi/v1/listenKey`
    `POST /dapi/v1/listenKey`

    `PUT /api/v3/userDataStream`
    `PUT /sapi/v3/userDataStream`
    `PUT /sapi/v3/userDataStream/isolated`
    `PUT /fapi/v1/listenKey`
    `PUT /dapi/v1/listenKey`

    `DELETE /api/v3/userDataStream`
    `DELETE /sapi/v3/userDataStream`
    `DELETE /sapi/v3/userDataStream/isolated`
    `DELETE /fapi/v1/listenKey`
    `DELETE /dapi/v1/listenKey`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#listen-key-spot
    https://binance-docs.github.io/apidocs/spot/en/#listen-key-margin
    https://binance-docs.github.io/apidocs/futures/en/#start-user-data-stream-user_stream
    https://binance-docs.github.io/apidocs/delivery/en/#start-user-data-stream-user_stream

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
        symbol : BinanceSymbol
            The trading pair. Only required for ISOLATED MARGIN accounts!

        """

        symbol: BinanceSymbol | None = None  # MARGIN_ISOLATED only, mandatory

    class PutDeleteParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        PUT & DELETE parameters for managing listen keys.

        Parameters
        ----------
        symbol : BinanceSymbol
            The trading pair. Only required for ISOLATED MARGIN accounts!
        listenKey : str
            The listen key to manage. Only required for SPOT/MARGIN accounts!

        """

        symbol: BinanceSymbol | None = None  # MARGIN_ISOLATED only, mandatory
        listenKey: str | None = None  # SPOT/MARGIN only, mandatory

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
    Provides access to the Binance User HTTP REST API.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance REST API client.
    account_type : BinanceAccountType
        The Binance account type, used to select the endpoint.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.

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
            self.base_endpoint = "/api/v3/"
            listen_key_url = self.base_endpoint + "userDataStream"
        elif account_type == BinanceAccountType.MARGIN:
            self.base_endpoint = "/sapi/v1/"
            listen_key_url = self.base_endpoint + "userDataStream"
        elif account_type == BinanceAccountType.ISOLATED_MARGIN:
            self.base_endpoint = "/sapi/v1/"
            listen_key_url = self.base_endpoint + "userDataStream/isolated"
        elif account_type == BinanceAccountType.USDT_FUTURE:
            self.base_endpoint = "/fapi/v1/"
            listen_key_url = self.base_endpoint + "listenKey"
        elif account_type == BinanceAccountType.COIN_FUTURE:
            self.base_endpoint = "/dapi/v1/"
            listen_key_url = self.base_endpoint + "listenKey"
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"invalid `BinanceAccountType`, was {account_type}",  # pragma: no cover (design-time error)
            )

        self._endpoint_listenkey = BinanceListenKeyHttp(client, listen_key_url)

    async def create_listen_key(
        self,
        symbol: str | None = None,
    ) -> BinanceListenKey:
        """
        Create Binance ListenKey.
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
        Ping/Keepalive Binance ListenKey.
        """
        await self._endpoint_listenkey._put(
            params=self._endpoint_listenkey.PutDeleteParameters(
                symbol=BinanceSymbol(symbol) if symbol else None,
                listenKey=listen_key,
            ),
        )

    async def delete_listen_key(
        self,
        symbol: str | None = None,
        listen_key: str | None = None,
    ):
        """
        Delete Binance ListenKey.
        """
        await self._endpoint_listenkey._delete(
            params=self._endpoint_listenkey.PutDeleteParameters(
                symbol=BinanceSymbol(symbol) if symbol else None,
                listenKey=listen_key,
            ),
        )
