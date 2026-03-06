from __future__ import annotations

import asyncio

import pytest


@pytest.fixture
def event_loop():
    loop = asyncio.new_event_loop()
    try:
        yield loop
    finally:
        if not loop.is_closed():
            loop.close()
