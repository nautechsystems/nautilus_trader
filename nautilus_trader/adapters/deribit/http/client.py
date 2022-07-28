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
import json
import urllib.parse
from typing import Any, Dict, Optional

import msgspec
from aiohttp import ClientResponse
from aiohttp import ClientResponseError

from nautilus_trader.adapters.deribit.http.error import DeribitClientError
from nautilus_trader.adapters.deribit.http.error import DeribitServerError
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.network.http import HttpClient


class DeribitHttpClient(HttpClient):
    """
    Provides a `Deribit` asynchronous HTTP client.
    """

    BASE_URL = "https://www.deribit.com"

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        clock: LiveClock,
        logger: Logger,
        key: Optional[str] = None,
        secret: Optional[str] = None,
        base_url: Optional[str] = None,
    ):
        super().__init__(
            loop=loop,
            logger=logger,
        )
        self._clock = clock
        self._key = key
        self._secret = secret
        self._base_url = base_url or self.BASE_URL
        self._nonce = 0

    @property
    def base_url(self) -> str:
        return self._base_url

    @property
    def api_key(self) -> str:
        return self._key

    @property
    def api_secret(self) -> str:
        return self._secret

    @staticmethod
    def _prepare_payload(payload: Dict[str, str]) -> Optional[str]:
        return json.dumps(payload, separators=(",", ":")) if payload else None

    @staticmethod
    def _url_encode(params: Dict[str, str]) -> str:
        return "?" + urllib.parse.urlencode(params) if params else ""

    def _generate_authorization(
        self,
        action,
        uri,
        payload,
    ):
        # https://docs.deribit.com/#authentication

        tstamp = self._clock.timestamp_ms()

        nonce = self._nonce
        self._nonce += 1

        body = '{"jsonrpc": "2.0","id": "5647","method": "private/list_api_keys"}'

        request_data = action + "\n" + uri + "\n" + body + "\n"
        base_signature_string = tstamp + "\n" + nonce + "\n" + request_data
        byte_key = self._secret.encode()
        message = base_signature_string.encode()
        sig = hmac.new(byte_key, message, hashlib.sha256).hexdigest()

        authorization = (
            "deri-hmac-sha256 id=" + self._key + ",ts=" + tstamp + ",sig=" + sig + ",nonce=" + nonce
        )

        return authorization

    async def _sign_request(
        self,
        http_method: str,
        url_path: str,
        payload: Dict[str, str] = None,
        params: Dict[str, Any] = None,
    ) -> Any:
        auth = self._generate_authorization(
            http_method,
            url_path,
            payload,
        )
        headers = {"Authorization": auth}

        return await self._send_request(
            http_method=http_method,
            url_path=url_path,
            headers=headers,
            payload=payload,
            params=params,
        )

    async def _send_request(
        self,
        http_method: str,
        url_path: str,
        headers: Dict[str, Any] = None,
        payload: Dict[str, str] = None,
        params: Dict[str, str] = None,
    ) -> Any:
        if payload is None:
            payload = {}
        # TODO(cs): Uncomment for development
        print(f"{http_method} {url_path} {headers} {payload}")
        query = self._url_encode(params)
        try:
            resp: ClientResponse = await self.request(
                method=http_method,
                url=self._base_url + url_path + query,
                headers=headers,
                data=self._prepare_payload(payload),
            )
        except ClientResponseError as e:
            await self._handle_exception(e)
            return

        try:
            data = msgspec.json.decode(resp.data)
            if not data["success"]:
                return data["error"]
            return data["result"]
        except msgspec.MsgspecError:
            self._log.error(f"Could not decode data to JSON: {resp.data}.")

    async def _handle_exception(self, error: ClientResponseError) -> None:
        if error.status < 400:
            return
        elif 400 <= error.status < 500:
            raise DeribitClientError(
                status=error.status,
                message=error.message,
                headers=error.headers,
            )
        else:
            raise DeribitServerError(
                status=error.status,
                message=error.message,
                headers=error.headers,
            )

    async def access_log(self):
        return await self._sign_request("GET", "/api/v2/private/get_access_log", {})
