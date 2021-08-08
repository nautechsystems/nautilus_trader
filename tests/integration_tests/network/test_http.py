import pytest

from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.network.http_client import HTTPClient
from tests.test_kit.stubs import TestStubs


@pytest.fixture()
def logger_adapter() -> LoggerAdapter:
    return LoggerAdapter("socket_test", TestStubs.logger())


@pytest.fixture()
async def client():
    client = HTTPClient(
        logger=logger_adapter,
    )
    await client.connect()
    return client


@pytest.mark.asyncio
async def test_client_get(client):
    resp = await client.get("https://httpbin.org/get")
    assert len(resp) > 100


@pytest.mark.asyncio
async def test_client_post(client):
    resp = await client.get("https://httpbin.org/get")
    assert len(resp) > 100
