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
import random
from typing import Any
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.common.component import Logger
from nautilus_trader.live.retry import RetryManager
from nautilus_trader.live.retry import RetryManagerPool
from nautilus_trader.live.retry import get_exponential_backoff


@pytest.fixture
def mock_logger():
    return MagicMock(spec=Logger)


@pytest.mark.parametrize(
    (
        "num_attempts",
        "delay_initial_ms",
        "backoff_factor",
        "delay_max_ms",
        "jitter",
        "expected_delay",
    ),
    [
        (1, 100, 2, 200, False, 100),
        (5, 100, 2, 200, False, 200),
        (5, 100, 1, 200, False, 100),
        (1, 100, 2, 200, True, 100),
        (5, 100, 2, 200, True, 117),
        (5, 100, 1, 200, True, 100),
    ],
)
def test_retry_backoff(
    num_attempts: int,
    delay_initial_ms: int,
    backoff_factor: int,
    delay_max_ms: int,
    jitter: bool,
    expected_delay: int,
) -> None:
    """
    Test the exponential backoff and jitter calculation.
    """
    # Arrange
    random.seed(1)

    # Act
    delay = get_exponential_backoff(
        num_attempts=num_attempts,
        delay_initial_ms=delay_initial_ms,
        backoff_factor=backoff_factor,
        delay_max_ms=delay_max_ms,
        jitter=jitter,
    )

    # Assert
    assert delay == expected_delay


def test_retry_manager_repr() -> None:
    # Arrange
    name = "submit_order"
    details: list[object] = ["O-123456", "123"]
    retry_manager = RetryManager[str](
        max_retries=3,
        delay_initial_ms=100,
        delay_max_ms=200,
        backoff_factor=2,
        logger=MagicMock(),
        exc_types=(Exception,),
    )
    retry_manager.name = name
    retry_manager.details = details

    # Act
    repr_str = repr(retry_manager)

    # Assert
    assert repr_str.startswith(f"<RetryManager(name='{name}', details={details}) at ")
    assert repr_str.endswith(f"{hex(id(retry_manager))}>")


@pytest.mark.asyncio
async def test_retry_manager_successful_run(mock_logger) -> None:
    # Arrange
    retry_manager = RetryManager[str](
        max_retries=3,
        delay_initial_ms=100,
        delay_max_ms=200,
        backoff_factor=2,
        exc_types=(Exception,),
        logger=mock_logger,
    )
    mock_func = AsyncMock()

    # Act
    await retry_manager.run(name="test", details=None, func=mock_func)

    # Assert
    mock_func.assert_awaited_once()
    mock_logger.warning.assert_not_called()
    mock_logger.error.assert_not_called()


@pytest.mark.asyncio
async def test_retry_manager_with_retries(mock_logger) -> None:
    # Arrange
    retry_manager = RetryManager[str](
        max_retries=3,
        delay_initial_ms=100,
        delay_max_ms=200,
        backoff_factor=2,
        exc_types=(Exception,),
        logger=mock_logger,
    )
    mock_func = AsyncMock(side_effect=[Exception("Test Error"), Exception("Test Error"), None])

    # Act
    await retry_manager.run(name="test", details=["ID123"], func=mock_func)

    # Assert
    assert mock_func.await_count == 3
    assert mock_logger.warning.call_count == 4
    mock_logger.error.assert_not_called()


@pytest.mark.asyncio
async def test_retry_manager_exhausts_retries(mock_logger) -> None:
    # Arrange
    retry_manager = RetryManager[str](
        max_retries=2,
        delay_initial_ms=100,
        delay_max_ms=200,
        backoff_factor=2,
        exc_types=(Exception,),
        logger=mock_logger,
    )
    mock_func = AsyncMock(side_effect=Exception("Test Error"))

    # Act
    await retry_manager.run(name="test", details=["ID123"], func=mock_func)

    # Assert
    assert mock_func.await_count == 3
    assert mock_logger.warning.call_count == 5
    mock_logger.error.assert_called_once()


