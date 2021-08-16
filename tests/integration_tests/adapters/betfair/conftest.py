import asyncio

import pytest

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LogLevel
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


INSTRUMENTS = []


@pytest.fixture(scope="session")
def loop():
    return asyncio.get_event_loop()


@pytest.fixture(scope="session", autouse=True)
def instrument_list(loop: asyncio.AbstractEventLoop):
    global INSTRUMENTS
    client = BetfairTestStubs.betfair_client()
    logger = LiveLogger(loop=loop, clock=LiveClock(), level_stdout=LogLevel.DEBUG)
    instrument_provider = BetfairInstrumentProvider(client=client, logger=logger, market_filter={})
    t = loop.create_task(instrument_provider.load_all_async())
    loop.run_until_complete(t)
    INSTRUMENTS.extend(instrument_provider.list_instruments())
    assert INSTRUMENTS
