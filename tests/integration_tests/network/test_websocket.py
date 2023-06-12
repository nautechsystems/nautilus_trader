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

import pytest

from nautilus_trader.core.nautilus_pyo3.network import WebSocketClient


def _server_url(server) -> str:
    return f"ws://{server.host}:{server.port}/ws"


@pytest.mark.asyncio()
async def test_connect_and_disconnect(websocket_server):
    # Arrange
    store = []

    # Act, Assert
    client = await WebSocketClient.connect(
        url=_server_url(websocket_server),
        handler=store.append,
    )
    assert client.is_connected
    await client.disconnect()
    assert not client.is_connected


@pytest.mark.asyncio()
async def test_client_recv(websocket_server):
    # Arrange
    store = []
    client = await WebSocketClient.connect(
        url=_server_url(websocket_server),
        handler=store.append,
    )

    # Act
    num_messages = 3
    for _ in range(num_messages):
        await client.send(b"Hello")
    await asyncio.sleep(0.1)
    await client.disconnect()

    expected = [b"connected"] + [b"Hello-response"] * 3
    assert store == expected
    assert not client.is_connected

    # @pytest.mark.asyncio()
    # async def test_reconnect_after_close(self, websocket_server):
    #     # Arrange
    #     await self.client.connect(ws_url=self._server_url(websocket_server))
    #     await self.client.send(b"close")
    #     await asyncio.sleep(0.1)
    #
    #     # Act
    #     await self.client.receive()
    #     await asyncio.sleep(0.1)
    #     await self.client.receive()
    #
    #     # Assert
    #     assert self.messages == [b"connected"] * 2
    #
    # @pytest.mark.asyncio()
    # async def test_reconnect_after_disconnect(self, websocket_server):
    #     # Arrange
    #     await self.client.connect(ws_url=self._server_url(websocket_server))
    #     await self.client.disconnect()
    #     await asyncio.sleep(0.1)
    #     await self.client.reconnect()
    #
    #     # Act
    #     await asyncio.sleep(0.1)
    #     await self.client.receive()
    #
    #     # Assert
    #     assert self.messages == [b"connected"]
    #
    # @pytest.mark.asyncio()
    # async def test_exponential_backoff(self, websocket_server):
    #     # Arrange
    #     await self.client.connect(ws_url=self._server_url(websocket_server))
    #
    #     # Act
    #     for _ in range(2):
    #         await self.client.send(b"close")
    #         await asyncio.sleep(0.1)
    #         await self.client.receive()
    #         await asyncio.sleep(0.1)
    #         await self.client.receive()
    #
    #     assert self.client.connection_retry_count == 2
