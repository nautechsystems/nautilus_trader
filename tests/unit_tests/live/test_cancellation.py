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
import contextlib
from weakref import WeakSet

import pytest

from nautilus_trader.common.component import Logger
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout
from nautilus_trader.test_kit.functions import eventually


@pytest.mark.asyncio
async def test_cancel_empty_task_set():
    # Arrange
    tasks: WeakSet[asyncio.Task] = WeakSet()
    logger = Logger("TestLogger")

    # Act, Assert - should complete without error
    await cancel_tasks_with_timeout(tasks, logger, timeout_secs=1.0)


@pytest.mark.asyncio
async def test_cancel_already_completed_tasks():
    # Arrange
    tasks: WeakSet[asyncio.Task] = WeakSet()
    logger = Logger("TestLogger")

    async def quick_task():
        return "done"

    task = asyncio.create_task(quick_task())
    await task
    tasks.add(task)

    # Act, Assert - should handle already-done tasks gracefully
    await cancel_tasks_with_timeout(tasks, logger, timeout_secs=1.0)


@pytest.mark.asyncio
async def test_cancel_pending_tasks_successfully():
    # Arrange
    tasks: WeakSet[asyncio.Task] = WeakSet()
    logger = Logger("TestLogger")

    async def long_running_task():
        await asyncio.sleep(10)

    created_tasks = []
    for _ in range(3):
        task = asyncio.create_task(long_running_task())
        tasks.add(task)
        created_tasks.append(task)

    # Act
    await cancel_tasks_with_timeout(tasks, logger, timeout_secs=1.0)

    # Assert
    for task in created_tasks:
        assert task.cancelled()


@pytest.mark.asyncio
async def test_cancel_with_timeout_exceeded():
    # Arrange
    tasks: WeakSet[asyncio.Task] = WeakSet()
    logger = Logger("TestLogger")

    async def stubborn_task():
        while True:
            with contextlib.suppress(asyncio.CancelledError):
                await asyncio.sleep(0.1)

    task = asyncio.create_task(stubborn_task())
    tasks.add(task)

    # Act
    await cancel_tasks_with_timeout(tasks, logger, timeout_secs=0.5)

    # Assert - should timeout waiting for cancellation
    task.cancel()
    with contextlib.suppress(TimeoutError, asyncio.CancelledError):
        await asyncio.wait_for(task, timeout=0.1)


@pytest.mark.asyncio
async def test_cancel_mixed_tasks_and_futures():
    # Arrange
    items: set[asyncio.Task | asyncio.Future] = set()
    logger = Logger("TestLogger")

    async def some_task():
        await asyncio.sleep(10)

    task = asyncio.create_task(some_task())
    items.add(task)

    future: asyncio.Future = asyncio.Future()
    items.add(future)

    # Act
    await cancel_tasks_with_timeout(items, logger, timeout_secs=1.0)

    # Assert
    assert task.cancelled()
    assert future.cancelled()


@pytest.mark.asyncio
async def test_cancel_without_logger():
    # Arrange
    tasks: WeakSet[asyncio.Task] = WeakSet()

    async def simple_task():
        await asyncio.sleep(10)

    task = asyncio.create_task(simple_task())
    tasks.add(task)

    # Act
    await cancel_tasks_with_timeout(tasks, logger=None, timeout_secs=1.0)

    # Assert
    assert task.cancelled()


@pytest.mark.asyncio
async def test_weakset_behavior_during_cancellation():
    # Arrange
    tasks: WeakSet[asyncio.Task] = WeakSet()
    logger = Logger("TestLogger")

    async def long_task():
        await asyncio.sleep(10)

    task1 = asyncio.create_task(long_task())
    task2 = asyncio.create_task(long_task())
    tasks.add(task1)
    tasks.add(task2)

    # Act
    await cancel_tasks_with_timeout(tasks, logger, timeout_secs=1.0)

    # Assert
    assert task1.cancelled()
    assert task2.cancelled()


@pytest.mark.asyncio
async def test_cancel_with_exception_in_task():
    # Arrange
    tasks: WeakSet[asyncio.Task] = WeakSet()
    logger = Logger("TestLogger")

    async def failing_task():
        await asyncio.sleep(0.1)
        raise ValueError("Task failed")

    async def normal_task():
        await asyncio.sleep(10)

    task1 = asyncio.create_task(failing_task())
    task2 = asyncio.create_task(normal_task())
    tasks.add(task1)
    tasks.add(task2)

    await asyncio.sleep(0.2)

    # Act
    await cancel_tasks_with_timeout(tasks, logger, timeout_secs=1.0)

    # Assert
    assert task1.done()
    assert not task1.cancelled()
    assert task2.cancelled()


@pytest.mark.asyncio
async def test_rapid_task_completion_during_cancellation():
    # Arrange
    tasks: WeakSet[asyncio.Task] = WeakSet()
    logger = Logger("TestLogger")
    completion_order = []

    async def quick_task(task_id: int, delay: float):
        await asyncio.sleep(delay)
        completion_order.append(task_id)

    task1 = asyncio.create_task(quick_task(1, 0.001))
    task2 = asyncio.create_task(quick_task(2, 0.002))
    task3 = asyncio.create_task(quick_task(3, 10.0))

    tasks.add(task1)
    tasks.add(task2)
    tasks.add(task3)

    await eventually(lambda: 1 in completion_order and 2 in completion_order, timeout=0.5)

    # Act
    await cancel_tasks_with_timeout(tasks, logger, timeout_secs=1.0)

    # Assert
    assert 1 in completion_order
    assert 2 in completion_order
    assert 3 not in completion_order
    assert task3.cancelled()
