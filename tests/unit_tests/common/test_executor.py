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
from concurrent.futures import ThreadPoolExecutor
from unittest.mock import Mock

import pytest
import pytest_asyncio

from nautilus_trader.common.executor import ActorExecutor
from nautilus_trader.common.executor import TaskId
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.test_kit.functions import eventually


def test_task_id_creation():
    task_id = TaskId.create()
    assert isinstance(task_id, TaskId)
    assert isinstance(task_id.value, str)


def test_task_id_repr():
    value = str(UUID4())
    task_id = TaskId(value=value)
    assert repr(task_id) == f"TaskId('{value}')"


@pytest.fixture
def logger():
    return Mock()


@pytest_asyncio.fixture(name="actor_executor")
async def fixture_actor_executor(event_loop):
    executor = ActorExecutor(
        loop=event_loop,
        executor=ThreadPoolExecutor(),
    )
    yield executor
    await executor.shutdown()


@pytest.mark.asyncio
async def test_cancel_invalid_task(actor_executor: ActorExecutor) -> None:
    # Arrange
    invalid_task_id = TaskId.create()

    # Act
    actor_executor.cancel_task(invalid_task_id)

    # Assert
    assert not actor_executor.has_active_tasks()
    assert not actor_executor.has_queued_tasks()


@pytest.mark.asyncio
async def test_queue_for_executor(actor_executor: ActorExecutor) -> None:
    # Arrange
    def func(x):
        return x + 1

    # Act
    task_id = actor_executor.queue_for_executor(func, 1)

    # Assert
    assert task_id in actor_executor.queued_task_ids()


@pytest.mark.asyncio
async def test_run_in_executor(actor_executor: ActorExecutor) -> None:
    # Arrange
    def func(x):
        return x + 1

    # Act
    task_id = actor_executor.run_in_executor(func, 1)

    # Assert
    assert task_id in actor_executor.active_task_ids()


@pytest.mark.asyncio
async def test_cancel_task(actor_executor: ActorExecutor) -> None:
    # Arrange
    def func(x):
        return x + 1

    # Act
    task_id = actor_executor.queue_for_executor(func, 1)
    actor_executor.cancel_task(task_id)

    # Assert
    assert task_id not in actor_executor.queued_task_ids()


@pytest.mark.asyncio
async def test_cancel_all_tasks(actor_executor: ActorExecutor) -> None:
    # Arrange
    def func(x):
        return x + 1

    # Act
    actor_executor.queue_for_executor(func, 1)
    actor_executor.run_in_executor(func, 2)
    actor_executor.cancel_all_tasks()

    # Assert
    assert not actor_executor.has_active_tasks()
    assert not actor_executor.has_queued_tasks()


@pytest.mark.asyncio
async def test_run_in_executor_execution(actor_executor: ActorExecutor) -> None:
    # Arrange
    handler: list[str] = []
    msg = "a"

    # Act
    actor_executor.run_in_executor(handler.append, msg)

    # Assert
    assert msg in handler
    assert actor_executor.queued_task_ids() == []  # <--- Not queued


@pytest.mark.asyncio
async def test_queue_for_executor_execution(actor_executor: ActorExecutor) -> None:
    # Arrange
    handler: list[str] = []
    msg = "a"

    # Act
    actor_executor.queue_for_executor(handler.append, msg)
    await eventually(lambda: bool(handler))
    await eventually(lambda: not actor_executor.queued_task_ids())

    # Assert
    assert msg in handler


@pytest.mark.asyncio
async def test_function_exception(actor_executor: ActorExecutor) -> None:
    # Arrange
    def func():
        raise ValueError("Test Exception")

    # Act
    task_id = actor_executor.run_in_executor(func)
    future = actor_executor.get_future(task_id)
    assert future
    with pytest.raises(ValueError):
        await future

    # Assert
    assert future.exception() is not None


@pytest.mark.asyncio
async def test_run_in_executor_multiple_functions(actor_executor: ActorExecutor) -> None:
    # Arrange
    def func(x):
        return x + 1

    async def async_func(x):
        task_id = actor_executor.run_in_executor(func, x)
        future = actor_executor.get_future(task_id)
        assert future
        return await asyncio.wrap_future(future)

    # Act
    tasks = [async_func(i) for i in range(5)]
    results = await asyncio.gather(*tasks)

    # Assert
    assert results == [1, 2, 3, 4, 5]
