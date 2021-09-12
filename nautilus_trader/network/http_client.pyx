# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
import itertools
import socket
from typing import Dict, List, Union

import aiohttp
from aiohttp import ClientResponse
from aiohttp import ClientResponseError
from aiohttp import ClientSession

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition


cdef int ONE_DAY = 86_400


cdef class HTTPClient:
    """
    Provides a low-level HTTP2 client.
    """

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        Logger logger not None,
        list addresses=None,
        list nameservers=None,
        int ttl_dns_cache=ONE_DAY,
        ssl=False,
        dict connector_kwargs=None,
    ):
        """
        Initialize a new instance of the ``HTTPClient`` class.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        logger : Logger
            The logger for the client.
        ttl_dns_cache : int
            The time to live for the DNS cache.
        ssl: SSL Context, default=False
            The ssl context to use for HTTPS.
        connector_kwargs : dict, optional
            The connector key word arguments.

        Raises
        ------
        ValueError
            If ttl_dns_cache is not positive.

        """
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

    @property
    def session(self) -> ClientSession:
        assert self._sessions, "No sessions, need to connect?"
        session = next(self._sessions)  # type: ClientSession
        return session

    async def connect(self):
        self._log.debug("Connecting sessions")
        sessions = [aiohttp.ClientSession(
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
            )) for address in self._addresses
        ]
        self._sessions = itertools.cycle(sessions)
        self._log.debug(f"Connected sessions: {sessions}")

    async def disconnect(self):
        for session in self._sessions:
            self._log.debug(f"Closing session: {session}")
            await session.close()

    async def request(self, method, url, headers=None, json=None, **kwargs) -> Union[bytes, Dict]:
        # self._log.debug(f"Request: {method=}, {url=}, {headers=}, {json=}, {kwargs if kwargs else ''}")
        session = self.session
        if session.closed:
            self._log.warning("Session closed! reconnecting")
            await self.connect()
        async with session.request(
            method=method,
            url=url,
            headers=headers,
            json=json,
            **kwargs
        ) as resp:
            try:
                data = await resp.read()
                resp.data = data
                resp.raise_for_status()
                # self._log.debug(str(data))
                return resp
            except ClientResponseError as e:
                self._log.exception(e)
                raise ResponseException(resp=resp, client_response_error=e)

    async def get(self, url, **kwargs):
        return await self.request(method="GET", url=url, **kwargs)

    async def post(self, url, **kwargs):
        return await self.request(method="POST", url=url, **kwargs)

    # TODO more convenience methods?


class ResponseException(BaseException):
    def __init__(self, resp: ClientResponse, client_response_error: ClientResponseError):
        super().__init__()
        self.resp = resp
        self.client_response_error = client_response_error
