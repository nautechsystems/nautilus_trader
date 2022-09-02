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

import asyncio
import hashlib
import hmac
from typing import Any, Dict, Optional

import aiohttp
import msgspec

import nautilus_trader
from nautilus_trader.adapters.binance.http.error import BinanceClientError
from nautilus_trader.adapters.binance.http.error import BinanceServerError
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.network.http import HttpClient


class BinanceHttpClient(HttpClient):
    """
    Provides a `Binance` asynchronous HTTP client.
    """

    BASE_URL = "https://api.binance.com"  # Default Spot/Margin

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        clock: LiveClock,
        logger: Logger,
        key: Optional[str] = None,
        secret: Optional[str] = None,
        base_url: Optional[str] = None,
        timeout: Optional[int] = None,
        show_limit_usage: bool = False,
    ):
        super().__init__(
            loop=loop,
            logger=logger,
        )
        self._clock = clock
        self._key = key
        self._secret = secret
        self._base_url = base_url or self.BASE_URL
        self._show_limit_usage = show_limit_usage
        self._proxies = None
        self._headers: Dict[str, Any] = {
            "Content-Type": "application/json;charset=utf-8",
            "User-Agent": "nautilus-trader/" + nautilus_trader.__version__,
            "X-MBX-APIKEY": key,
        }

        if timeout is not None:
            self._headers["timeout"] = timeout

        # TODO(cs): Implement limit usage

    @property
    def base_url(self) -> str:
        return self._base_url

    @property
    def api_key(self) -> str:
        return self._key

    @property
    def api_secret(self) -> str:
        return self._secret

    @property
    def headers(self):
        return self._headers

    async def query(self, url_path, payload: Dict[str, str] = None) -> Any:
        return await self.send_request("GET", url_path, payload=payload)

    async def limit_request(
        self,
        http_method: str,
        url_path: str,
        payload: Dict[str, Any] = None,
    ) -> Any:
        """
        Limit request is for those endpoints requiring an API key in the header.
        """
        return await self.send_request(http_method, url_path, payload=payload)

    async def sign_request(
        self,
        http_method: str,
        url_path: str,
        payload: Dict[str, str] = None,
    ) -> Any:
        if payload is None:
            payload = {}
        payload["timestamp"] = str(self._clock.timestamp_ms())
        query_string = self._prepare_params(payload)
        signature = self._get_sign(query_string)
        payload["signature"] = signature
        return await self.send_request(http_method, url_path, payload)

    async def limited_encoded_sign_request(
        self,
        http_method: str,
        url_path: str,
        payload: Dict[str, str] = None,
    ) -> Any:
        """
        Limit encoded sign request.

        This is used for some endpoints has special symbol in the url.
        In some endpoints these symbols should not encoded.
        - @
        - [
        - ]
        so we have to append those parameters in the url.
        """
        if payload is None:
            payload = {}
        payload["timestamp"] = str(self._clock.timestamp_ms())
        query_string = self._prepare_params(payload)
        signature = self._get_sign(query_string)
        url_path = url_path + "?" + query_string + "&signature=" + signature
        return await self.send_request(http_method, url_path)

    async def send_request(
        self,
        http_method: str,
        url_path: str,
        payload: Dict[str, str] = None,
    ) -> Any:
        # TODO(cs): Uncomment for development
        # print(f"{http_method} {url_path} {payload}")
        if payload is None:
            payload = {}
        try:
            resp: aiohttp.ClientResponse = await self.request(
                method=http_method,
                url=self._base_url + url_path,
                headers=self._headers,
                params=self._prepare_params(payload),
            )
        except aiohttp.ServerDisconnectedError:
            self._log.error("Server was disconnected.")
            return b""
        except aiohttp.ClientResponseError as e:
            await self._handle_exception(e)
            return

        if self._show_limit_usage:
            limit_usage = {}
            for key in resp.headers.keys():
                key = key.lower()
                if (
                    key.startswith("x-mbx-used-weight")
                    or key.startswith("x-mbx-order-count")
                    or key.startswith("x-sapi-used")
                ):
                    limit_usage[key] = resp.headers[key]

        try:
            return resp.data
        except msgspec.MsgspecError:
            self._log.error(f"Could not decode data to JSON: {resp.data}.")

    def _get_sign(self, data) -> str:
        m = hmac.new(self._secret.encode(), data.encode(), hashlib.sha256)
        return m.hexdigest()

    async def _handle_exception(self, error: aiohttp.ClientResponseError) -> None:
        if error.status < 400:
            return
        elif 400 <= error.status < 500:
            raise BinanceClientError(
                status=error.status,
                message=error.message,
                headers=error.headers,
            )
        else:
            raise BinanceServerError(
                status=error.status,
                message=error.message,
                headers=error.headers,
            )
