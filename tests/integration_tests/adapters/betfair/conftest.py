import asyncio

import pytest


@pytest.fixture(scope="session")
def loop():
    return asyncio.get_event_loop()
