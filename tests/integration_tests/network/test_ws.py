# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
# -------------------------------------------------------------------------------------------------import asyncio

import asyncio

import pytest

from nautilus_trader.network.websocket import WebSocketClient
from tests.test_kit.stubs import TestStubs


@pytest.mark.skip(reason="WIP")
@pytest.mark.asyncio
async def test_client_recv():
    num_messages = 3
    lines = []

    def record(*args, **kwargs):
        lines.append((args, kwargs))

    client = WebSocketClient(
        ws_url="ws://echo.websocket.org",
        loop=asyncio.get_event_loop(),
        handler=record,
        logger=TestStubs.logger(),
    )
    await client.connect()
    for _ in range(num_messages):
        await client.send(b"Hello")
    await asyncio.sleep(1)
    await client.close()
    assert len(lines) == num_messages
