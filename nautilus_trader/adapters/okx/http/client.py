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

import base64
import datetime
import json
from typing import Any
from urllib import parse

import msgspec

import nautilus_trader
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.core.nautilus_pyo3 import HttpClient
from nautilus_trader.core.nautilus_pyo3 import HttpMethod
from nautilus_trader.core.nautilus_pyo3 import HttpResponse
from nautilus_trader.core.nautilus_pyo3 import Quota
from nautilus_trader.core.nautilus_pyo3 import hmac_signature
from nautilus_trader.okx.common.error import raise_okx_error
from nautilus_trader.okx.http.errors import OKXHttpError


class OKXResponseCode(msgspec.Struct):
    code: str


HTTP_METHOD_STRINGS = {
    HttpMethod.GET: "GET",
    HttpMethod.POST: "POST",
    HttpMethod.PUT: "PUT",
    HttpMethod.DELETE: "DELETE",
    HttpMethod.PATCH: "PATCH",
}


class OKXHttpClient:
    """
    Provides a OKX asynchronous HTTP client.

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
    key : str
        The OKX API key for requests.
    secret : str
        The OKX API secret for signed requests.
    passphrase : str
        The passphrase used when creating the OKX API keys (for signed requests).
    base_url : str, optional
        The base endpoint URL for the client.
    is_demo : bool
        Whether the client is to be used for demo trading or not.
    ratelimiter_quotas : list[tuple[str, Quota]], optional
        The keyed rate limiter quotas for the client.
    ratelimiter_quota : Quota, optional
        The default rate limiter quota for the client.

    """

    def __init__(
        self,
        clock: LiveClock,
        api_key: str,
        api_secret: str,
        passphrase: str,
        base_url: str,
        is_demo: bool,
        default_timeout_secs: int | None = None,
        ratelimiter_quotas: list[tuple[str, Quota]] | None = None,
        ratelimiter_default_quota: Quota | None = None,
    ) -> None:
        self._clock: LiveClock = clock
        self._log: Logger = Logger(name=type(self).__name__)
        self._api_key: str = api_key
        self._api_secret: str = api_secret
        self._passphrase: str = passphrase
        self._base_url: str = base_url
        self._is_demo: bool = is_demo
        self._default_timeout_secs = default_timeout_secs

        self._headers: dict[str, Any] = {
            "Content-Type": "application/json",
            "User-Agent": nautilus_trader.NAUTILUS_USER_AGENT,
        }
        self._client = HttpClient(
            keyed_quotas=ratelimiter_quotas or [],
            default_quota=ratelimiter_default_quota,
        )
        self._decoder_response_code = msgspec.json.Decoder(OKXResponseCode)

    @property
    def api_key(self) -> str:
        return self._api_key

    @property
    def api_secret(self) -> str:
        return self._api_secret

    @property
    def passphrase(self) -> str:
        return self._passphrase

    @property
    def base_url(self) -> str:
        return self._base_url

    @property
    def is_demo(self) -> bool:
        return self._is_demo

    def _get_timestamp(self) -> str:
        now = datetime.datetime.now(datetime.UTC)
        t = now.isoformat("T", "milliseconds")
        return t.split("+")[0] + "Z"

    def _sign(self, timestamp: str, method: str, url_path: str, body: str) -> str:
        if body == "{}" or body == "None":
            body = ""
        message = str(timestamp) + method.upper() + url_path + body
        digest = hmac_signature(self._api_secret, message).encode()
        return base64.b64encode(digest).decode()

    async def send_request(
        self,
        http_method: HttpMethod,
        url_path: str,
        payload: dict[str, str] | None = None,
        ratelimiter_keys: list[str] | None = None,
        timeout_secs: int | None = None,
        sign: bool = False,
    ) -> bytes | None:
        if payload and http_method == HttpMethod.GET:
            url_path += "?" + parse.urlencode(payload)
            payload = None
        url = self._base_url + url_path
        body = json.dumps(payload) if http_method == HttpMethod.POST else ""
        if sign:
            timestamp = self._get_timestamp()
            signature = self._sign(timestamp, HTTP_METHOD_STRINGS[http_method], url_path, body)
            headers = {
                **self._headers,
                "OK-ACCESS-KEY": self._api_key,
                "OK-ACCESS-SIGN": signature,
                "OK-ACCESS-TIMESTAMP": timestamp,
                "OK-ACCESS-PASSPHRASE": self._passphrase,
            }
        else:
            headers = self._headers

        if self.is_demo:
            headers["x-simulated-trading"] = "1"

        # Uncomment for development
        # self._log.info(f"{url_path=}, {payload=}", LogColor.MAGENTA)

        response: HttpResponse = await self._client.request(
            method=http_method,
            url=url,
            headers=headers,
            body=body.encode(),
            keys=ratelimiter_keys,
            timeout_secs=timeout_secs or self._default_timeout_secs,
        )
        # First check for server error
        if 400 <= response.status < 500:
            message = msgspec.json.decode(response.body) if response.body else None
            raise OKXHttpError(
                status=response.status,
                message=message or "",
                headers=response.headers,
            )
        # Then check for error inside response
        okx_response_code = self._decoder_response_code.decode(response.body)
        if okx_response_code.code == "0":
            return response.body
        else:
            message = msgspec.json.decode(response.body) if response.body else None
            raise_okx_error(
                error_code=int(okx_response_code.code),
                status_code=response.status,
                message=message,
            )
        return None

    async def sign_request(
        self,
        http_method: HttpMethod,
        url_path: str,
        payload: dict[str, str] | None = None,
        ratelimiter_keys: list[str] | None = None,
        timeout_secs: int | None = None,
    ) -> Any:
        if payload is None:
            payload = {}

        return await self.send_request(
            http_method=http_method,
            url_path=url_path,
            payload=payload,
            ratelimiter_keys=ratelimiter_keys,
            timeout_secs=timeout_secs,
            sign=True,
        )
