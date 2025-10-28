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
    thread_pool = ThreadPoolExecutor()
    # Mirror production setup: set as default executor
    event_loop.set_default_executor(thread_pool)
    executor = ActorExecutor(
        loop=event_loop,
        executor=thread_pool,
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


@pytest.mark.asyncio
async def test_cancel_all_tasks_leaves_executor_usable(actor_executor: ActorExecutor) -> None:
    # Arrange
    import time

    handler: list[str] = []

    def slow_append(value: str):
        time.sleep(0.3)
        handler.append(value)

    # Act: Queue some slow tasks, wait for first to start, then cancel all
    actor_executor.queue_for_executor(slow_append, "a")
    actor_executor.queue_for_executor(slow_append, "b")

    # Wait for at least one task to be active
    await eventually(lambda: actor_executor.has_active_tasks())

    # Cancel all tasks
    actor_executor.cancel_all_tasks()

    # Queue a new task after cancelling
    actor_executor.queue_for_executor(handler.append, "c")
    await eventually(lambda: "c" in handler)

    # Assert
    assert "c" in handler
    assert "b" not in handler  # Should have been cancelled before starting


@pytest.mark.asyncio
async def test_reset_leaves_executor_usable(actor_executor: ActorExecutor) -> None:
    # Arrange
    import time

    handler: list[str] = []

    def slow_append(value: str):
        time.sleep(0.3)
        handler.append(value)

    # Act: Queue some slow tasks, wait for first to start, then reset
    actor_executor.queue_for_executor(slow_append, "a")
    actor_executor.queue_for_executor(slow_append, "b")

    # Wait for at least one task to be active
    await eventually(lambda: actor_executor.has_active_tasks())

    # Reset the executor
    actor_executor.reset()

    # Queue a new task after reset
    actor_executor.queue_for_executor(handler.append, "c")
    await eventually(lambda: "c" in handler)

    # Assert
    assert "c" in handler
    assert "b" not in handler  # Should have been cancelled before starting


@pytest.mark.asyncio
async def test_cancel_active_queued_task(actor_executor: ActorExecutor) -> None:
    # Arrange
    import time

    def slow_func():
        time.sleep(0.5)
        return "done"

    # Act: Queue multiple tasks, let first one start, then cancel the second
    task_id1 = actor_executor.queue_for_executor(slow_func)
    task_id2 = actor_executor.queue_for_executor(slow_func)

    # Wait for first task to start executing and second to be queued
    await eventually(lambda: task_id1 in actor_executor.active_task_ids())
    await eventually(lambda: task_id2 in actor_executor.queued_task_ids())

    # Cancel the second task while it's in the queue
    actor_executor.cancel_task(task_id2)

    # Wait for first task to complete
    future1 = actor_executor.get_future(task_id1)
    if future1:
        await asyncio.wrap_future(future1)

    # Assert
    assert task_id2 not in actor_executor.active_task_ids()
    assert task_id2 not in actor_executor.queued_task_ids()


@pytest.mark.asyncio
async def test_shutdown_closes_underlying_executor() -> None:
    # Arrange
    loop = asyncio.get_event_loop()
    thread_pool = ThreadPoolExecutor(max_workers=2)
    actor_executor = ActorExecutor(loop=loop, executor=thread_pool)

    # Act
    handler: list[str] = []
    actor_executor.queue_for_executor(handler.append, "test")
    await eventually(lambda: "test" in handler)
    await actor_executor.shutdown()

    # Assert: ThreadPoolExecutor should be shutdown
    with pytest.raises(RuntimeError, match="cannot schedule new futures after shutdown"):
        thread_pool.submit(lambda: None)


@pytest.mark.asyncio
async def test_cancel_running_queued_task_keeps_worker_alive(
    actor_executor: ActorExecutor,
) -> None:
    # Arrange
    import time

    results: list[str] = []

    def slow_func(msg: str):
        time.sleep(0.5)
        results.append(msg)

    # Act: Queue multiple tasks
    task_id1 = actor_executor.queue_for_executor(slow_func, "first")
    task_id2 = actor_executor.queue_for_executor(slow_func, "second")
    actor_executor.queue_for_executor(slow_func, "third")

    # Wait for first task to be actively executing and second to still be queued
    await eventually(lambda: task_id1 in actor_executor.active_task_ids())
    await eventually(lambda: task_id2 in actor_executor.queued_task_ids())

    # Cancel the actively running task (note: thread will still complete)
    actor_executor.cancel_task(task_id1)

    # Cancel the second task while it's still queued
    actor_executor.cancel_task(task_id2)

    # Verify second task was cancelled before starting
    await eventually(lambda: task_id2 not in actor_executor.queued_task_ids())

    # The third task should still execute (worker should survive the cancellation)
    await eventually(lambda: "third" in results, timeout=3.0)

    # Assert: Worker survived and processed third task
    assert "third" in results
    assert "second" not in results  # Was cancelled before starting
    # Note: "first" may appear since threads can't be truly interrupted


@pytest.mark.asyncio
async def test_shutdown_with_default_executor_set(actor_executor: ActorExecutor) -> None:
    # Arrange
    handler: list[str] = []

    # Act: Queue and execute a task
    actor_executor.queue_for_executor(handler.append, "test")
    await eventually(lambda: "test" in handler)

    # Shutdown should work even when executor is also the default executor
    await actor_executor.shutdown()

    # Assert: No RuntimeError about joining current thread
    assert "test" in handler


@pytest.mark.asyncio
async def test_shutdown_does_not_block_event_loop(actor_executor: ActorExecutor) -> None:
    # Arrange
    import time

    def slow_func():
        time.sleep(0.8)

    # Queue a slow task
    actor_executor.queue_for_executor(slow_func)

    # Wait for it to start
    await eventually(lambda: actor_executor.has_active_tasks())

    # Track if concurrent coroutine can run during shutdown
    concurrent_executed = []
    completed = False

    async def concurrent_coro():
        nonlocal completed
        # This should complete well before the slow_func if event loop is not blocked
        for i in range(5):
            await asyncio.sleep(0.05)
            concurrent_executed.append(i)
        completed = True

    # Act: Start shutdown and concurrent coroutine simultaneously
    shutdown_task = asyncio.create_task(actor_executor.shutdown())
    concurrent_task = asyncio.create_task(concurrent_coro())

    # The concurrent task should make progress while shutdown waits
    await eventually(lambda: len(concurrent_executed) > 0, timeout=1.0)

    await asyncio.gather(shutdown_task, concurrent_task)

    # Assert: Concurrent coroutine completed and made progress during shutdown
    assert completed, "Concurrent coroutine should complete"
    assert len(concurrent_executed) == 5, "All iterations should execute"


@pytest.mark.asyncio
async def test_exception_logged_with_traceback(logger: Mock) -> None:
    # Arrange
    loop = asyncio.get_event_loop()
    thread_pool = ThreadPoolExecutor(max_workers=1)
    actor_executor = ActorExecutor(loop=loop, executor=thread_pool, logger=logger)

    def failing_func():
        def inner_func():
            raise ValueError("Test exception with traceback")

        inner_func()

    # Act
    task_id = actor_executor.run_in_executor(failing_func)
    await eventually(lambda: task_id not in actor_executor.active_task_ids())

    # Assert
    await actor_executor.shutdown()
    assert logger.exception.call_count == 1
    call_args = logger.exception.call_args[0]
    error_message = call_args[0]
    exception_arg = call_args[1]

    assert "Executor: Exception in" in error_message
    assert isinstance(exception_arg, ValueError)
    assert "Test exception with traceback" in str(exception_arg)


@pytest.mark.asyncio
async def test_queued_exception_logged_with_traceback(logger: Mock) -> None:
    # Arrange
    loop = asyncio.get_event_loop()
    thread_pool = ThreadPoolExecutor(max_workers=1)
    actor_executor = ActorExecutor(loop=loop, executor=thread_pool, logger=logger)

    def process_signal_data(data):
        # Simulate nested call stack like real user code
        def validate_data(d):
            if d is None:
                raise ValueError(f"Invalid signal data: {d}")
            return d

        def transform_data(d):
            return validate_data(d)

        transform_data(data)

    # Act
    task_id = actor_executor.queue_for_executor(process_signal_data, None)
    await eventually(lambda: task_id not in actor_executor.queued_task_ids())
    await eventually(lambda: task_id not in actor_executor.active_task_ids())

    # Assert
    await eventually(lambda: logger.exception.called)

    try:
        await actor_executor.shutdown()
    except ValueError:
        pass  # Expected - worker was processing when shutdown was called

    assert logger.exception.call_count == 1
    call_args = logger.exception.call_args[0]
    error_message = call_args[0]
    exception_arg = call_args[1]

    assert "Executor: Exception in" in error_message
    assert isinstance(exception_arg, ValueError)
    assert "Invalid signal data" in str(exception_arg)
