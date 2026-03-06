import pytest

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.model.identifiers import Venue


@pytest.fixture(scope="session")
def live_clock():
    return LiveClock()


@pytest.fixture(scope="session")
def live_logger():
    return Logger("TEST_LOGGER")


@pytest.fixture(scope="session")
def binance_http_client(session_event_loop, live_clock):
    client = BinanceHttpClient(
        clock=live_clock,
        api_key="SOME_BINANCE_API_KEY",
        api_secret="SOME_BINANCE_API_SECRET",
        base_url="https://api.binance.com/",  # Spot/Margin
    )
    return client


@pytest.fixture
def venue() -> Venue:
    raise BINANCE_VENUE


@pytest.fixture
def data_client():
    pass


@pytest.fixture
def exec_client():
    pass


@pytest.fixture
def instrument():
    pass


@pytest.fixture
def account_state():
    pass
