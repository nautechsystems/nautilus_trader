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

import msgspec
import pytest

from nautilus_trader.core.nautilus_pyo3.network import SocketClient
from nautilus_trader.core.nautilus_pyo3.network import SocketConfig
from nautilus_trader.test_kit.functions import eventually


def _server_url(host, port) -> str:
    return f"tcp://{host}:{port}"


def _config(socket_server, handler):
    host, port = socket_server
    return SocketConfig(
        url=_server_url(host, port),
        handler=handler,
        ssl=True,
        suffix=b"\r\n",
    )


@pytest.mark.asyncio()
async def test_connect_and_disconnect(socket_server):
    # Arrange
    store = []

    client = await SocketClient.connect(_config(socket_server, store.append))

    # Act, Assert
    await eventually(lambda: client.is_alive)
    await client.disconnect()
    await eventually(lambda: not client.is_alive)


@pytest.mark.asyncio()
async def test_client_send_recv(socket_server):
    # Arrange
    store = []
    client = await SocketClient.connect(_config(socket_server, store.append))

    await eventually(lambda: client.is_alive)

    # Act
    num_messages = 3
    for _ in range(num_messages):
        await client.send(b"Hello")
    await asyncio.sleep(0.1)
    await client.disconnect()

    await eventually(lambda: store == [b"connected"] + [b"Hello-response"] * 3)
    await client.disconnect()
    await eventually(lambda: not client.is_alive)


@pytest.mark.asyncio()
async def test_client_send_recv_json(socket_server):
    # Arrange
    store = []
    client = await SocketClient.connect(_config(socket_server, store.append))

    await eventually(lambda: client.is_alive)

    # Act
    num_messages = 3
    for _ in range(num_messages):
        await client.send(msgspec.json.encode({"method": "SUBSCRIBE"}))
    await asyncio.sleep(0.3)
    await client.disconnect()

    expected = [b"connected"] + [b'{"method":"SUBSCRIBE"}-response'] * 3
    assert store == expected
    await client.disconnect()
    await eventually(lambda: not client.is_alive)


@pytest.mark.asyncio()
async def test_reconnect_after_close(socket_server):
    # Arrange
    store = []
    client = await SocketClient.connect(_config(socket_server, store.append))

    await eventually(lambda: client.is_alive)

    # Act
    await client.send(b"close")

    # Assert
    await eventually(lambda: store == [b"connected"] * 2)


# @pytest.mark.asyncio()
# async def test_exponential_backoff(self, websocket_server):
#     # Arrange
#     store = []
#     client = await WebSocketClient.connect(
#         url=_server_url(websocket_server),
#         handler=store.append,
#     )
#
#     # Act
#     for _ in range(2):
#         await self.client.send(b"close")
#         await asyncio.sleep(0.1)
#
#     assert client.connection_retry_count == 2
