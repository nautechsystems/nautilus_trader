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
from nautilus_trader.adapters.binance.common.functions import format_symbol
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient


class BinanceSpotUserDataHttpAPI:
    """
    Provides access to the `Binance Spot/Margin` User Data HTTP REST API.

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
        self.client = client
        self.account_type = account_type

        if account_type == BinanceAccountType.SPOT:
            self.BASE_ENDPOINT = "/api/v3/"
        elif account_type == BinanceAccountType.MARGIN:
            self.BASE_ENDPOINT = "sapi/v1/"
        else:  # pragma: no cover (design-time error)
            raise RuntimeError(f"invalid Binance Spot/Margin account type, was {account_type}")

    async def create_listen_key(self) -> Dict[str, Any]:
        """
        Create a new listen key for the Binance Spot/Margin.

        Start a new user data stream. The stream will close after 60 minutes
        unless a keepalive is sent. If the account has an active listenKey,
        that listenKey will be returned and its validity will be extended for 60
        minutes.

        Create a ListenKey (USER_STREAM).

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#listen-key-spot

        """
        raw: bytes = await self.client.send_request(
            http_method="POST",
            url_path=self.BASE_ENDPOINT + "userDataStream",
        )

        return msgspec.json.decode(raw)

    async def ping_listen_key(self, key: str) -> Dict[str, Any]:
        """
        Ping/Keep-alive a listen key for the Binance Spot/Margin API.

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

        """
        raw: bytes = await self.client.send_request(
            http_method="PUT",
            url_path=self.BASE_ENDPOINT + "userDataStream",
            payload={"listenKey": key},
        )

        return msgspec.json.decode(raw)

    async def close_listen_key(self, key: str) -> Dict[str, Any]:
        """
        Close a listen key for the Binance Spot/Margin API.

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

        """
        raw: bytes = await self.client.send_request(
            http_method="DELETE",
            url_path=self.BASE_ENDPOINT + "userDataStream",
            payload={"listenKey": key},
        )

        return msgspec.json.decode(raw)

    async def create_listen_key_isolated_margin(self, symbol: str) -> Dict[str, Any]:
        """
        Create a new listen key for the ISOLATED MARGIN API.

        Start a new user data stream. The stream will close after 60 minutes
        unless a keepalive is sent. If the account has an active listenKey,
        that listenKey will be returned and its validity will be extended for 60
        minutes.

        Create a ListenKey (USER_STREAM).
        `POST /api/v3/userDataStream `.

        Parameters
        ----------
        symbol : str
            The symbol for the listen key request.

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#listen-key-isolated-margin

        """
        raw: bytes = await self.client.send_request(
            http_method="POST",
            url_path="/sapi/v1/userDataStream/isolated",
            payload={"symbol": format_symbol(symbol)},
        )

        return msgspec.json.decode(raw)

    async def ping_listen_key_isolated_margin(self, symbol: str, key: str) -> Dict[str, Any]:
        """
        Ping/Keep-alive a listen key for the ISOLATED MARGIN API.

        Keep-alive a user data stream to prevent a time-out. User data streams
        will close after 60 minutes. It's recommended to send a ping about every
        30 minutes.

        Ping/Keep-alive a ListenKey (USER_STREAM).
        `PUT /api/v3/userDataStream`.

        Parameters
        ----------
        symbol : str
            The symbol for the listen key request.
        key : str
            The listen key for the request.

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#listen-key-isolated-margin

        """
        raw: bytes = await self.client.send_request(
            http_method="PUT",
            url_path="/sapi/v1/userDataStream/isolated",
            payload={"listenKey": key, "symbol": format_symbol(symbol)},
        )

        return msgspec.json.decode(raw)

    async def close_listen_key_isolated_margin(self, symbol: str, key: str) -> Dict[str, Any]:
        """
        Close a listen key for the ISOLATED MARGIN API.

        Close a ListenKey (USER_STREAM).
        `DELETE /sapi/v1/userDataStream`.

        Parameters
        ----------
        symbol : str
            The symbol for the listen key request.
        key : str
            The listen key for the request.

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#listen-key-isolated-margin

        """
        raw: bytes = await self.client.send_request(
            http_method="DELETE",
            url_path="/sapi/v1/userDataStream/isolated",
            payload={"listenKey": key, "symbol": format_symbol(symbol)},
        )

        return msgspec.json.decode(raw)
