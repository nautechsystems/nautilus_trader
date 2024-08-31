# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.common.component import Logger
from nautilus_trader.live.retry import RetryManager
from nautilus_trader.live.retry import RetryManagerPool


@pytest.fixture
def mock_logger():
    return MagicMock(spec=Logger)


@pytest.mark.asyncio
async def test_retry_manager_successful_run(mock_logger):
    # Arrange
    retry_manager = RetryManager(
        max_retries=3,
        retry_delay=0.1,
        exc_types=(Exception,),
        logger=mock_logger,
    )
    mock_func = AsyncMock()

    # Act
    await retry_manager.run(name="Test Operation", details=None, func=mock_func)

    # Assert
    mock_func.assert_awaited_once()
    mock_logger.warning.assert_not_called()
    mock_logger.error.assert_not_called()


@pytest.mark.asyncio
async def test_retry_manager_with_retries(mock_logger):
    # Arrange
    retry_manager = RetryManager(
        max_retries=3,
        retry_delay=0.1,
        exc_types=(Exception,),
        logger=mock_logger,
    )
    mock_func = AsyncMock(side_effect=[Exception("Test Error"), Exception("Test Error"), None])

    # Act
    await retry_manager.run(name="Test Operation", details=["ID123"], func=mock_func)

    # Assert
    assert mock_func.await_count == 3
    assert mock_logger.warning.call_count == 4
    mock_logger.error.assert_not_called()


@pytest.mark.asyncio
async def test_retry_manager_exhausts_retries(mock_logger):
    # Arrange
    retry_manager = RetryManager(
        max_retries=2,
        retry_delay=0.1,
        exc_types=(Exception,),
        logger=mock_logger,
    )
    mock_func = AsyncMock(side_effect=Exception("Test Error"))

    # Act
    await retry_manager.run(name="Test Operation", details=["ID123"], func=mock_func)

    # Assert
    assert mock_func.await_count == 3
    assert mock_logger.warning.call_count == 5
    mock_logger.error.assert_called_once()


@pytest.mark.asyncio
async def test_retry_manager_pool_acquire_and_release(mock_logger):
    # Arrange
    pool_size = 3
    pool = RetryManagerPool(
        pool_size=pool_size,
        max_retries=2,
        retry_delay=0.1,
        exc_types=(Exception,),
        logger=mock_logger,
    )

    # Act
    async with pool as retry_manager:
        assert isinstance(retry_manager, RetryManager)
        assert len(pool._pool) == pool_size - 1

    # Assert
    assert len(pool._pool) == pool_size


@pytest.mark.asyncio
async def test_retry_manager_pool_create_new_when_empty(mock_logger):
    # Arrange
    pool_size = 1
    pool = RetryManagerPool(
        pool_size=pool_size,
        max_retries=2,
        retry_delay=0.1,
        exc_types=(Exception,),
        logger=mock_logger,
    )

    # Act
    async with pool as retry_manager1:
        async with pool as retry_manager2:
            # Ensure new manager was created as pool empty
            assert retry_manager1 is not retry_manager2

    # Assert
    assert len(pool._pool) == pool_size
