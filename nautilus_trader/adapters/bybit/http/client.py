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
import urllib
from typing import Any

import aiohttp
import msgspec

import nautilus_trader
from nautilus_trader.adapters.bybit.common.error import raise_bybit_error
from nautilus_trader.adapters.bybit.http.errors import BybitError
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.core.nautilus_pyo3 import HttpClient
from nautilus_trader.core.nautilus_pyo3 import HttpMethod
from nautilus_trader.core.nautilus_pyo3 import HttpResponse
from nautilus_trader.core.nautilus_pyo3 import Quota


def create_string_from_dict(data):
    property_strings = []

    for key, value in data.items():
        property_string = f'"{key}":"{value}"'
        property_strings.append(property_string)

    result_string = "{" + ",".join(property_strings) + "}"
    return result_string


class ResponseCode(msgspec.Struct):
    retCode: int


class BybitHttpClient:
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
        self._recv_window: int = 8000

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
        self._decoder_response_code = msgspec.json.Decoder(ResponseCode)

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
    ) -> bytes | None:
        if payload and http_method == HttpMethod.GET:
            url_path += "?" + urllib.parse.urlencode(payload)
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
        # first check for server error
        if 400 <= response.status < 500:
            message = msgspec.json.decode(response.body) if response.body else None
            print(str(response.body))
            raise BybitError(
                status=response.status,
                message=message,
                headers=response.headers,
            )
        # then check for error inside spot response
        response_status = self._decoder_response_code.decode(response.body)
        if response_status.retCode == 0:
            return response.body
        else:
            raise_bybit_error(response_status.retCode)
        return None

    async def sign_request(
        self,
        http_method: HttpMethod,
        url_path: str,
        payload: dict[str, str] | None = None,
        ratelimiter_keys: list[str] | None = None,
    ) -> Any:
        if payload is None:
            payload = {}
        # we need to get timestamp and signature

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

    def _handle_exception(self, error: aiohttp.ClientResponseError):
        self._log.error(
            f"Some exception in HTTP request status: {error.status} message:{error.message}",
        )

    def _sign_post_request(self, payload: dict[str, Any]) -> list[str]:
        timestamp = str(self._clock.timestamp_ms())
        payload_str = create_string_from_dict(payload)
        result = timestamp + self._api_key + str(self._recv_window) + payload_str
        signature = hmac.new(
            self._api_secret.encode("utf-8"),
            result.encode("utf-8"),
            hashlib.sha256,
        ).hexdigest()
        return [timestamp, signature]

    def _sign_get_request(self, payload: dict[str, Any]) -> list[str]:
        timestamp = str(self._clock.timestamp_ms())
        payload_str = urllib.parse.urlencode(payload)
        result = timestamp + self._api_key + str(self._recv_window) + payload_str
        signature = hmac.new(
            self._api_secret.encode("utf-8"),
            result.encode("utf-8"),
            hashlib.sha256,
        ).hexdigest()
        return [timestamp, signature]
