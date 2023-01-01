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

import asyncio
import socket
import urllib.parse
from collections import deque
from ssl import SSLContext
from typing import Any, Optional, Union

from libc.stdint cimport uint64_t

import aiohttp
import cython
from aiohttp import ClientResponse
from aiohttp import ClientSession
from aiohttp import Fingerprint

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.core cimport unix_timestamp_ns


cdef class HttpClient:
    """
    Provides an asynchronous HTTP client.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    logger : Logger
        The logger for the client.
    ttl_dns_cache : int
        The time to live for the DNS cache.
    ssl: Union[None, bool, Fingerprint, SSLContext], default False
        The ssl context to use for HTTPS.
    connector_kwargs : dict, optional
        The connector key word arguments.
    latency_qsize : int, default 1000
        The maxlen for the internal latencies deque.

    Raises
    ------
    ValueError
        If `ttl_dns_cache` is not positive (> 0).
    ValueError
        If `latency_qsize` is not position (> 0).
    """

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        Logger logger not None,
        list addresses = None,
        list nameservers = None,
        int ttl_dns_cache = 86_400,  # Seconds in day
        ssl_context: Optional[SSLContext] = None,
        ssl: Optional[Union[bool, Fingerprint, SSLContext]] = None,
        dict connector_kwargs = None,
        int latency_qsize = 1000,
    ):
        Condition.positive(ttl_dns_cache, "ttl_dns_cache")
        Condition.positive_int(latency_qsize, "latency_qsize")

        self._loop = loop
        self._log = LoggerAdapter(
            component_name=type(self).__name__,
            logger=logger,
        )
        self._addresses = addresses or ['0.0.0.0']
        self._nameservers = nameservers or ['8.8.8.8', '8.8.4.4']
        self._ssl_context = ssl_context
        self._ssl = ssl
        self._ttl_dns_cache = ttl_dns_cache
        self._connector_kwargs = connector_kwargs or {}
        self._sessions: list[ClientSession] = []
        self._sessions_idx = 0
        self._sessions_len = 0
        self._latencies = deque(maxlen=latency_qsize)

    @property
    def connected(self) -> bool:
        """
        Return whether the HTTP client is connected.

        Returns
        -------
        bool

        """
        return len(self._sessions) > 0

    @property
    def session(self) -> ClientSession:
        """
        Return the current HTTP client session.

        Returns
        -------
        aiohttp.ClientSession

        """
        return self._get_session()

    cpdef uint64_t min_latency(self) except *:
        """
        Return the minimum round-trip latency (nanoseconds) for this client.

        Many factors will affect latency including which endpoints are hit and
        server side processing time. If no latencies recorded yet, then will
        return zero.

        Returns
        -------
        uint64_t

        """
        if not self._latencies:
            return 0  # Protect divide by zero

        # Could use a heap here, but we don't need to be too optimal yet
        return sorted(self._latencies)[0]

    cpdef uint64_t max_latency(self) except *:
        """
        Return the maximum round-trip latency (nanoseconds) for this client.

        Many factors will affect latency including which endpoints are hit and
        server side processing time. If no latencies recorded yet, then will
        return zero.

        Returns
        -------
        uint64_t

        """
        if not self._latencies:
            return 0  # Protect divide by zero

        # Could use a heap here, but we don't need to be too optimal yet
        return sorted(self._latencies)[-1]

    cpdef uint64_t avg_latency(self) except *:
        """
        Return the average round-trip latency (nanoseconds) for this client.

        Many factors will affect latency including which endpoints are hit and
        server side processing time. If no latencies recorded yet, then will
        return zero.

        Returns
        -------
        uint64_t

        """
        if not self._latencies:
            return 0  # Protect divide by zero

        return sum(self._latencies) / len(self._latencies)

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cdef object _get_session(self):
        if not self._sessions:
            raise RuntimeError("no sessions, need to connect?")
        # Circular buffer
        if self._sessions_idx >= self._sessions_len:
            self._sessions_idx = 0
        cdef int idx = self._sessions_idx
        self._sessions_idx += 1
        return self._sessions[idx]

    cpdef str _prepare_params(self, dict params):
        # Encode a dict into a URL query string
        return urllib.parse.urlencode(params)

    async def connect(self) -> None:
        """
        Connect the HTTP client session.

        """
        self._log.debug("Connecting sessions...")
        self._sessions = [aiohttp.ClientSession(
            connector=aiohttp.TCPConnector(
                limit=0,
                resolver=aiohttp.AsyncResolver(
                    nameservers=self._nameservers
                ),
                local_addr=(address, 0),
                ttl_dns_cache=self._ttl_dns_cache,
                family=socket.AF_INET,
                ssl=self._ssl if self._ssl_context is None else None,  # if ssl_context set, ssl must be None
                ssl_context=self._ssl_context,
                **self._connector_kwargs
            ),
            loop=self._loop,
        ) for address in self._addresses
        ]
        self._sessions_len = len(self._sessions)
        self._sessions_idx = 0
        self._log.debug(f"Connected sessions: {self._sessions}.")

    async def disconnect(self) -> None:
        """
        Disconnect the HTTP client session.

        """
        for session in self._sessions:
            self._log.debug(f"Closing session: {session}...")
            await session.close()
            self._log.debug(f"Session closed.")

    async def request(
        self,
        method: str,
        url: str,
        headers: Optional[dict[str, str]] = None,
        json: Optional[dict[str, Any]] = None,
        **kwargs,
    ) -> ClientResponse:
        session: ClientSession = self._get_session()
        if session.closed:
            self._log.warning("Session closed: getting next session.")
            session = self._get_session()
            if session.closed:
                self._log.warning("Session closed: reconnecting...")
                await self.connect()
                session = self._get_session()
                if session.closed:
                    self._log.error("Cannot connect a session.")
                    return
        cdef uint64_t ts_sent = unix_timestamp_ns()
        cdef uint64_t ts_recv
        async with session.request(
            method=method,
            url=url,
            headers=headers,
            json=json,
            **kwargs
        ) as resp:
            ts_recv = unix_timestamp_ns()
            self._latencies.appendleft(ts_recv - ts_sent)
            if resp.status >= 400:
                # reason should always be not None for a started response
                assert resp.reason is not None
                error = aiohttp.ClientResponseError(
                    resp.request_info,
                    resp.history,
                    status=resp.status,
                    message=resp.reason,
                    headers=resp.headers,
                )
                try:
                    error.json = await resp.json()
                except aiohttp.ContentTypeError:
                    self._log.debug("Could not parse any JSON error body.")

                resp.release()
                raise error
            resp.data = await resp.read()
            return resp

    async def get(
        self,
        url: str,
        headers: Optional[dict[str, str]] = None,
        **kwargs,
    ) -> ClientResponse:
        return await self.request(
            method="GET",
            url=url,
            headers=headers,
            **kwargs,
        )

    async def post(
        self,
        url: str,
        headers: Optional[dict[str, str]] = None,
        **kwargs,
    ) -> ClientResponse:
        return await self.request(
            method="POST",
            url=url,
            headers=headers,
            **kwargs,
        )

    async def delete(
        self,
        url: str,
        headers: Optional[dict[str, str]] = None,
        **kwargs,
    ) -> ClientResponse:
        return await self.request(
            method="DELETE",
            url=url,
            headers=headers,
            **kwargs,
        )
