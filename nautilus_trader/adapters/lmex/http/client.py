# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter
#  Licensed under the GNU Lesser General Public License Version 3.0
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import asyncio
import hashlib
import hmac
import json
import os
import time
from typing import Any

import msgspec

import nautilus_trader
from nautilus_trader.adapters.lmex.http.error import LmexClientError
from nautilus_trader.adapters.lmex.http.error import LmexServerError
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.nautilus_pyo3 import HttpClient
from nautilus_trader.core.nautilus_pyo3 import HttpMethod
from nautilus_trader.core.nautilus_pyo3 import HttpResponse
from nautilus_trader.core.nautilus_pyo3 import Quota


class LmexHttpClient:
    """
    Provides an asynchronous HTTP client for the LMEX REST API.

    Handles request signing, nonce generation, and error parsing.  The
    underlying transport is ``nautilus_pyo3.HttpClient`` which provides
    built-in connection pooling and optional rate-limiting.

    Parameters
    ----------
    clock : LiveClock
        The clock used for timestamping nonces.
    api_key : str or None
        The LMEX API public key.  If ``None`` the client operates in
        read-only (unauthenticated) mode.
    api_secret : str or None
        The LMEX API secret used for HMAC-SHA384 request signing.
    base_url : str
        The base URL for all REST requests
        (e.g. ``"https://api.lmex.io/spot"``).
    ratelimiter_default_quota : Quota or None, optional
        Default rate-limiter quota for the underlying HTTP engine.
    proxy_url : str or None, optional
        Optional HTTP proxy URL.

    Notes
    -----
    LMEX authentication uses HMAC-SHA384 (not SHA-256).  The signature is
    computed as::

        HMAC_SHA384(api_secret, url_path + nonce + body)

    where ``url_path`` is the path component only (no query string),
    ``nonce`` is the current epoch in milliseconds as a string, and
    ``body`` is the raw JSON request body (empty string for GET/DELETE).

    """

    def __init__(
        self,
        clock: LiveClock,
        api_key: str | None,
        api_secret: str | None,
        base_url: str,
        ratelimiter_default_quota: Quota | None = None,
        proxy_url: str | None = None,
    ) -> None:
        self._clock = clock
        self._log: Logger = Logger(type(self).__name__)
        self._key: str | None = api_key
        self._secret: str | None = api_secret
        self._base_url: str = base_url.rstrip("/")

        self._headers: dict[str, str] = {
            "Content-Type": "application/json",
            "User-Agent": nautilus_trader.NAUTILUS_USER_AGENT,
        }

        self._http = HttpClient(
            keyed_quotas=[],
            default_quota=ratelimiter_default_quota,
            proxy_url=proxy_url,
        )

    # ------------------------------------------------------------------
    # Properties
    # ------------------------------------------------------------------

    @property
    def base_url(self) -> str:
        """Return the REST base URL being used by this client."""
        return self._base_url

    @property
    def api_key(self) -> str | None:
        """Return the API key (may be ``None`` for unauthenticated clients)."""
        return self._key

    @property
    def api_key_masked(self) -> str:
        """
        Return a partially-redacted API key suitable for logging.

        Shows the first 4 and last 4 characters; returns ``"<none>"`` if no
        key is configured.
        """
        if not self._key:
            return "<none>"
        if len(self._key) <= 8:
            return "****"
        return f"{self._key[:4]}...{self._key[-4:]}"

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    def _nonce(self) -> str:
        """Return the current epoch milliseconds as a string (the LMEX nonce)."""
        return str(int(time.time() * 1_000))

    def _sign(self, path: str, nonce: str, body: str) -> str:
        """
        Compute the LMEX HMAC-SHA384 request signature.

        Parameters
        ----------
        path : str
            The URL path component only (e.g. ``"/spot/api/v3.2/order"``).
            Must **not** include query parameters.
        nonce : str
            The request nonce (epoch milliseconds string).
        body : str
            The raw JSON request body; empty string ``""`` for GET/DELETE.

        Returns
        -------
        str
            Lowercase hex-encoded HMAC-SHA384 digest.

        Raises
        ------
        ValueError
            If ``api_secret`` is not configured.

        """
        if self._secret is None:
            raise ValueError("Cannot sign request: api_secret is not configured")

        message = path + nonce + body
        digest = hmac.new(
            self._secret.encode("utf-8"),
            message.encode("utf-8"),
            hashlib.sha384,
        ).hexdigest()
        return digest

    def _auth_headers(self, path: str, nonce: str, body: str) -> dict[str, str]:
        """
        Build the three LMEX authentication headers for a signed request.

        Parameters
        ----------
        path : str
            URL path (no query string).
        nonce : str
            Epoch milliseconds string.
        body : str
            Raw JSON body (or empty string).

        Returns
        -------
        dict[str, str]

        Raises
        ------
        ValueError
            If ``api_key`` or ``api_secret`` is not configured.

        """
        if self._key is None:
            raise ValueError("Cannot add auth headers: api_key is not configured")
        return {
            "request-api": self._key,
            "request-nonce": nonce,
            "request-sign": self._sign(path, nonce, body),
        }

    # ------------------------------------------------------------------
    # Public interface
    # ------------------------------------------------------------------

    async def send_request(
        self,
        http_method: HttpMethod,
        path: str,
        params: dict[str, Any] | None = None,
        payload: dict[str, Any] | None = None,
        signed: bool = False,
    ) -> bytes:
        """
        Send an HTTP request to the LMEX REST API.

        Parameters
        ----------
        http_method : HttpMethod
            The HTTP verb (GET, POST, DELETE …).
        path : str
            The API path starting with ``/api/v3.2/…``
            (e.g. ``"/api/v3.2/orderbook"``).
        params : dict, optional
            Query-string parameters appended to the URL for GET requests.
        payload : dict, optional
            JSON body parameters for POST/DELETE requests.
        signed : bool, default False
            When ``True`` authentication headers are added to the request.

        Returns
        -------
        bytes
            The raw response body.

        Raises
        ------
        LmexClientError
            On 4xx HTTP responses.
        LmexServerError
            On 5xx HTTP responses.

        """
        # Build query string
        url = self._base_url + path
        if params:
            qs = "&".join(f"{k}={v}" for k, v in params.items() if v is not None)
            url = f"{url}?{qs}"

        # Serialize body
        body_str: str = json.dumps(payload, separators=(",", ":")) if payload else ""
        body_bytes: bytes | None = body_str.encode("utf-8") if body_str else None

        # Build headers
        headers = dict(self._headers)
        if signed:
            nonce = self._nonce()
            headers.update(self._auth_headers(path, nonce, body_str))

        self._log.debug(f"→ {http_method.name} {url}", LogColor.MAGENTA)

        response: HttpResponse = await self._http.request(
            http_method,
            url=url,
            headers=headers,
            body=body_bytes,
        )

        if response.status >= 400:
            body_content = response.body
            try:
                message: Any = msgspec.json.decode(body_content) if body_content else None
            except (msgspec.DecodeError, Exception):
                message = body_content.decode("utf-8", errors="replace") if body_content else None

            resp_headers: dict[str, str] = dict(response.headers) if response.headers else {}

            if response.status >= 500:
                raise LmexServerError(
                    status=response.status,
                    message=message,
                    headers=resp_headers,
                )
            raise LmexClientError(
                status=response.status,
                message=message,
                headers=resp_headers,
            )

        self._log.debug(f"← HTTP {response.status} ({len(response.body)} bytes)", LogColor.MAGENTA)
        return response.body

    async def get(
        self,
        path: str,
        params: dict[str, Any] | None = None,
        signed: bool = False,
    ) -> bytes:
        """
        Send an authenticated or public GET request.

        Parameters
        ----------
        path : str
            API path (e.g. ``"/api/v3.2/orderbook"``).
        params : dict, optional
            Query-string parameters.
        signed : bool, default False
            Attach authentication headers when ``True``.

        Returns
        -------
        bytes

        """
        return await self.send_request(
            HttpMethod.GET,
            path=path,
            params=params,
            signed=signed,
        )

    async def post(
        self,
        path: str,
        payload: dict[str, Any] | None = None,
    ) -> bytes:
        """
        Send a signed POST request.

        Parameters
        ----------
        path : str
            API path (e.g. ``"/api/v3.2/order"``).
        payload : dict, optional
            JSON request body.

        Returns
        -------
        bytes

        """
        return await self.send_request(
            HttpMethod.POST,
            path=path,
            payload=payload,
            signed=True,
        )

    async def delete(
        self,
        path: str,
        params: dict[str, Any] | None = None,
        payload: dict[str, Any] | None = None,
    ) -> bytes:
        """
        Send a signed DELETE request.

        Parameters
        ----------
        path : str
            API path (e.g. ``"/api/v3.2/order"``).
        params : dict, optional
            Query-string parameters.  LMEX cancel endpoints use query params,
            not a JSON body.
        payload : dict, optional
            JSON request body (unused by most LMEX endpoints; kept for
            future compatibility).

        Returns
        -------
        bytes

        """
        return await self.send_request(
            HttpMethod.DELETE,
            path=path,
            params=params,
            payload=payload,
            signed=True,
        )
