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
#
#  Heavily refactored from MIT licensed github.com/binance/binance-connector-python
#  Original author: Jeremy https://github.com/2pd
# -------------------------------------------------------------------------------------------------

from typing import Any, Dict

from nautilus_trader.adapters.binance.common import format_symbol
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.core.correctness import PyCondition


class BinanceUserDataHttpAPI:
    """
    Provides access to the `Binance Wallet` HTTP REST API.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance REST API client.
    """

    BASE_ENDPOINT_SPOT = "/api/v3/userDataStream"
    BASE_ENDPOINT_MARGIN = "/sapi/v1/userDataStream"
    BASE_ENDPOINT_ISOLATED = "/sapi/v1/userDataStream/isolated"

    def __init__(self, client: BinanceHttpClient):
        PyCondition.not_none(client, "client")

        self.client = client

    async def create_listen_key_spot(self) -> Dict[str, Any]:
        """
        Create a new listen key for the SPOT API.

        Start a new user data stream. The stream will close after 60 minutes
        unless a keepalive is sent. If the account has an active listenKey,
        that listenKey will be returned and its validity will be extended for 60
        minutes.

        Create a ListenKey (USER_STREAM).
        `POST /api/v3/userDataStream `.

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#listen-key-spot

        """
        return await self.client.send_request(
            http_method="POST",
            url_path=self.BASE_ENDPOINT_SPOT,
        )

    async def ping_listen_key_spot(self, key: str) -> Dict[str, Any]:
        """
        Ping/Keep-alive a listen key for the SPOT API.

        Keep-alive a user data stream to prevent a time-out. User data streams
        will close after 60 minutes. It's recommended to send a ping about every
        30 minutes.

        Ping/Keep-alive a ListenKey (USER_STREAM).
        `PUT /api/v3/userDataStream `

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
        return await self.client.send_request(
            http_method="PUT",
            url_path=self.BASE_ENDPOINT_SPOT,
            payload={"listenKey": key},
        )

    async def close_listen_key_spot(self, key: str) -> Dict[str, Any]:
        """
        Close a listen key for the SPOT API.

        Close a ListenKey (USER_STREAM).
        `DELETE /api/v3/userDataStream`.

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
        return await self.client.send_request(
            http_method="DELETE",
            url_path=self.BASE_ENDPOINT_SPOT,
            payload={"listenKey": key},
        )

    async def create_listen_key_margin(self) -> Dict[str, Any]:
        """
        Create a new listen key for the MARGIN API.

        Start a new user data stream. The stream will close after 60 minutes
        unless a keepalive is sent. If the account has an active listenKey,
        that listenKey will be returned and its validity will be extended for 60
        minutes.

        Create a ListenKey (USER_STREAM).
        `POST /api/v3/userDataStream `.

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#listen-key-margin

        """
        return await self.client.send_request(
            http_method="POST",
            url_path=self.BASE_ENDPOINT_MARGIN,
        )

    async def ping_listen_key_margin(self, key: str) -> Dict[str, Any]:
        """
        Ping/Keep-alive a listen key for the MARGIN API.

        Keep-alive a user data stream to prevent a time-out. User data streams
        will close after 60 minutes. It's recommended to send a ping about every
        30 minutes.

        Ping/Keep-alive a ListenKey (USER_STREAM).
        `PUT /api/v3/userDataStream`.

        Parameters
        ----------
        key : str
            The listen key for the request.

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#listen-key-margin

        """
        return await self.client.send_request(
            http_method="PUT",
            url_path=self.BASE_ENDPOINT_MARGIN,
            payload={"listenKey": key},
        )

    async def close_listen_key_margin(self, key: str) -> Dict[str, Any]:
        """
        Close a listen key for the MARGIN API.

        Close a ListenKey (USER_STREAM).
        `DELETE /sapi/v1/userDataStream`.

        Parameters
        ----------
        key : str
            The listen key for the request.

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#listen-key-margin

        """
        return await self.client.send_request(
            http_method="DELETE",
            url_path=self.BASE_ENDPOINT_MARGIN,
            payload={"listenKey": key},
        )

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
        return await self.client.send_request(
            http_method="POST",
            url_path=self.BASE_ENDPOINT_ISOLATED,
            payload={"symbol": format_symbol(symbol).upper()},
        )

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
        return await self.client.send_request(
            http_method="PUT",
            url_path=self.BASE_ENDPOINT_ISOLATED,
            payload={"listenKey": key, "symbol": format_symbol(symbol).upper()},
        )

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
        return await self.client.send_request(
            http_method="DELETE",
            url_path=self.BASE_ENDPOINT_ISOLATED,
            payload={"listenKey": key, "symbol": format_symbol(symbol).upper()},
        )
