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
import weakref

import pytest
import pytest_asyncio
from aiohttp import WSCloseCode
from aiohttp import WSMsgType
from aiohttp import web
from aiohttp.test_utils import TestServer

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


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


@pytest_asyncio.fixture()
async def socket_server():
    server = await asyncio.start_server(handle_echo, "127.0.0.1", 0)
    addr = server.sockets[0].getsockname()
    async with server:
        await server.start_serving()
        yield addr


@pytest_asyncio.fixture()
async def closing_socket_server():
    async def handler(reader: asyncio.StreamReader, writer: asyncio.StreamWriter):
        async def write():
            while True:
                writer.write(b"hello\r\n")
                await asyncio.sleep(0.1)
                writer.close()

        asyncio.get_event_loop().create_task(write())

    server = await asyncio.start_server(handler, "127.0.0.1", 0)
    addr = server.sockets[0].getsockname()
    async with server:
        await server.start_serving()
        yield addr


@pytest_asyncio.fixture()
@pytest.mark.asyncio()
async def websocket_server(event_loop):
    async def handler(request):
        ws = web.WebSocketResponse()
        await ws.prepare(request)
        request.app["websockets"].add(ws)

        await ws.send_bytes(b"connected")

        async for msg in ws:
            if msg.type == WSMsgType.BINARY:
                if msg.data == b"close":
                    await ws.close(code=257)
                else:
                    await ws.send_bytes(msg.data + b"-response")
        return ws

    app = web.Application()
    app["websockets"] = weakref.WeakSet()
    app.add_routes([web.get("/ws", handler)])

    async def on_shutdown(app):
        for ws in set(app["websockets"]):
            await ws.close(code=WSCloseCode.GOING_AWAY, message="Server shutdown")

    app.on_shutdown.append(on_shutdown)

    server = TestServer(app)
    await server.start_server(loop=event_loop)
    yield server
    await app.shutdown()
    await app.cleanup()
    await server.close()


@pytest.fixture()
def logger(event_loop):
    clock = LiveClock()
    return Logger(clock=clock)
