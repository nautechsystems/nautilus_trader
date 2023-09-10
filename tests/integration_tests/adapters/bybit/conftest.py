import asyncio
import os

import pytest

from nautilus_trader.adapters.bybit.common.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.utils.env import get_env_key

@pytest.fixture(scope="session")
def loop():
    return asyncio.get_event_loop()


@pytest.fixture(scope="session")
def live_clock():
    return LiveClock()


@pytest.fixture(scope="session")
def live_logger(live_clock):
    return Logger(clock=live_clock)


@pytest.fixture(scope="session")
def bybit_http_client(loop, live_clock, live_logger):
    client = BybitHttpClient(
        clock=live_clock,
        logger=live_logger,
        api_key="BYBIT_API_KEY",
        api_secret='BYBIT_API_SECRET',
        base_url="https://api-testnet.bybit.com"
    )
    return client


@pytest.fixture()
def venue() -> Venue:
    raise BYBIT_VENUE

@pytest.fixture()
def data_client():
    pass


@pytest.fixture()
def exec_client():
    pass


@pytest.fixture()
def instrument():
    pass


@pytest.fixture()
def account_state():
    pass