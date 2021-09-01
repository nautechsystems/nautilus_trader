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
# -------------------------------------------------------------------------------------------------

import asyncio

import pytest

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger


async def handle_echo(reader: asyncio.StreamReader, writer: asyncio.StreamWriter):
    async def write():
        while True:
            writer.write(b"hello\r\n")
            await asyncio.sleep(0.1)

    asyncio.get_event_loop().create_task(write())

    while True:
        req = await reader.readline()
        if req == b"CLOSE_STREAM":
            writer.close()


@pytest.fixture()
async def socket_server():
    server = await asyncio.start_server(handle_echo, "127.0.0.1", 0)
    addr = server.sockets[0].getsockname()
    async with server:
        await server.start_serving()
        yield addr


@pytest.fixture()
def logger(event_loop):
    clock = LiveClock()
    return LiveLogger(loop=event_loop, clock=clock)
