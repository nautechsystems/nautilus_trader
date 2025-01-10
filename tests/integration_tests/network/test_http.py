# -------------------------------------------------------------------------------------------------G
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from collections.abc import Callable
from collections.abc import Coroutine
from typing import Any

import msgspec
import pytest
from aiohttp import web
from aiohttp.test_utils import TestServer

from nautilus_trader.core.nautilus_pyo3 import HttpClient
from nautilus_trader.core.nautilus_pyo3 import HttpMethod
from nautilus_trader.core.nautilus_pyo3 import HttpResponse


@pytest.fixture(name="test_server")
async def fixture_test_server(
    aiohttp_server: Callable[..., Coroutine[Any, Any, TestServer]],
) -> TestServer:
    async def hello(request):
        return web.Response(text="Hello, world")

    app = web.Application()
    app.router.add_route("GET", "/get", hello)
    app.router.add_route("POST", "/post", hello)
    app.router.add_route("PATCH", "/patch", hello)
    app.router.add_route("DELETE", "/delete", hello)

    server = await aiohttp_server(app)
    return server


@pytest.mark.asyncio()
async def test_client_get(test_server: Coroutine) -> None:
    # Arrange
    server: TestServer = await test_server
    client = HttpClient()
    url = f"http://{server.host}:{server.port}/get"

    # Act
    response: HttpResponse = await client.request(HttpMethod.GET, url, headers={})

    # Assert
    assert response.status == 200
    assert len(response.body) > 0


@pytest.mark.asyncio()
async def test_client_post(test_server: Coroutine) -> None:
    # Arrange
    server: TestServer = await test_server
    client = HttpClient()
    url = f"http://{server.host}:{server.port}/post"

    # Act
    response: HttpResponse = await client.request(HttpMethod.POST, url, headers={})

    # Assert
    assert response.status == 200
    assert len(response.body) > 0


@pytest.mark.asyncio()
async def test_client_post_with_body(test_server: Coroutine) -> None:
    # Arrange
    server: TestServer = await test_server
    client = HttpClient()
    url = f"http://{server.host}:{server.port}/post"
    body = {"key1": "value1", "key2": "value2"}
    body_bytes = msgspec.json.encode(body)

    # Act
    response: HttpResponse = await client.request(HttpMethod.POST, url, headers={}, body=body_bytes)

    # Assert
    assert response.status == 200
    assert len(response.body) > 0


@pytest.mark.asyncio()
async def test_client_patch(test_server: Coroutine) -> None:
    # Arrange
    server: TestServer = await test_server
    client = HttpClient()
    url = f"http://{server.host}:{server.port}/patch"

    # Act
    response: HttpResponse = await client.request(HttpMethod.PATCH, url, headers={})

    # Assert
    assert response.status == 200
    assert len(response.body) > 0


@pytest.mark.asyncio()
async def test_client_delete(test_server: Coroutine) -> None:
    # Arrange
    server: TestServer = await test_server
    client = HttpClient()
    url = f"http://{server.host}:{server.port}/delete"

    # Act
    response: HttpResponse = await client.request(HttpMethod.DELETE, url, headers={})

    # Assert
    assert response.status == 200
    assert len(response.body) > 0
