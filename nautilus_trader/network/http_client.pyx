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

import aiohttp

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition


cdef int ONE_DAY = 86_400


cdef class HTTPClient:
    """
    Provides a low level HTTP2 client.
    """

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        Logger logger not None,
        int ttl_dns_cache=ONE_DAY,
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

        self._ttl_dns_cache = ttl_dns_cache
        self._session = None

    async def connect(self):
        connector = aiohttp.TCPConnector(ttl_dns_cache=300)
        self._session = aiohttp.ClientSession(loop=self._loop, connector=connector)

    async def _request(self, method, url, **kwargs) -> bytes:
        resp = await self._session._request(method=method, str_or_url=url, **kwargs)
        # TODO - Do something with status code?
        # assert resp.status
        return await resp.read()

    async def get(self, url, **kwargs):
        return await self._request(method="GET", url=url, **kwargs)

    async def post(self, url, **kwargs):
        return await self._request(method="POST", url=url, **kwargs)

    # TODO more convenience methods?
