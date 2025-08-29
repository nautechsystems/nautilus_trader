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
"""
Provide task cancellation utilities for live components.
"""

import asyncio
from weakref import WeakSet

from nautilus_trader.common.component import Logger


# Default timeout for canceling futures (shorter than tasks as they represent external connections)
DEFAULT_FUTURE_CANCELLATION_TIMEOUT: float = 2.0

# Default timeout for canceling regular tasks
DEFAULT_TASK_CANCELLATION_TIMEOUT: float = 5.0


async def cancel_tasks_with_timeout(
    tasks: WeakSet[asyncio.Task] | set[asyncio.Task | asyncio.Future],
    logger: Logger | None = None,
    timeout_secs: float = DEFAULT_TASK_CANCELLATION_TIMEOUT,
) -> None:
    """
    Cancel all pending tasks and await their completion with timeout.

    This function takes a strong snapshot of the tasks to ensure they don't get
    garbage collected during cancellation. It cancels all pending tasks and waits
    for them to complete with the specified timeout.

    Parameters
    ----------
    tasks : WeakSet[asyncio.Task] | set[asyncio.Task | asyncio.Future]
        The collection of tasks to cancel. Can be a WeakSet (for normal operation)
        or a regular set (for futures).
    logger : Logger | None, optional
        Logger for debug and warning messages.
    timeout_secs : float, default 5.0
        Maximum time to wait for tasks to complete cancellation.

    Notes
    -----
    - Takes a strong reference snapshot to prevent tasks from being GC'd during cancellation.
    - Uses return_exceptions=True to prevent "exception was never retrieved" warnings.
    - Logs timeout warnings if tasks don't complete within the specified timeout.

    """
    # Take a strong snapshot to prevent tasks from disappearing during cancellation
    pending_tasks = [task for task in tasks if not task.done()]

    if not pending_tasks:
        if logger:
            logger.debug("No pending tasks to cancel")
        return

    if logger:
        logger.debug(f"Canceling {len(pending_tasks)} pending tasks")

    # Cancel all tasks
    for task in pending_tasks:
        task.cancel()

    # Await with the strong references we captured
    try:
        await asyncio.wait_for(
            asyncio.gather(*pending_tasks, return_exceptions=True),
            timeout=timeout_secs,
        )
        if logger:
            logger.debug(f"Successfully canceled {len(pending_tasks)} tasks")
    except TimeoutError:
        if logger:
            logger.warning(
                f"Timeout ({timeout_secs}s) waiting for {len(pending_tasks)} tasks to cancel",
            )
            _log_still_pending_tasks(pending_tasks, logger)


def _log_still_pending_tasks(
    pending_tasks: list[asyncio.Task | asyncio.Future],
    logger: Logger,
) -> None:
    still_pending = [task for task in pending_tasks if not task.done()]
    if not still_pending:
        return

    for task in still_pending:
        # Tasks have get_name(), Futures don't
        if hasattr(task, "get_name"):
            logger.warning(f"Task still pending: {task.get_name()} (id={id(task)})")
        else:
            logger.warning(f"Future still pending: id={id(task)}")
