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

from nautilus_trader.common.enums import LogLevel
from nautilus_trader.network.socket_pyo3 import SocketClient
from nautilus_trader.test_kit.functions import eventually
from nautilus_trader.test_kit.stubs.component import TestComponentStubs


@pytest.mark.asyncio()
async def test_socket_base_connect(socket_server):
    messages = []

    def handler(raw):
        messages.append(raw)
        if len(messages) > 5:
            client.stop()

    host, port = socket_server
    client = SocketClient(
        logger=TestComponentStubs.logger(),
        host=host,
        port=port,
        handler=handler,
        ssl=False,
    )
    await client.connect()
    await eventually(lambda: messages == [b"hello"] * 6)


@pytest.mark.asyncio()
async def test_socket_base_reconnect_on_incomplete_read(closing_socket_server):
    messages = []

    def handler(raw):
        messages.append(raw)

    host, port = closing_socket_server
    client = SocketClient(
        logger=TestComponentStubs.logger(level=LogLevel.DEBUG),
        host=host,
        port=port,
        handler=handler,
        ssl=False,
    )
    # mock_post_conn = mock.patch.object(client, "post_connection")
    await client.connect()
    await asyncio.sleep(0.1)
    await eventually(lambda: messages == [b"hello"] * 1)

    # Reconnect and receive another message
    await asyncio.sleep(1)
    # assert client._connection_retry_count >= 1
