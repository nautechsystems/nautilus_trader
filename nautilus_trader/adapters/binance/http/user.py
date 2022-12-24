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
# -------------------------------------------------------------------------------------------------

from typing import Any

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.schemas import BinanceListenKey
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.core.correctness import PyCondition


class BinanceUserDataHttpAPI:
    """
    Provides access to the `Binance` User HTTP REST API.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance REST API client.
    account_type : BinanceAccountType
        The Binance account type, used to select the endpoint

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
            self.listen_key_endpoint = self.base_endpoint + "userDataStream"
        elif account_type == BinanceAccountType.MARGIN:
            self.base_endpoint = "/sapi/v1/"
            self.listen_key_endpoint = self.base_endpoint + "userDataStream"
        elif account_type == BinanceAccountType.FUTURES_USDT:
            self.base_endpoint = "/fapi/v1/"
            self.listen_key_endpoint = self.base_endpoint + "listenKey"
        elif account_type == BinanceAccountType.FUTURES_COIN:
            self.base_endpoint = "/dapi/v1/"
            self.listen_key_endpoint = self.base_endpoint + "listenKey"
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"invalid `BinanceAccountType`, was {account_type}",  # pragma: no cover (design-time error)  # noqa
            )

    async def create_listen_key(self) -> BinanceListenKey:
        """
        Create a new listen key for the Binance account type.

        Start a new user data stream. The stream will close after 60 minutes
        unless a keepalive is sent. If the account has an active listenKey,
        that listenKey will be returned and its validity will be extended for 60
        minutes.

        Create a ListenKey (USER_STREAM).

        Returns
        -------
        BinanceListenKey

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#listen-key-spot
        https://binance-docs.github.io/apidocs/spot/en/#listen-key-margin
        https://binance-docs.github.io/apidocs/futures/en/#start-user-data-stream-user_stream
        https://binance-docs.github.io/apidocs/delivery/en/#start-user-data-stream-user_stream

        """
        raw: bytes = await self.client.send_request(
            http_method="POST",
            url_path=self.listen_key_endpoint,
        )

        return msgspec.json.decode(raw, type=BinanceListenKey)

    async def ping_listen_key(self, key: str) -> dict[str, Any]:
        """
        Ping/Keep-alive a listen key for the Binance account type.

        Keep-alive a user data stream to prevent a time-out. User data streams
        will close after 60 minutes. It's recommended to send a ping about every
        30 minutes.

        Ping/Keep-alive a ListenKey (USER_STREAM).

        Parameters
        ----------
        key : str
            The listen key for the request.

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#listen-key-spot
        https://binance-docs.github.io/apidocs/spot/en/#listen-key-margin
        https://binance-docs.github.io/apidocs/futures/en/#keepalive-user-data-stream-user_stream
        https://binance-docs.github.io/apidocs/delivery/en/#keepalive-user-data-stream-user_stream

        """
        raw: bytes = await self.client.send_request(
            http_method="PUT",
            url_path=self.listen_key_endpoint,
            payload={"listenKey": key},
        )

        return msgspec.json.decode(raw)

    async def close_listen_key(self, key: str) -> dict[str, Any]:
        """
        Close a listen key for the Binance account type.

        Close a ListenKey (USER_STREAM).

        Parameters
        ----------
        key : str
            The listen key for the request.

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#listen-key-spot
        https://binance-docs.github.io/apidocs/spot/en/#listen-key-margin
        https://binance-docs.github.io/apidocs/futures/en/#close-user-data-stream-user_stream
        https://binance-docs.github.io/apidocs/delivery/en/#close-user-data-stream-user_stream

        """
        raw: bytes = await self.client.send_request(
            http_method="DELETE",
            url_path=self.listen_key_endpoint,
            payload={"listenKey": key},
        )

        return msgspec.json.decode(raw)
