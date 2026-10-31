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
Host-loop behavior a `LiveNode` must hold across loop implementations and teardown.

These cases drive the loop themselves rather than borrowing pytest-asyncio's, because
loop choice and loop closure are what is under test.

"""

import asyncio
import time
from collections.abc import Callable

import pytest

from nautilus_trader.common import DataActor
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveExecutionEngineConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveNodeConfig
from nautilus_trader.live import NodeState
from nautilus_trader.model import TraderId


pytestmark = pytest.mark.usefixtures("collect_node_cycles")


try:
    import uvloop
except ImportError:  # uvloop is an optional test dependency and is unavailable on Windows
    uvloop = None

requires_uvloop = pytest.mark.skipif(uvloop is None, reason="uvloop is not installed")

# Skipping only the uvloop cases keeps the asyncio coverage on platforms without uvloop.
LOOP_RUNNERS = [pytest.param(asyncio.run, id="asyncio")]
if uvloop is not None:
    LOOP_RUNNERS.append(pytest.param(uvloop.run, id="uvloop"))


def build_node(trader_id: str) -> LiveNode:
    """
    Build node.
    """
    return LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=TraderId(trader_id),
            environment=Environment.SANDBOX,
            exec_engine=LiveExecutionEngineConfig(reconciliation=False),
            timeout_connection_secs=0,
            timeout_reconciliation_secs=0,
            timeout_portfolio_secs=0,
            timeout_disconnection_secs=0,
            delay_post_stop_secs=0,
            timeout_shutdown_secs=0,
        ),
    )


@requires_uvloop
def test_node_runs_on_a_uvloop_event_loop() -> None:
    """
    Check await suspension still works on uvloop.
    """
    node = build_node("LOOPS-001")
    handle = node.handle()
    observed = {}

    async def main() -> None:
        """
        Run the entry point.
        """
        observed["loop"] = type(asyncio.get_running_loop()).__module__
        task = asyncio.create_task(node.run_async())
        async with asyncio.timeout(15.0):
            while not handle.is_running:
                await asyncio.sleep(0.01)

        observed["running"] = handle.is_running
        handle.stop()
        async with asyncio.timeout(10.0):
            await task

    uvloop.run(main())

    assert observed["loop"].startswith("uvloop"), "test did not run on uvloop"
    assert observed["running"] is True
    assert handle.state == NodeState.STOPPED


@requires_uvloop
def test_uvloop_host_stays_responsive_while_the_node_runs() -> None:
    """
    Test uvloop host stays responsive while the node runs.
    """
    node = build_node("LOOPS-002")
    handle = node.handle()
    running: list[bool] = []

    async def main() -> None:
        task = asyncio.create_task(node.run_async())
        try:
            async with asyncio.timeout(15.0):
                while not handle.is_running:
                    await asyncio.sleep(0.01)

                for _ in range(200):
                    await asyncio.sleep(0)
                    running.append(handle.is_running and not task.done())
        finally:
            handle.stop()
            async with asyncio.timeout(10.0):
                await task

    uvloop.run(main())

    assert running == [True] * 200
    assert handle.state == NodeState.STOPPED


def test_loop_closed_with_a_pending_run_still_stops_the_node() -> None:
    """
    Drain shutdown when the loop closes during a pending run.
    """
    node = build_node("LOOPS-003")
    handle = node.handle()

    async def main() -> None:
        """
        Run the entry point.
        """
        task = asyncio.create_task(node.run_async())
        assert task is not None
        async with asyncio.timeout(15.0):
            while not handle.is_running:
                await asyncio.sleep(0.01)

        assert handle.is_running is True

    # Returning with the task pending closes the loop under it.
    asyncio.run(main())

    assert handle.state == NodeState.STOPPED, "node was abandoned when the loop closed"
    assert handle.is_running is False


@requires_uvloop
def test_loop_closed_with_a_pending_run_on_uvloop() -> None:
    """
    Test loop closed with a pending run on uvloop.
    """
    node = build_node("LOOPS-004")
    handle = node.handle()

    async def main() -> None:
        """
        Run the entry point.
        """
        task = asyncio.create_task(node.run_async())
        assert task is not None
        async with asyncio.timeout(15.0):
            while not handle.is_running:
                await asyncio.sleep(0.01)

        assert handle.is_running is True

    uvloop.run(main())

    assert handle.state == NodeState.STOPPED


class TimerBurstActor(DataActor):
    """
    Queues timer events before the host loop resumes to exercise a saturated runner.
    """

    def _queue_burst(self, count: int) -> None:
        self.count = count
        self.fired = 0
        self.progress: list[int] = []
        self.loop = asyncio.get_running_loop()
        self.completed = asyncio.Event()
        due = self.clock.timestamp_ns()
        for index in range(count):
            self.clock.set_time_alert_ns(f"burst-{index}", due, self._on_alert)

        # Hold the host here until native timer tasks finish queuing the entire burst
        deadline = time.monotonic() + 15.0
        while self.clock.timer_count() > 0:
            assert time.monotonic() < deadline, "timer burst did not enter the runner queue"
            time.sleep(0.001)

    def _on_alert(self, _event: object) -> None:
        self.fired += 1
        if self.fired == 1:
            self.loop.call_soon(self._record_progress)
        if self.fired == self.count:
            self.completed.set()

    def _record_progress(self) -> None:
        self.progress.append(self.fired)


@pytest.mark.parametrize("loop_runner", LOOP_RUNNERS)
def test_host_loop_progresses_during_a_timer_burst(loop_runner: Callable[..., None]) -> None:
    """
    Check a dispatch batch yields to the host within 64 queued timer callbacks.
    """
    burst = 5_000
    node = build_node(f"LOOPS-{'A' if loop_runner is asyncio.run else 'U'}05")
    handle = node.handle()
    actor = TimerBurstActor()
    node.add_actor(actor)

    async def main() -> None:
        task = asyncio.create_task(node.run_async())
        try:
            async with asyncio.timeout(15.0):
                while not handle.is_running:
                    await asyncio.sleep(0.01)

                actor._queue_burst(burst)
                await actor.completed.wait()
        finally:
            handle.stop()
            async with asyncio.timeout(15.0):
                await task

    loop_runner(main())

    assert actor.fired == burst, "burst did not drain"
    assert len(actor.progress) == 1
    assert 0 < actor.progress[0] <= 64, "dispatch batch exceeded its callback budget"
    assert handle.state == NodeState.STOPPED
