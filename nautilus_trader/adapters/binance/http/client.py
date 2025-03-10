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

import urllib.parse
from typing import Any

import msgspec

import nautilus_trader
from nautilus_trader.adapters.binance.common.enums import BinanceKeyType
from nautilus_trader.adapters.binance.http.error import BinanceClientError
from nautilus_trader.adapters.binance.http.error import BinanceServerError
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.nautilus_pyo3 import HttpClient
from nautilus_trader.core.nautilus_pyo3 import HttpMethod
from nautilus_trader.core.nautilus_pyo3 import HttpResponse
from nautilus_trader.core.nautilus_pyo3 import Quota
from nautilus_trader.core.nautilus_pyo3 import ed25519_signature
from nautilus_trader.core.nautilus_pyo3 import hmac_signature
from nautilus_trader.core.nautilus_pyo3 import rsa_signature


class BinanceHttpClient:
    """
    Provides a Binance asynchronous HTTP client.

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
    api_key : str
        The Binance API key for requests.
    api_secret : str
        The Binance API secret for signed requests.
    key_type : BinanceKeyType, default 'HMAC'
        The private key cryptographic algorithm type.
    rsa_private_key : str, optional
        The RSA private key for RSA signing.
    ed25519_private_key : str, optional
        The Ed25519 private key for Ed25519 signing.
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
        key_type: BinanceKeyType = BinanceKeyType.HMAC,
        rsa_private_key: str | None = None,
        ed25519_private_key: str | None = None,
        ratelimiter_quotas: list[tuple[str, Quota]] | None = None,
        ratelimiter_default_quota: Quota | None = None,
    ) -> None:
        self._clock: LiveClock = clock
        self._log: Logger = Logger(type(self).__name__)
        self._key: str = api_key

        self._base_url: str = base_url
        self._secret: str = api_secret
        self._key_type: BinanceKeyType = key_type
        self._rsa_private_key: str | None = rsa_private_key
        self._ed25519_private_key: bytes | None = (
            ed25519_private_key.encode() if ed25519_private_key else None
        )

        self._headers: dict[str, Any] = {
            "Content-Type": "application/json",
            "User-Agent": nautilus_trader.NAUTILUS_USER_AGENT,
            "X-MBX-APIKEY": api_key,
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
            case BinanceKeyType.HMAC:
                return hmac_signature(self._secret, data)
            case BinanceKeyType.RSA:
                if not self._rsa_private_key:
                    raise ValueError("`rsa_private_key` was `None`")
                return rsa_signature(self._rsa_private_key, data)
            case BinanceKeyType.ED25519:
                if not self._ed25519_private_key:
                    raise ValueError("`ed25519_private_key` was `None`")
                return ed25519_signature(self._ed25519_private_key, data)
            case _:
                # Theoretically unreachable but retained to keep the match exhaustive
                raise ValueError(f"Unsupported key type, was '{self._key_type.value}'")

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

        response_body = response.body

        if response.status >= 400:
            try:
                message = msgspec.json.decode(response_body) if response_body else None
            except msgspec.DecodeError:
                message = response_body.decode()

            if response.status >= 500:
                raise BinanceServerError(
                    status=response.status,
                    message=message,
                    headers=response.headers,
                )
            else:
                raise BinanceClientError(
                    status=response.status,
                    message=message,
                    headers=response.headers,
                )

        return response.body
