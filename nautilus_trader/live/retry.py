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

import asyncio

from nautilus_trader.common.component import Logger
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import VenueOrderId


class RetryManager:
    """
    Provides retry state management for an HTTP request.

    Parameters
    ----------
    description : str
        The description for the operation.
    exc_types : tuple[Type[BaseException], ...]
        The exception types to handle for retries.
    max_retries : int
        The maximum number of retries before failure.
    retry_delay_secs : float
        The delay (seconds) between retry attempts.
    logger : Logger
        The logger for the manager.
    client_order_id : ClientOrderId, optional
        The client order ID for the operation.
    venue_order_id : VenueOrderId, optional
        The venue order ID for the operation.

    """

    def __init__(
        self,
        description: str,
        exc_types: tuple[type[BaseException], ...],
        logger: Logger,
        max_retries: int,
        retry_delay: float,
        client_order_id: ClientOrderId | None = None,
        venue_order_id: VenueOrderId | None = None,
    ) -> None:
        self.description = description
        self.exc_types = exc_types
        self.client_order_id = client_order_id
        self.venue_order_id = venue_order_id
        self.max_retries = max_retries
        self.retry_delay = retry_delay
        self.retries = 0
        self.log = logger

    async def run(self, func, *args, **kwargs) -> None:
        """
        Execute the given `func` with retry management.

        If an exception in `self.exc_types` is raised, a warning is logged, and the function is
        retried after a delay until the maximum retries are reached, at which point an error is logged.

        """
        while True:
            try:
                await func(*args, **kwargs)
                return  # Successful request
            except self.exc_types as e:
                self.log.warning(repr(e))
                if self._should_retry():
                    await asyncio.sleep(self.retry_delay)
                    continue
                return  # Operation failed

    def _should_retry(self) -> bool:
        if not self.max_retries or self.retries > self.max_retries:
            self.log.error(
                f"Failed on '{self.description}': "
                f"{repr(self.client_order_id) if self.client_order_id else ''} "
                f"{repr(self.venue_order_id) if self.venue_order_id else ''}",
            )
            return False

        self.retries += 1
        self.log.warning(
            f"Retrying {self.retries}/{self.max_retries} for '{self.description}' in {self.retry_delay}s: "
            f"{repr(self.client_order_id) if self.client_order_id else ''} "
            f"{repr(self.venue_order_id) if self.venue_order_id else ''}",
        )
        return True


class RetryManagerPool:
    """
    Provides a pool of `RetryManager`s.
    """

    def __init__(
        self,
        pool_size: int,
        max_retries: int,
        retry_delay: float,
        exc_types: tuple[type[BaseException], ...],
        logger: Logger,
    ) -> None:
        self.max_retries = max_retries
        self.retry_delay = retry_delay
        self.exc_types = exc_types
        self.logger = logger
        self.pool_size = pool_size
        self._pool: list[RetryManager] = [self._create_manager() for _ in range(pool_size)]
        self._lock = asyncio.Lock()

    def _create_manager(self) -> RetryManager:
        return RetryManager(
            description="",
            exc_types=self.exc_types,
            logger=self.logger,
            max_retries=self.max_retries,
            retry_delay=self.retry_delay,
        )

    async def acquire(
        self,
        description: str,
        client_order_id: ClientOrderId | None = None,
        venue_order_id: VenueOrderId | None = None,
    ) -> RetryManager:
        """
        Acquire a `RetryManager` from the pool, or creates a new one if the pool is
        empty.

        Parameters
        ----------
        description : str
            The description for the operation.
        client_order_id : ClientOrderId, optional
            The client order ID for the operation.
        venue_order_id : VenueOrderId, optional
            The venue order ID for the operation.

        Returns
        -------
        RetryManager

        """
        async with self._lock:
            if self._pool:
                retry_manager = self._pool.pop()
            else:
                retry_manager = self._create_manager()

            retry_manager.retries = 0
            retry_manager.description = description
            retry_manager.client_order_id = client_order_id
            retry_manager.venue_order_id = venue_order_id

        return retry_manager

    async def release(self, retry_manager: RetryManager) -> None:
        """
        Release the given `RetryManager` back into the pool.

        Parameters
        ----------
        retry_manager : RetryManager
            The manager to be returned to the pool.

        """
        async with self._lock:
            if len(self._pool) < self.pool_size:
                retry_manager.retries = 0
                retry_manager.description = ""
                retry_manager.client_order_id = None
                retry_manager.venue_order_id = None
                self._pool.append(retry_manager)
