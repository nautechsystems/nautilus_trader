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
import socket
import urllib.parse
from ssl import SSLContext
from typing import Any, Dict, List, Optional, Union

import aiohttp
import cython
from aiohttp import ClientResponse
from aiohttp import ClientResponseError
from aiohttp import ClientSession
from aiohttp import Fingerprint

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition


# Seconds in one day
cdef int ONE_DAY = 86_400


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

    Raises
    ------
    ValueError
        If `ttl_dns_cache` is not positive (> 0).
    """

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        Logger logger not None,
        list addresses = None,
        list nameservers = None,
        int ttl_dns_cache = 86_400,
        ssl: Union[None, bool, Fingerprint, SSLContext] = False,
        dict connector_kwargs = None,
    ):
        Condition.positive(ttl_dns_cache, "ttl_dns_cache")

        self._loop = loop
        self._log = LoggerAdapter(
            component_name=type(self).__name__,
            logger=logger,
        )
        self._addresses = addresses or ['0.0.0.0']
        self._nameservers = nameservers or ['8.8.8.8', '8.8.4.4']
        self._ssl = ssl
        self._ttl_dns_cache = ttl_dns_cache
        self._connector_kwargs = connector_kwargs or {}
        self._sessions: List[ClientSession] = []
        self._sessions_idx = 0
        self._sessions_len = 0

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
                ssl=self._ssl,
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
        headers: Optional[Dict[str, str]] = None,
        json: Optional[Dict[str, Any]] = None,
        **kwargs,
    ) -> ClientResponse:
        session: ClientSession = self._get_session()
        if session.closed:
            self._log.warning("Session closed: reconnecting.")
            await self.connect()
        async with session.request(
            method=method,
            url=url,
            headers=headers,
            json=json,
            **kwargs
        ) as resp:
            if resp.status >= 400:
                # reason should always be not None for a started response
                assert resp.reason is not None
                resp.release()
                error = ClientResponseError(
                    resp.request_info,
                    resp.history,
                    status=resp.status,
                    message=resp.reason,
                    headers=resp.headers,
                )
                error.json = await resp.json()
                raise error
            resp.data = await resp.read()
            return resp

    async def get(
        self,
        url: str,
        headers: Optional[Dict[str, str]] = None,
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
        headers: Optional[Dict[str, str]] = None,
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
        headers: Optional[Dict[str, str]] = None,
        **kwargs,
    ) -> ClientResponse:
        return await self.request(
            method="DELETE",
            url=url,
            headers=headers,
            **kwargs,
        )
