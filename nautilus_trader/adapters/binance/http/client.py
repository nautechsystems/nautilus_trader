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
import urllib.parse
from base64 import b64encode
from typing import Any

import msgspec
from Crypto.Hash import SHA256
from Crypto.Signature import pkcs1_15
from nacl.signing import SigningKey

import nautilus_trader
from nautilus_trader.adapters.binance.http.error import BinanceClientError
from nautilus_trader.adapters.binance.http.error import BinanceServerError
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.nautilus_pyo3 import HttpClient
from nautilus_trader.core.nautilus_pyo3 import HttpMethod
from nautilus_trader.core.nautilus_pyo3 import HttpResponse
from nautilus_trader.core.nautilus_pyo3 import Quota


class BinanceHttpClient:
    """
    Provides a Binance asynchronous HTTP client.

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
    key : str
        The Binance API key for requests.
    secret : str
        The Binance API secret for signed requests.
    base_url : str, optional
        The base endpoint URL for the client.
    ratelimiter_quotas : list[tuple[str, Quota]], optional
        The keyed rate limiter quotas for the client.
    ratelimiter_quota : Quota, optional
        The default rate limiter quota for the client.
    key_type : str, optional
        The type of API key (HMAC, RSA, Ed25519).
    rsa_private_key : str, optional
        The RSA private key for RSA signing.
    ed25519_private_key : str, optional
        The Ed25519 private key for Ed25519 signing.

    """

    def __init__(
        self,
        clock: LiveClock,
        key: str,
        secret: str,
        base_url: str,
        ratelimiter_quotas: list[tuple[str, Quota]] | None = None,
        ratelimiter_default_quota: Quota | None = None,
        key_type: str = "HMAC",
        rsa_private_key: str | None = None,
        ed25519_private_key: str | None = None,
    ) -> None:
        self._clock: LiveClock = clock
        self._log: Logger = Logger(type(self).__name__)
        self._key: str = key

        self._base_url: str = base_url
        self._secret: str = secret
        self._key_type: str = key_type
        self._rsa_private_key: str | None = rsa_private_key
        self._ed25519_private_key: str | None = ed25519_private_key

        self._headers: dict[str, Any] = {
            "Content-Type": "application/json",
            "User-Agent": nautilus_trader.USER_AGENT,
            "X-MBX-APIKEY": key,
        }
        self._client = HttpClient(
            keyed_quotas=ratelimiter_quotas or [],
            default_quota=ratelimiter_default_quota,
        )

    @property
    def base_url(self) -> str:
        """
        Return the base URL being used by the client.

        Returns
        -------
        str

        """
        return self._base_url

    @property
    def api_key(self) -> str:
        """
        Return the Binance API key being used by the client.

        Returns
        -------
        str

        """
        return self._key

    @property
    def headers(self):
        """
        Return the headers being used by the client.

        Returns
        -------
        str

        """
        return self._headers

    def _prepare_params(self, params: dict[str, Any]) -> str:
        # Encode a dict into a URL query string
        return urllib.parse.urlencode(params)

    def _get_sign(self, data: str) -> str:
        match self._key_type:
            case "HMAC":
                return self._hmac_sign(data)
            case "RSA":
                return self._rsa_signature(data)
            case "Ed25519":
                return self._ed25519_signature(data)
            case _:
                raise ValueError(f"Unsupported key type, was {self._key_type}")

    def _hmac_sign(self, data: str) -> str:
        m = hmac.new(self._secret.encode(), data.encode(), hashlib.sha256)
        return m.hexdigest()

    def _rsa_signature(self, query_string: str) -> str:
        assert self._rsa_private_key
        h = SHA256.new(query_string.encode())
        signature = pkcs1_15.new(self._rsa_private_key).sign(h)
        return b64encode(signature).decode()

    def _ed25519_signature(self, query_string: str) -> str:
        assert self._ed25519_private_key
        signing_key = SigningKey(self._ed25519_private_key.encode())
        signed_message = signing_key.sign(query_string.encode())
        return b64encode(signed_message.signature).decode()

    async def sign_request(
        self,
        http_method: HttpMethod,
        url_path: str,
        payload: dict[str, str] | None = None,
        ratelimiter_keys: list[str] | None = None,
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
            ratelimiter_keys=ratelimiter_keys,
        )

    async def send_request(
        self,
        http_method: HttpMethod,
        url_path: str,
        payload: dict[str, str] | None = None,
        ratelimiter_keys: list[str] | None = None,
    ) -> bytes:
        if payload:
            url_path += "?" + urllib.parse.urlencode(payload)
            payload = None  # Don't send payload in the body

        self._log.debug(f"{url_path} {payload}", LogColor.MAGENTA)

        response: HttpResponse = await self._client.request(
            http_method,
            url=self._base_url + url_path,
            headers=self._headers,
            body=msgspec.json.encode(payload) if payload else None,
            keys=ratelimiter_keys,
        )

        if 400 <= response.status < 500:
            raise BinanceClientError(
                status=response.status,
                message=msgspec.json.decode(response.body) if response.body else None,
                headers=response.headers,
            )
        elif response.status >= 500:
            raise BinanceServerError(
                status=response.status,
                message=msgspec.json.decode(response.body) if response.body else None,
                headers=response.headers,
            )

        return response.body
