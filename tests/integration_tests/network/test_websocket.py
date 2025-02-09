# -------------------------------------------------------------------------------------------------
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

import asyncio
import sys

import msgspec
import pytest
from aiohttp.test_utils import TestServer

from nautilus_trader.core.nautilus_pyo3 import WebSocketClient
from nautilus_trader.core.nautilus_pyo3 import WebSocketConfig
from nautilus_trader.test_kit.functions import eventually


pytestmark = pytest.mark.skipif(sys.platform != "linux", reason="Run socket tests on Linux only")


def _server_url(server: TestServer) -> str:
    return f"ws://{server.host}:{server.port}/ws"


@pytest.mark.asyncio()
async def test_connect_and_disconnect(websocket_server):
    # Arrange
    store = []
    config = WebSocketConfig(_server_url(websocket_server), store.append, [])
    client = await WebSocketClient.connect(config)

    # Act, Assert
    await eventually(lambda: client.is_active())
    await client.disconnect()
    await eventually(lambda: not client.is_active())


@pytest.mark.asyncio()
async def test_client_send_recv(websocket_server):
    # Arrange
    store = []
    config = WebSocketConfig(_server_url(websocket_server), store.append, [])
    client = await WebSocketClient.connect(config)
    await eventually(lambda: client.is_active())

    # Act
    num_messages = 3
    for _ in range(num_messages):
        await client.send(b"Hello")
    await asyncio.sleep(0.1)
    await client.disconnect()

    await eventually(lambda: store == [b"connected"] + [b"Hello-response"] * 3)
    await client.disconnect()
    await eventually(lambda: not client.is_active())


@pytest.mark.asyncio()
async def test_client_send_recv_json(websocket_server):
    # Arrange
    store = []
    config = WebSocketConfig(_server_url(websocket_server), store.append, [])
    client = await WebSocketClient.connect(config)
    await eventually(lambda: client.is_active())

    # Act
    num_messages = 3
    for _ in range(num_messages):
        await client.send(msgspec.json.encode({"method": "SUBSCRIBE"}))
    await asyncio.sleep(0.3)
    await client.disconnect()

    expected = [b"connected"] + [b'{"method":"SUBSCRIBE"}-response'] * 3
    assert store == expected
    await client.disconnect()
    await eventually(lambda: not client.is_active())


@pytest.mark.asyncio()
async def test_reconnect_after_close(websocket_server):
    # Arrange
    store = []
    config = WebSocketConfig(_server_url(websocket_server), store.append, [])
    client = await WebSocketClient.connect(config)
    await eventually(lambda: client.is_active())

    # Act
    await client.send(b"close")

    # Assert
    await eventually(lambda: store == [b"connected"] * 2)
    await client.disconnect()


@pytest.mark.asyncio()
async def test_exponential_backoff(websocket_server):
    # Arrange
    store = []
    config = WebSocketConfig(_server_url(websocket_server), store.append, [])
    client = await WebSocketClient.connect(config)
    await eventually(lambda: client.is_active())

    # Act
    await client.send(b"close")
    await eventually(lambda: len([msg for msg in store if msg == b"connected"]) == 2)

    # Assert
    assert len([msg for msg in store if msg == b"connected"]) == 2  # Initial + 1 reconnect
    await client.disconnect()


@pytest.mark.asyncio()
async def test_websocket_headers(websocket_server):
    # Arrange
    store = []
    headers = [("X-Test-Header", "test-value")]
    config = WebSocketConfig(_server_url(websocket_server), store.append, headers)
    client = await WebSocketClient.connect(config)

    # Act
    await eventually(lambda: client.is_active())
    await client.send(b"Hello")
    await asyncio.sleep(0.1)

    # Assert
    assert store == [b"connected", b"Hello-response"]
    await client.disconnect()
