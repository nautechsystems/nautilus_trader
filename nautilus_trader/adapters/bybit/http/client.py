# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Any
from urllib import parse

import msgspec

import nautilus_trader
from nautilus_trader.adapters.bybit.http.errors import BybitError
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.core.nautilus_pyo3 import HttpClient
from nautilus_trader.core.nautilus_pyo3 import HttpMethod
from nautilus_trader.core.nautilus_pyo3 import HttpResponse
from nautilus_trader.core.nautilus_pyo3 import Quota


class BybitResponse(msgspec.Struct, frozen=True):
    retCode: int
    retMsg: str
    result: dict[str, Any]
    time: int
    retExtInfo: dict[str, Any] | None = None


class BybitHttpClient:
    """
    Provides a Bybit asynchronous HTTP client.

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
    key : str
        The Bybit API key for requests.
    secret : str
        The Bybit API secret for signed requests.
    base_url : str, optional
        The base endpoint URL for the client.
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
        base_url: str,
        ratelimiter_quotas: list[tuple[str, Quota]] | None = None,
        ratelimiter_default_quota: Quota | None = None,
    ) -> None:
        self._clock: LiveClock = clock
        self._log: Logger = Logger(name=type(self).__name__)
        self._api_key: str = api_key
        self._api_secret: str = api_secret
        self._recv_window: int = 5000

        self._base_url: str = base_url
        self._headers: dict[str, Any] = {
            "Content-Type": "application/json",
            "User-Agent": nautilus_trader.USER_AGENT,
            "X-BAPI-API-KEY": self._api_key,
        }
        self._client = HttpClient(
            keyed_quotas=ratelimiter_quotas or [],
            default_quota=ratelimiter_default_quota,
        )
        self._decoder_response = msgspec.json.Decoder(BybitResponse)

    @property
    def api_key(self) -> str:
        return self._api_key

    @property
    def api_secret(self) -> str:
        return self._api_secret

    async def send_request(
        self,
        http_method: HttpMethod,
        url_path: str,
        payload: dict[str, str] | None = None,
        signature: str | None = None,
        timestamp: str | None = None,
        ratelimiter_keys: list[str] | None = None,
    ) -> bytes:
        if payload and http_method == HttpMethod.GET:
            url_path += "?" + parse.urlencode(payload)
            payload = None
        url = self._base_url + url_path
        if signature is not None:
            headers = {
                **self._headers,
                "X-BAPI-TIMESTAMP": timestamp,
                "X-BAPI-SIGN": signature,
                "X-BAPI-RECV-WINDOW": str(self._recv_window),
            }
        else:
            headers = self._headers

        response: HttpResponse = await self._client.request(
            http_method,
            url,
            headers,
            msgspec.json.encode(payload) if payload else None,
            ratelimiter_keys,
        )

        if response.status >= 400:
            raise BybitError(
                code=response.status,
                message=msgspec.json.decode(response.body) if response.body else None,
            )

        bybit_resp: BybitResponse = self._decoder_response.decode(response.body)
        if bybit_resp.retCode == 0:
            return response.body
        else:
            raise BybitError(code=bybit_resp.retCode, message=bybit_resp.retMsg)

    async def sign_request(
        self,
        http_method: HttpMethod,
        url_path: str,
        payload: dict[str, str] | None = None,
        ratelimiter_keys: list[str] | None = None,
    ) -> Any:
        if payload is None:
            payload = {}

        [timestamp, authed_signature] = (
            self._sign_get_request(payload)
            if http_method == HttpMethod.GET
            else self._sign_post_request(payload)
        )
        return await self.send_request(
            http_method=http_method,
            url_path=url_path,
            payload=payload,
            signature=authed_signature,
            timestamp=timestamp,
            ratelimiter_keys=ratelimiter_keys,
        )

    def _sign_post_request(self, payload: dict[str, Any]) -> list[str]:
        timestamp = str(self._clock.timestamp_ms())
        payload_str = msgspec.json.encode(payload).decode()
        result = timestamp + self._api_key + str(self._recv_window) + payload_str
        signature = hmac.new(
            self._api_secret.encode(),
            result.encode(),
            hashlib.sha256,
        ).hexdigest()
        return [timestamp, signature]

    def _sign_get_request(self, payload: dict[str, Any]) -> list[str]:
        timestamp = str(self._clock.timestamp_ms())
        payload_str = parse.urlencode(payload)
        result = timestamp + self._api_key + str(self._recv_window) + payload_str
        signature = hmac.new(
            self._api_secret.encode(),
            result.encode(),
            hashlib.sha256,
        ).hexdigest()
        return [timestamp, signature]
