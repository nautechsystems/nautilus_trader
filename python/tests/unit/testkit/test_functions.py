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
Verify bounded predicate waits.
"""

import asyncio

import pytest

from nautilus_trader.testkit.functions import eventually


@pytest.mark.asyncio
async def test_eventually_raises_when_the_predicate_stays_false() -> None:
    """
    A failed predicate reaches the requested deadline before the test's deadline.
    """
    async with asyncio.timeout(1):
        with pytest.raises(TimeoutError):
            await eventually(lambda: False, timeout_secs=0.01)


@pytest.mark.asyncio
async def test_eventually_propagates_cancellation_while_polling() -> None:
    """
    Cancelling a pending wait completes it with cancellation.
    """
    started = asyncio.Event()

    def pending() -> bool:
        started.set()
        return False

    task = asyncio.create_task(eventually(pending))
    async with asyncio.timeout(1):
        await started.wait()

    task.cancel()
    done, _ = await asyncio.wait([task], timeout=1)

    assert done == {task}
    assert task.cancelled() is True
