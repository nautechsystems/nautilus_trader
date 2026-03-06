import asyncio
from unittest.mock import MagicMock

import pytest

from nautilus_trader.common.component import Logger
from nautilus_trader.common.component import TestClock
from nautilus_trader.live.enqueue import ThrottledEnqueuer
from nautilus_trader.test_kit.functions import eventually


@pytest.fixture
def clock():
    return TestClock()


@pytest.fixture
def logger():
    return MagicMock(Logger)


def test_properties(event_loop, clock, logger):
    # Arrange
    queue = asyncio.Queue(maxsize=5)

    # Act
    enqueuer = ThrottledEnqueuer(
        qname="test_queue",
        queue=queue,
        loop=event_loop,
        clock=clock,
        logger=logger,
    )

    # Assert
    assert enqueuer.qname == "test_queue"
    assert enqueuer.size == 0
    assert enqueuer.capacity == 5

    # Put some items in
    event_loop.run_until_complete(queue.put("item1"))
    event_loop.run_until_complete(queue.put("item2"))
    assert enqueuer.size == 2
    assert enqueuer.capacity == 5


@pytest.mark.asyncio
async def test_enqueue_when_queue_has_capacity(event_loop, clock, logger):
    # Arrange
    queue = asyncio.Queue(maxsize=10)
    enqueuer = ThrottledEnqueuer(
        qname="test_queue",
        queue=queue,
        loop=event_loop,
        clock=clock,
        logger=logger,
    )

    # Act
    # We expect a call_soon_threadsafe to enqueue_nowait_safely
    # But that callback won't run until we let the loop step
    enqueuer.enqueue("message1")
    await eventually(lambda: not queue.empty())

    # Assert: check the queue now has our item
    assert not queue.empty()
    assert queue.get_nowait() == "message1"


@pytest.mark.asyncio
async def test_enqueue_when_queue_is_full(event_loop, clock, logger):
    # Arrange
    queue = asyncio.Queue(maxsize=1)
    await queue.put("message1")

    enqueuer = ThrottledEnqueuer(
        qname="test_queue",
        queue=queue,
        loop=event_loop,
        clock=clock,
        logger=logger,
    )

    # Act: enqueue the new item (queue is full)
    enqueuer.enqueue("message2")
    await eventually(lambda: queue.qsize() == 1)

    # Assert: check queue is still size=1
    assert queue.qsize() == 1
