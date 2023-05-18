# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import hashlib
import hmac
import urllib.parse
from typing import Any, Optional

import msgspec

import nautilus_trader
from nautilus_trader.adapters.binance.http.error import BinanceClientError
from nautilus_trader.adapters.binance.http.error import BinanceServerError
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.nautilus_pyo3.network import HttpClient
from nautilus_trader.core.nautilus_pyo3.network import HttpResponse


class BinanceHttpClient:
    """
    Provides a `Binance` asynchronous HTTP client.
    """

    BASE_URL = "https://api.binance.com"  # Default Spot/Margin

    def __init__(
        self,
        clock: LiveClock,
        logger: Logger,
        key: str,
        secret: str,
        base_url: Optional[str] = None,
        timeout: Optional[int] = None,
        show_limit_usage: bool = False,
    ):
        self._clock = clock
        self._key = key
        self._secret = secret
        self._base_url = base_url or self.BASE_URL
        self._show_limit_usage = show_limit_usage
        self._proxies = None
        self._headers: dict[str, Any] = {
            "User-Agent": "nautilus-trader/" + nautilus_trader.__version__,
            "X-MBX-APIKEY": key,
        }
        self._client = HttpClient(debug=True)

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
    def headers(self):
        return self._headers

    def _prepare_params(self, params: dict[str, Any]) -> str:
        # Encode a dict into a URL query string
        return urllib.parse.urlencode(params)

    def _get_sign(self, data: str) -> str:
        m = hmac.new(self._secret.encode(), data.encode(), hashlib.sha256)
        return m.hexdigest()

    async def limit_request(
        self,
        http_method: str,
        url_path: str,
        payload: Optional[dict[str, Any]] = None,
    ) -> Any:
        """
        Limit request is for those endpoints requiring an API key in the header.
        """
        return await self.send_request(
            http_method,
            url_path,
            payload=payload,
        )

    async def sign_request(
        self,
        http_method: str,
        url_path: str,
        payload: Optional[dict[str, str]] = None,
    ) -> Any:
        if payload is None:
            payload = {}
        query_string = self._prepare_params(payload)
        signature = self._get_sign(query_string)
        payload["signature"] = signature
        return await self.send_request(
            http_method,
            url_path,
            payload=payload,
        )

    async def limited_encoded_sign_request(
        self,
        http_method: str,
        url_path: str,
        payload: Optional[dict[str, str]] = None,
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
        query_string = self._prepare_params(payload)
        signature = self._get_sign(query_string)
        url_path = url_path + "?" + query_string + "&signature=" + signature
        return await self.send_request(
            http_method,
            url_path,
            payload=payload,
        )

    async def send_request(
        self,
        http_method: str,
        url_path: str,
        payload: Optional[dict[str, str]] = None,
    ) -> bytes:
        print("############################################")
        print(http_method)
        print(url_path)
        print(payload)
        print("############################################")
        response: HttpResponse = await self._client.request(
            http_method,
            url=self._base_url + url_path,
            headers=self._headers,
            body=msgspec.json.encode(payload) if payload else None,
        )

        if 400 <= response.status < 500:
            raise BinanceClientError(
                status=response.status,
                message=response.body,
                headers=response.headers,
            )
        elif response.status >= 500:
            raise BinanceServerError(
                status=response.status,
                message=response.body,
                headers=response.headers,
            )

        return response.body
