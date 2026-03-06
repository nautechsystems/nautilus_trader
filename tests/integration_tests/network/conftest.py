import asyncio
import weakref

import pytest
import pytest_asyncio
from aiohttp import WSCloseCode
from aiohttp import WSMsgType
from aiohttp import web
from aiohttp.test_utils import TestServer


async def handle_echo(reader: asyncio.StreamReader, writer: asyncio.StreamWriter):
    tasks = set()

    async def write():
        writer.write(b"connected\r\n")
        while True:
            writer.write(b"hello\r\n")
            await asyncio.sleep(0.1)

    loop = asyncio.get_running_loop()

    task = loop.create_task(write())
    tasks.add(task)

    while True:
        req = await reader.readline()
        if req.strip() == b"close":
            writer.close()


@pytest_asyncio.fixture()
async def socket_server():
    server = await asyncio.start_server(handle_echo, "127.0.0.1", 0)
    addr = server.sockets[0].getsockname()
    async with server:
        await server.start_serving()
        yield addr


@pytest_asyncio.fixture(name="closing_socket_server")
async def fixture_closing_socket_server():
    async def handler(reader: asyncio.StreamReader, writer: asyncio.StreamWriter):
        async def write():
            writer.write(b"connected\r\n")
            await asyncio.sleep(0.1)
            await writer.drain()
            writer.close()
            await writer.wait_closed()
            # print("Server closed")

        await write()

    server = await asyncio.start_server(handler, "127.0.0.1", 0)
    addr = server.sockets[0].getsockname()
    async with server:
        yield addr


@pytest_asyncio.fixture(name="websocket_server")
@pytest.mark.asyncio
async def fixture_websocket_server(event_loop):
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
