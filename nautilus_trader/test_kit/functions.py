# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Callable, TypeVar


T = TypeVar("T")


async def eventually(condition: Callable, timeout: float = 2.0) -> None:
    """
    Await the given condition to eventually evaluate True.

    The intention is to pass an anonymous function as the `condition` which will
    be continually evaluated until either returning True, or the timeout expiring.

    Parameters
    ----------
    condition : Callable
        The condition to evaluate.
    timeout: float, default 2.0
        The amount of time (seconds) to wait for the condition to become True.

    Raises
    ------
    asyncio.TimeoutError
        If `condition` does not become True prior to `timeout` expiring.

    """

    async def await_condition(c):
        while not c():
            await asyncio.sleep(0)

    await asyncio.wait_for(await_condition(condition), timeout=timeout)
