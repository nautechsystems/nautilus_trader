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
"""
Provides a dYdX asynchronous HTTP client.
"""

import asyncio
import urllib.parse
from random import randint
from typing import Any

import msgspec

import nautilus_trader
from nautilus_trader.adapters.dydx.http.errors import DYDXError
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.nautilus_pyo3 import HttpClient
from nautilus_trader.core.nautilus_pyo3 import HttpError
from nautilus_trader.core.nautilus_pyo3 import HttpMethod
from nautilus_trader.core.nautilus_pyo3 import HttpResponse
from nautilus_trader.core.nautilus_pyo3 import HttpTimeoutError
from nautilus_trader.core.nautilus_pyo3 import Quota


INTERNAL_SERVER_ERROR_CODE = 500
BAD_REQUEST_ERROR_CODE = 400


class DYDXHttpClient:
    """
    Provide a dYdX asynchronous HTTP client.

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
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
        base_url: str,
        ratelimiter_quotas: list[tuple[str, Quota]] | None = None,
        ratelimiter_default_quota: Quota | None = None,
    ) -> None:
        """
        Provide a dYdX asynchronous HTTP client.
        """
        self._clock: LiveClock = clock
        self._log: Logger = Logger(type(self).__name__)
        self._base_url: str = base_url
        self._headers: dict[str, Any] = {
            "Content-Type": "application/json",
            "User-Agent": nautilus_trader.USER_AGENT,
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
    def headers(self) -> dict[str, Any]:
        """
        Return the headers being used by the client.

        Returns
        -------
        str

        """
        return self._headers

    def _urlencode(self, payload: dict[str, Any]) -> str:
        # Booleans are capitalized (True/False) when directly passed to `urlencode`
        payload_list = [
            (key, str(values).lower() if isinstance(values, bool) else values)
            for key, values in payload.items()
        ]

        return urllib.parse.urlencode(payload_list)

    async def send_request(
        self,
        http_method: HttpMethod,
        url_path: str,
        payload: dict[str, Any] | None = None,
        ratelimiter_keys: list[str] | None = None,
        max_tries: int = 5,
        initial_sleep_duration_ms: int = 1000,
        max_sleep_ms: int = 30_000,
        timeout_secs: int = 10,
    ) -> bytes | None:
        """
        Asynchronously send an HTTP request.

        Retries the HTTP request if the call times out, and uses exponential backoff.

        """
        if payload:
            url_path += "?" + self._urlencode(payload)
            payload = None  # Don't send payload in the body

        self._log.debug(f"{self._base_url + url_path}", LogColor.MAGENTA)
        done = False
        sleep_duration_ms = initial_sleep_duration_ms

        for retry_counter in range(max_tries):
            if not done:
                try:
                    response: HttpResponse = await self._client.request(
                        http_method,
                        url=self._base_url + url_path,
                        headers=self._headers,
                        body=msgspec.json.encode(payload) if payload else None,
                        keys=ratelimiter_keys,
                        timeout_secs=timeout_secs,
                    )
                    done = True
                except HttpTimeoutError as e:
                    if retry_counter < max_tries - 1:
                        sleep_duration_ms = randint(  # noqa: S311
                            initial_sleep_duration_ms,
                            min(max_sleep_ms, initial_sleep_duration_ms * 2**retry_counter),
                        )
                        sleep_duration_secs = sleep_duration_ms / 1_000
                        self._log.warning(
                            f"Failed to perform HTTP request: {e}. Retry {retry_counter + 1}/{max_tries}. Sleep {sleep_duration_secs:0.1f}s",
                        )
                        await asyncio.sleep(sleep_duration_secs)
                    else:
                        self._log.error(f"Failed to perform HTTP request: {e}")
                        raise
                except HttpError as e:
                    self._log.error(f"Failed to perform HTTP request: {e}")
                    raise

        if BAD_REQUEST_ERROR_CODE <= response.status < INTERNAL_SERVER_ERROR_CODE:
            raise DYDXError(
                status=response.status,
                message=msgspec.json.decode(response.body) if response.body else None,
                headers=response.headers,
            )

        if response.status >= INTERNAL_SERVER_ERROR_CODE:
            raise DYDXError(
                status=response.status,
                message=msgspec.json.decode(response.body) if response.body else None,
                headers=response.headers,
            )

        return response.body
