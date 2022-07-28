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

from typing import Any, Dict

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.schemas import BinanceListenKey
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.core.correctness import PyCondition


class BinanceFuturesUserDataHttpAPI:
    """
    Provides access to the `Binance Futures` User Data HTTP REST API.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance REST API client.
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        account_type: BinanceAccountType = BinanceAccountType.FUTURES_USDT,
    ):
        PyCondition.not_none(client, "client")

        self.client = client
        self.account_type = account_type

        if account_type == BinanceAccountType.FUTURES_USDT:
            self.BASE_ENDPOINT = "/fapi/v1/"
        elif account_type == BinanceAccountType.FUTURES_COIN:
            self.BASE_ENDPOINT = "/dapi/v1/"
        else:  # pragma: no cover (design-time error)
            raise RuntimeError(f"invalid Binance account type, was {account_type}")

    async def create_listen_key(self) -> BinanceListenKey:
        """
        Create a new listen key for the Binance FUTURES_USDT or FUTURES_COIN API.

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
        https://binance-docs.github.io/apidocs/futures/en/#start-user-data-stream-user_stream

        """
        raw: bytes = await self.client.send_request(
            http_method="POST",
            url_path=self.BASE_ENDPOINT + "listenKey",
        )

        return msgspec.json.decode(raw, type=BinanceListenKey)

    async def ping_listen_key(self, key: str) -> Dict[str, Any]:
        """
        Ping/Keep-alive a listen key for the Binance FUTURES_USDT or FUTURES_COIN API.

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
        https://binance-docs.github.io/apidocs/futures/en/#keepalive-user-data-stream-user_stream

        """
        raw: bytes = await self.client.send_request(
            http_method="PUT",
            url_path=self.BASE_ENDPOINT + "listenKey",
            payload={"listenKey": key},
        )

        return msgspec.json.decode(raw)

    async def close_listen_key(self, key: str) -> Dict[str, Any]:
        """
        Close a user data stream for the Binance FUTURES_USDT or FUTURES_COIN API.

        Parameters
        ----------
        key : str
            The listen key for the request.

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/futures/en/#close-user-data-stream-user_stream

        """
        raw: bytes = await self.client.send_request(
            http_method="DELETE",
            url_path=self.BASE_ENDPOINT + "listenKey",
            payload={"listenKey": key},
        )

        return msgspec.json.decode(raw)
