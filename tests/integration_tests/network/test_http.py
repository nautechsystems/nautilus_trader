# -------------------------------------------------------------------------------------------------G
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


@pytest.mark.asyncio
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


@pytest.mark.asyncio
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


@pytest.mark.asyncio
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


@pytest.mark.asyncio
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


@pytest.mark.asyncio
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


# Note: Blocking HTTP functions (http_get, http_post, http_patch, http_delete)
# are tested in Rust unit tests. They are thin wrappers that spawn threads
# with isolated runtimes and are intended for simple synchronous scripts.