@pytest.mark.asyncio
async def test_retry_manager_pool_acquire_and_release(mock_logger) -> None:
    # Arrange
    pool_size = 3
    pool = RetryManagerPool[Any](
        pool_size=pool_size,
        max_retries=2,
        delay_initial_ms=100,
        delay_max_ms=200,
        backoff_factor=2,
        exc_types=(Exception,),
        logger=mock_logger,
    )

    # Act
    retry_manager = await pool.acquire()

    # Assert
    assert isinstance(retry_manager, RetryManager)
    assert len(pool._pool) == pool_size - 1

    await pool.release(retry_manager)
    assert len(pool._pool) == pool_size


@pytest.mark.asyncio
async def test_retry_manager_pool_create_new_when_empty(mock_logger) -> None:
    # Arrange
    pool_size = 1
    pool = RetryManagerPool[Any](
        pool_size=pool_size,
        max_retries=2,
        delay_initial_ms=100,
        delay_max_ms=200,
        backoff_factor=2,
        exc_types=(Exception,),
        logger=mock_logger,
    )

    # Act
    retry_manager1 = await pool.acquire()
    retry_manager2 = await pool.acquire()

    await pool.release(retry_manager1)

    # Assert
    # Ensure new manager was created as pool empty
    assert retry_manager1 is not retry_manager2
    assert len(pool._pool) == pool_size


@pytest.mark.asyncio
async def test_retry_manager_with_retry_check(mock_logger) -> None:
    # Arrange
    def retry_check(exception):
        return "Retry" in str(exception)

    retry_manager = RetryManager[str](
        max_retries=3,
        delay_initial_ms=100,
        delay_max_ms=200,
        backoff_factor=2,
        exc_types=(Exception,),
        logger=mock_logger,
        retry_check=retry_check,
    )
    mock_func = AsyncMock(side_effect=[Exception("Do not retry"), Exception("Retry Error"), None])

    # Act
    await retry_manager.run(name="test", details=["ID123"], func=mock_func)

    # Assert
    assert mock_func.await_count == 1
    assert mock_logger.warning.call_count == 1
    mock_logger.error.assert_called_once()


@pytest.mark.asyncio
async def test_retry_manager_cancellation(mock_logger) -> None:
    # Arrange
    retry_manager = RetryManager[str](
        max_retries=5,
        delay_initial_ms=500,
        delay_max_ms=1_000,
        backoff_factor=2,
        logger=mock_logger,
        exc_types=(Exception,),
    )
    mock_func = AsyncMock(side_effect=Exception("Test Error"))

    async def cancel_after_delay():
        await asyncio.sleep(1)
        retry_manager.cancel()

    # Act
    task = asyncio.create_task(cancel_after_delay())
    await retry_manager.run(name="test", details=["ID123"], func=mock_func)

    # Assert
    assert 1 <= mock_func.await_count < 5  # Aborts retry operation
    mock_logger.warning.assert_called_with("Canceled retry for 'test'")
    assert retry_manager.result is False
    assert retry_manager.message == "Canceled retry"
    task.cancel()


@pytest.mark.asyncio
async def test_retry_manager_pool_shutdown(mock_logger) -> None:
    # Arrange
    pool_size = 2
    pool = RetryManagerPool[Any](
        pool_size=pool_size,
        max_retries=3,
        delay_initial_ms=100,
        delay_max_ms=200,
        backoff_factor=2,
        exc_types=(Exception,),
        logger=mock_logger,
    )

    retry_manager = await pool.acquire()

    async def long_running_task():
        await retry_manager.run(
            name="long_running",
            details=["O-123"],
            func=AsyncMock(side_effect=Exception("Test Error")),
        )

    task = asyncio.create_task(long_running_task())

    # Act
    await asyncio.sleep(0.2)
    pool.shutdown()

    # Assert
    await task
    assert len(pool._pool) == 1
    assert retry_manager.result is False
    assert retry_manager.message == "Canceled retry"
