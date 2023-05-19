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
from collections.abc import Coroutine

import pytest
import pytest_asyncio
from aiohttp import web
from aiohttp.test_utils import TestServer

from nautilus_trader.network.http import HttpClient
from nautilus_trader.test_kit.stubs.component import TestComponentStubs


@pytest.fixture(name="server")
async def fixture_server(aiohttp_server):
    async def hello(request):
        return web.Response(text="Hello, world")

    app = web.Application()
    app.router.add_route("GET", "/get", hello)
    app.router.add_route("POST", "/post", hello)

    server = await aiohttp_server(app)
    return server


@pytest_asyncio.fixture(name="client")
async def fixture_client() -> HttpClient:
    client = HttpClient(
        loop=asyncio.get_event_loop(),
        logger=TestComponentStubs.logger(),
    )
    await client.connect()
    return client


@pytest.mark.asyncio()
async def test_client_get(client: HttpClient, server: Coroutine) -> None:
    test_server: TestServer = await server
    url = f"http://{test_server.host}:{test_server.port}/get"
    resp = await client.get(url)
    assert resp.status == 200
    assert len(resp.data) > 0
    assert client.max_latency() > 0
    assert client.min_latency() > 0
    assert client.avg_latency() > 0


@pytest.mark.asyncio()
async def test_client_post(client: HttpClient, server: Coroutine) -> None:
    test_server = await server
    url = f"http://{test_server.host}:{test_server.port}/post"
    resp = await client.post(url)
    assert resp.status == 200
    assert len(resp.data) > 10
    assert client.max_latency() > 0
    assert client.min_latency() > 0
    assert client.avg_latency() > 0
