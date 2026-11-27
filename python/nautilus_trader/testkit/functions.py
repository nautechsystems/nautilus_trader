# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
Bounded polling for asynchronous tests.
"""

import asyncio
from collections.abc import Callable


async def eventually(condition: Callable[[], bool], timeout_secs: float = 2.0) -> None:
    """
    Wait for a condition to become true, yielding to the event loop between checks.

    Parameters
    ----------
    condition : Callable[[], bool]
        The predicate to evaluate.
    timeout_secs : float, default 2.0
        The maximum wait in seconds.

    Raises
    ------
    TimeoutError
        If the condition does not become true before the deadline.

    """

    async def await_condition() -> None:
        while True:
            if condition():
                return
            await asyncio.sleep(0)

    await asyncio.wait_for(await_condition(), timeout=timeout_secs)
