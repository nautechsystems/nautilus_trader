import pytest

from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_VENUE
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.model.identifiers import Venue


@pytest.fixture(scope="session")
def live_clock():
    return LiveClock()


@pytest.fixture(scope="session")
def live_logger():
    return Logger("TEST_LOGGER")


@pytest.fixture
def venue() -> Venue:
    raise POLYMARKET_VENUE


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
