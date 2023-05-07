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

from nautilus_trader.network.websocket import WebSocketClient
from nautilus_trader.test_kit.stubs.component import TestComponentStubs


class TestWebsocketClient:
    def setup(self):
        self.messages = []

        def record(data: bytes):
            self.messages.append(data)

        self.client = WebSocketClient(
            loop=asyncio.get_event_loop(),
            logger=TestComponentStubs.logger(level="DEBUG"),
            handler=record,
            max_retry_connection=6,
            log_send=True,
            log_recv=True,
        )

    @staticmethod
    def _server_url(server) -> str:
        return f"http://{server.host}:{server.port}/ws"

    @pytest.mark.asyncio
    async def test_connect(self, websocket_server):
        await self.client.connect(ws_url=self._server_url(websocket_server))
        assert self.client.is_connected
        await self.client.disconnect()

    @pytest.mark.asyncio
    async def test_client_recv(self, websocket_server):
        num_messages = 3
        await self.client.connect(ws_url=self._server_url(websocket_server))
        for _ in range(num_messages):
            await self.client.send(b"Hello")
        await asyncio.sleep(0.1)
        await self.client.close()

        expected = [b"connected"] + [b"Hello-response"] * 3
        assert self.messages == expected

    @pytest.mark.asyncio
    async def test_reconnect_after_close(self, websocket_server):
        # Arrange
        await self.client.connect(ws_url=self._server_url(websocket_server))
        await self.client.send(b"close")
        await asyncio.sleep(0.1)

        # Act
        await self.client.receive()
        await asyncio.sleep(0.1)
        await self.client.receive()

        # Assert
        assert self.messages == [b"connected"] * 2

    @pytest.mark.asyncio
    async def test_reconnect_after_disconnect(self, websocket_server):
        # Arrange
        await self.client.connect(ws_url=self._server_url(websocket_server))
        await self.client.disconnect()
        await asyncio.sleep(0.1)
        await self.client.reconnect()

        # Act
        await asyncio.sleep(0.1)
        await self.client.receive()

        # Assert
        assert self.messages == [b"connected"]

    @pytest.mark.asyncio
    async def test_exponential_backoff(self, websocket_server):
        # Arrange
        await self.client.connect(ws_url=self._server_url(websocket_server))

        # Act
        for _ in range(2):
            await self.client.send(b"close")
            await asyncio.sleep(0.1)
            await self.client.receive()
            await asyncio.sleep(0.1)
            await self.client.receive()

        assert self.client.connection_retry_count == 2
