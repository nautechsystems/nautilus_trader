import asyncio

import aiohttp


ONE_DAY = 86_400


class HTTPClient:
    def __init__(self, logger, loop=None, ttl_dns_cache=ONE_DAY):
        self._log = logger
        self._loop = loop or asyncio.get_event_loop()
        self.ttl_dns_cache = ttl_dns_cache
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
