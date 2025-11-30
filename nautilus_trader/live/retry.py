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
from collections.abc import Awaitable
from collections.abc import Callable
from random import randint

from nautilus_trader.common.component import Logger


def get_exponential_backoff(
    num_attempts: int,
    delay_initial_ms: int = 500,
    delay_max_ms: int = 2_000,
    backoff_factor: int = 2,
    jitter: bool = True,
) -> int:
    """
    Compute the backoff using exponential backoff and jitter.

    Parameters
    ----------
    num_attempts : int, default 1
        The number of attempts that have already been made.
    delay_initial_ms : int, default 500
        The time to sleep in the first attempt.
    delay_max_ms : int, default 2_000
        The maximum delay.
    backoff_factor : int, default 2
        The exponential backoff factor for delays.
    jitter : bool, default True
        Whether or not to apply jitter.

    Notes
    -----
    https://aws.amazon.com/blogs/architecture/exponential-backoff-and-jitter/

    Returns
    -------
    int
        Delay in milliseconds.

    """
    delay = min(delay_max_ms, delay_initial_ms * backoff_factor ** (num_attempts - 1))

    if jitter:
        return randint(delay_initial_ms, delay)  # noqa: S311

    return delay


class RetryManager[T]:
    """
    Provides retry state management for an HTTP request.

    This class is generic over `T`, where `T` is the return type of the
    function passed to the `run` method.

    Parameters
    ----------
    max_retries : int
        The maximum number of retries before failure.
    delay_initial_ms : int
        The initial delay (milliseconds) for retries.
    delay_max_ms : int
        The maximum delay (milliseconds) for exponential backoff.
    backoff_factor : int
        The exponential backoff factor for retry delays.
    logger : Logger
        The logger for the manager.
    exc_types : tuple[Type[BaseException], ...]
        The exception types to handle for retries.
    retry_check : Callable[[BaseException], None], optional
        A function that performs additional checks on the exception.
        If the function returns `False`, a retry will not be attempted.
    error_logger : Callable[[str, BaseException | None], None], optional
        A custom error logging function to use instead of the default logger.error.

    """

    def __init__(
        self,
        max_retries: int,
        delay_initial_ms: int,
        delay_max_ms: int,
        backoff_factor: int,
        logger: Logger,
        exc_types: tuple[type[BaseException], ...],
        retry_check: Callable[[BaseException], bool] | None = None,
        error_logger: Callable[[str, BaseException | None], None] | None = None,
    ) -> None:
        self.max_retries = max_retries
        self.delay_initial_ms = delay_initial_ms
        self.delay_max_ms = delay_max_ms
        self.backoff_factor = backoff_factor
        self.retries = 0
        self.exc_types = exc_types
        self.retry_check = retry_check
        self.error_logger = error_logger
        self.cancel_event = asyncio.Event()
        self.log = logger

        self.name: str | None = None
        self.details: list[object] | None = None
        self.details_str: str | None = None
        self.result: bool = False
        self.message: str | None = None
        self.last_exception: BaseException | None = None

    def __repr__(self) -> str:
        return f"<{type(self).__name__}(name='{self.name}', details={self.details}) at {hex(id(self))}>"

    async def run(
        self,
        name: str,
        details: list[object] | None,
        func: Callable[..., Awaitable[T]],
        *args,
        **kwargs,
    ) -> T | None:
        """
        Execute the given `func` with retry management.

        If an exception in `self.exc_types` is raised, a warning is logged, and the function is
        retried after a delay until the maximum retries are reached, at which point an error is logged.

        Parameters
        ----------
        name : str
            The name of the operation to run.
        details : list[object], optional
            The operation details such as identifiers.
        func : Callable[..., Awaitable[T]]
            The function to execute.
        args : Any
            Positional arguments to pass to the function `func`.
        kwargs : Any
            Keyword arguments to pass to the function `func`.

        Returns
        -------
        T | None
            The result of the executed function, or ``None`` if the retries fail.

        """
        self.name = name
        self.details = details

        try:
            while True:
                if self.cancel_event.is_set():
                    self._cancel()
                    return None

                try:
                    response = await func(*args, **kwargs)
                    self.result = True
                    return response  # Successful request
                except self.exc_types as e:
                    self.last_exception = e

                    if (
                        (self.retry_check and not self.retry_check(e))
                        or not self.max_retries
                        or self.retries >= self.max_retries
                    ):
                        self._log_error()
                        self.result = False
                        self.message = str(e)
                        return None  # Operation failed

                    self.retries += 1
                    retry_delay_ms = get_exponential_backoff(
                        delay_initial_ms=self.delay_initial_ms,
                        delay_max_ms=self.delay_max_ms,
                        backoff_factor=self.backoff_factor,
                        num_attempts=self.retries,
                        jitter=True,
                    )
                    self._log_retry(retry_delay_ms=retry_delay_ms)
                    await asyncio.sleep(retry_delay_ms / 1000)
        except asyncio.CancelledError:
            self._cancel()
            return None

    def cancel(self) -> None:
        """
        Cancel the retry operation.
        """
        self.log.debug(f"Canceling {self!r}")
        self.cancel_event.set()

    def clear(self) -> None:
        """
        Clear all state from this retry manager.
        """
        self.retries = 0
        self.name = None
        self.details = None
        self.details_str = None
        self.result = False
        self.message = None
        self.last_exception = None

    def _cancel(self) -> None:
        self.log.warning(f"Canceled retry for '{self.name}'")
        self.result = False
        self.message = "Canceled retry"

    def _log_retry(self, retry_delay_ms: int) -> None:
        self.log.warning(
            f"Retrying {self.retries}/{self.max_retries} for '{self.name}' "
            f"in {retry_delay_ms / 1000}s{self._details_str()}",
        )

    def _log_error(self) -> None:
        message = f"Failed on {self.name}{self._details_str()}"
        if self.error_logger:
            self.error_logger(message, self.last_exception)
        else:
            self.log.error(message)

    def _details_str(self) -> str:
        self.details_str = " " + ", ".join([repr(x) for x in self.details]) if self.details else ""
        self.details_str += f": {self.last_exception!r}" if self.last_exception else ""
        return self.details_str


