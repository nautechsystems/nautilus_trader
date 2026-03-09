import pytest

from nautilus_trader.common.component import LiveClock


@pytest.fixture(name="clock")
def fixture_clock():
    return LiveClock()
