# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import asyncio
from unittest.mock import MagicMock

import pytest

from nautilus_trader.common.component import Logger
from nautilus_trader.common.component import TestClock
from nautilus_trader.live.enqueue import ThrottledEnqueuer


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
    await asyncio.sleep(0)  # allow the loop to process scheduled callbacks

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
    await asyncio.sleep(0)  # allow the event loop to run the scheduled put

    # Assert: check queue is still size=1
    assert queue.qsize() == 1
