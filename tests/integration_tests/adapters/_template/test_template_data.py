import pytest

from nautilus_trader.adapters._template.data import TemplateLiveMarketDataClient
from nautilus_trader.live.data_client import LiveMarketDataClient


pytestmark = pytest.mark.skip(reason="template")


@pytest.fixture
def data_client() -> LiveMarketDataClient:
    return TemplateLiveMarketDataClient()  # type: ignore


def test_connect(data_client: LiveMarketDataClient):
    data_client.connect()
    assert data_client.is_connected


def test_disconnect(data_client: LiveMarketDataClient):
    data_client.connect()
    data_client.disconnect()
    assert not data_client.is_connected


def test_reset(data_client: LiveMarketDataClient):
    pass


def test_dispose(data_client: LiveMarketDataClient):
    pass