class RetryManagerPool[T]:
    """
    Provides a pool of `RetryManager`s.

    Parameters
    ----------
    pool_size : int
        The size of the retry manager pool.
    max_retries : int
        The maximum number of retries before failure.
    delay_initial_ms : int
        The initial delay (milliseconds) for retries.
    delay_max_ms : int
        The maximum delay (milliseconds) for exponential backoff.
    backoff_factor : int
        The exponential backoff factor for retry delays.
    logger : Logger
        The logger for retry managers.
    exc_types : tuple[Type[BaseException], ...]
        The exception types to handle for retries.
    retry_check : Callable[[BaseException], None], optional
        A function that performs additional checks on the exception.
        If the function returns `False`, a retry will not be attempted.
    error_logger : Callable[[str, BaseException | None], None], optional
        A custom error logging function to use instead of the default logger.error.

    """

    def __init__(
        self,
        pool_size: int,
        max_retries: int,
        delay_initial_ms: int,
        delay_max_ms: int,
        backoff_factor: int,
        logger: Logger,
        exc_types: tuple[type[BaseException], ...],
        retry_check: Callable[[BaseException], bool] | None = None,
        error_logger: Callable[[str, BaseException | None], None] | None = None,
    ) -> None:
        self.max_retries = max_retries
        self.delay_initial_ms = delay_initial_ms
        self.delay_max_ms = delay_max_ms
        self.backoff_factor = backoff_factor
        self.logger = logger
        self.exc_types = exc_types
        self.retry_check = retry_check
        self.error_logger = error_logger
        self.pool_size = pool_size
        self._pool: list[RetryManager[T]] = [self._create_manager() for _ in range(pool_size)]
        self._lock = asyncio.Lock()
        self._active_managers: set[RetryManager[T]] = set()

    def _create_manager(self) -> RetryManager:
        return RetryManager(
            max_retries=self.max_retries,
            delay_initial_ms=self.delay_initial_ms,
            delay_max_ms=self.delay_max_ms,
            backoff_factor=self.backoff_factor,
            logger=self.logger,
            exc_types=self.exc_types,
            retry_check=self.retry_check,
            error_logger=self.error_logger,
        )

    def shutdown(self) -> None:
        """
        Gracefully shuts down the retry manager pool, ensuring all active retry managers
        are canceled.

        This method should be called when the component using the pool is stopped, to
        ensure that all resources are released in an orderly manner.

        """
        self.logger.info("Shutting down retry manager pool")
        for retry_manager in self._active_managers:
            retry_manager.cancel()
        self._active_managers.clear()

    async def acquire(self) -> RetryManager:
        """
        Acquire a `RetryManager` from the pool, or creates a new one if the pool is
        empty.

        Returns
        -------
        RetryManager

        """
        async with self._lock:
            if self._pool:
                # Pop the most recently used manager and clear its state
                retry_manager = self._pool.pop()
                retry_manager.clear()
            else:
                # Create new manager if pool is empty
                retry_manager = self._create_manager()

            self._active_managers.add(retry_manager)
            self.logger.debug(f"Acquired {retry_manager!r} (active: {len(self._active_managers)})")
            return retry_manager

    async def release(self, retry_manager: RetryManager) -> None:
        """
        Release the given `retry_manager` back into the pool.

        If the pool is already full, the `retry_manager` will be dropped.

        Parameters
        ----------
        retry_manager : RetryManager
            The manager to be returned to the pool.

        """
        async with self._lock:
            self._active_managers.discard(retry_manager)

            if len(self._pool) < self.pool_size:
                # Append the manager to the pool without clearing its state,
                # state is cleared on acquisition to avoid potential race conditions.
                self._pool.append(retry_manager)
                self.logger.debug(
                    f"Released {retry_manager!r} back to pool (active: {len(self._active_managers)})",
                )
            else:
                # Pool already at capacity
                self.logger.debug(
                    f"Discarding extra {retry_manager!r} (active: {len(self._active_managers)})",
                )
