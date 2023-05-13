# -------------------------------------------------------------------------------------------------G
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

from collections.abc import Awaitable
from collections.abc import Coroutine

import pytest
from aiohttp import web
from aiohttp.test_utils import TestServer

from nautilus_trader.core.nautilus_pyo3.network import HttpClient
from nautilus_trader.core.nautilus_pyo3.network import HttpResponse


@pytest.mark.asyncio()
async def test_client_get_github() -> None:
    # Arrange
    client = HttpClient()
    url = "https://github.com"

    # Act
    resp: HttpResponse = await client.get(url, headers={})

    # Assert
    assert resp.status == 200
    assert len(resp.body) > 0


@pytest.fixture(name="test_server")
async def fixture_test_server(aiohttp_server) -> Awaitable[TestServer]:
    async def hello(request):
        return web.Response(text="Hello, world")

    app = web.Application()
    app.router.add_route("GET", "/get", hello)
    app.router.add_route("POST", "/post", hello)

    server = await aiohttp_server(app)
    return server


@pytest.mark.skip(reason="WIP")
@pytest.mark.asyncio()
async def test_client_get(test_server: Coroutine) -> None:
    # Arrange
    server: TestServer = await test_server
    client = HttpClient()
    url = f"http://{server.host}:{server.port}/get"

    # Act
    resp: HttpResponse = await client.get(url, headers={})

    # Assert
    assert resp.status == 200
    assert len(resp) > 0


@pytest.mark.skip(reason="WIP")
@pytest.mark.asyncio()
async def test_client_post(test_server: Coroutine) -> None:
    # Arrange
    server: TestServer = await test_server
    client = HttpClient()
    url = f"http://{server.host}:{server.port}/post"

    # Act
    resp: HttpResponse = await client.post(url, headers={})

    # Assert
    assert resp.status == 200
    assert len(resp) > 0
