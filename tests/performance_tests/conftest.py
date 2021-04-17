import pytest

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger


@pytest.fixture
def clock():
    return TestClock()


@pytest.fixture
def logger(clock):
    return Logger(clock, bypass_logging=True)
