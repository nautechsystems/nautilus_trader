# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
import urllib.parse
from typing import Any

import msgspec

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
from nautilus_trader.core.nautilus_pyo3 import ed25519_signature
from nautilus_trader.core.nautilus_pyo3 import hmac_signature
from nautilus_trader.core.nautilus_pyo3 import mask_api_key
from nautilus_trader.core.nautilus_pyo3 import rsa_signature


class BinanceHttpClient:
    """
    Provides a Binance asynchronous HTTP client.

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
    api_key : str, optional
        The Binance API key for requests.
        If ``None``, the client will work for public market data only.
    api_secret : str, optional
        The Binance API secret for signed requests.
        If ``None``, the client will work for public market data only.
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
    proxy_url : str, optional
        The proxy URL for HTTP requests.

    """

    def __init__(
        self,
        clock: LiveClock,
        api_key: str | None,
        api_secret: str | None,
        base_url: str,
        rsa_private_key: str | None = None,
        ed25519_private_key: str | None = None,
        ratelimiter_quotas: list[tuple[str, Quota]] | None = None,
        ratelimiter_default_quota: Quota | None = None,
        proxy_url: str | None = None,
    ) -> None:
        self._clock: LiveClock = clock
        self._log: Logger = Logger(type(self).__name__)
        self._key: str | None = api_key

        self._base_url: str = base_url
        self._secret: str | None = api_secret
        self._rsa_private_key: str | None = rsa_private_key
        self._ed25519_private_key: bytes | None = None
        if ed25519_private_key:
            # Strip PEM headers/footers if present, then decode base64
            key_data = "".join(
                line for line in ed25519_private_key.splitlines() if not line.startswith("-----")
            )
            key_bytes = base64.b64decode(key_data)
            # Extract 32-byte seed (works for both raw and PKCS#8 DER format)
            self._ed25519_private_key = key_bytes[-32:]

        self._headers: dict[str, Any] = {
            "Content-Type": "application/json",
            "User-Agent": nautilus_trader.NAUTILUS_USER_AGENT,
        }
        if api_key:
            self._headers["X-MBX-APIKEY"] = api_key
        self._client = HttpClient(
            keyed_quotas=ratelimiter_quotas or [],
            default_quota=ratelimiter_default_quota,
            proxy_url=proxy_url,
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
    def api_key(self) -> str | None:
        """
        Return the Binance API key being used by the client.

        Returns
        -------
        str or None

        """
        return self._key

    @property
    def api_key_masked(self) -> str:
        """
        Return the masked Binance API key being used by the client.

        Shows first 4 and last 4 characters with ellipsis in between.
        For keys shorter than 8 characters, shows asterisks only.

        Returns
        -------
        str

        """
        if self._key is None:
            return ""
        return mask_api_key(self._key)

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
        if self._secret is None:
            raise ValueError("Cannot sign request: api_secret not configured")

        if self._ed25519_private_key is not None:
            return ed25519_signature(self._ed25519_private_key, data)
        if self._rsa_private_key is not None:
            return rsa_signature(self._rsa_private_key, data)
        return hmac_signature(self._secret, data)

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
