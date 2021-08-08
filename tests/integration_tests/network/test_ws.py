import asyncio

import pytest

from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.network.ws_client import WebsocketClient
from tests.test_kit.stubs import TestStubs


@pytest.fixture()
def logger_adapter() -> LoggerAdapter:
    return LoggerAdapter("socket_test", TestStubs.logger())


@pytest.mark.asyncio
@pytest.mark.skip
async def test_client_recv(logger_adapter):
    NUM_MESSAGES = 3
    lines = []

    def record(*args, **kwargs):
        lines.append((args, kwargs))

    client = WebsocketClient(
        ws_url="ws://echo.websocket.org",
        handler=record,
        logger=logger_adapter,
        loop=asyncio.get_event_loop(),
    )
    await client.connect()
    for _ in range(NUM_MESSAGES):
        await client.send(b"Hello")
    await asyncio.sleep(1)
    await client.close()
    assert len(lines) == NUM_MESSAGES
