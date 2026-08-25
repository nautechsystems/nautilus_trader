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
Host-loop behaviour a `LiveNode` must hold across loop implementations and teardown.

These cases drive the loop themselves rather than borrowing pytest-asyncio's, because
loop choice and loop closure are what is under test.

"""

import asyncio
import time
from datetime import UTC
from datetime import datetime
from datetime import timedelta

import pytest

from nautilus_trader.common import DataActor
from nautilus_trader.common import Environment
from nautilus_trader.common import ImportableActorConfig
from nautilus_trader.live import LiveExecutionEngineConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveNodeConfig
from nautilus_trader.live import NodeState
from nautilus_trader.model import TraderId


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
def test_node_runs_on_a_uvloop_event_loop():
    """
    Suspension uses the private `_asyncio_future_blocking` protocol, so it must hold on
    uvloop.
    """
    node = build_node("LOOPS-001")
    handle = node.handle()
    observed = {}

    async def main() -> None:
        observed["loop"] = type(asyncio.get_running_loop()).__module__
        task = asyncio.create_task(node.run_async())
        await asyncio.sleep(0.2)
        observed["running"] = handle.is_running
        handle.stop()
        async with asyncio.timeout(10.0):
            await task

    uvloop.run(main())

    assert observed["loop"].startswith("uvloop"), "test did not run on uvloop"
    assert observed["running"] is True
    assert handle.state == NodeState.STOPPED


@requires_uvloop
def test_uvloop_host_stays_responsive_while_the_node_runs():
    node = build_node("LOOPS-002")
    handle = node.handle()
    gaps: list[float] = []

    async def main() -> None:
        task = asyncio.create_task(node.run_async())

        # Sample the running loop, not the startup drain, which is deliberately unbudgeted.
        async with asyncio.timeout(15.0):
            while not handle.is_running:  # noqa: ASYNC110
                await asyncio.sleep(0.01)

        # uvloop's `loop.time()` is millisecond-granular, so measure with perf_counter.
        previous = time.perf_counter()

        for _ in range(200):
            await asyncio.sleep(0)
            now = time.perf_counter()
            gaps.append(now - previous)
            previous = now

        handle.stop()
        async with asyncio.timeout(10.0):
            await task

    uvloop.run(main())

    # Assert on a percentile: one descheduled sample on a loaded runner says nothing about the
    # budget, while a starvation regression shows up across many samples.
    gaps.sort()
    p99 = gaps[int(len(gaps) * 0.99)]
    assert p99 < 0.01, f"host loop p99 stall {p99 * 1000:.2f}ms"
    assert gaps[-1] < 0.5, f"host loop stalled for {gaps[-1] * 1000:.1f}ms"


def test_loop_closed_with_a_pending_run_still_stops_the_node():
    """
    A closed loop can never resume the run, so `close` must drain shutdown rather than
    leak.
    """
    node = build_node("LOOPS-003")
    handle = node.handle()

    async def main() -> None:
        task = asyncio.create_task(node.run_async())
        assert task is not None
        await asyncio.sleep(0.2)
        assert handle.is_running is True

    # Returning with the task pending closes the loop under it.
    asyncio.run(main())

    assert handle.state == NodeState.STOPPED, "node was abandoned when the loop closed"
    assert handle.is_running is False


@requires_uvloop
def test_loop_closed_with_a_pending_run_on_uvloop():
    node = build_node("LOOPS-004")
    handle = node.handle()

    async def main() -> None:
        task = asyncio.create_task(node.run_async())
        assert task is not None
        await asyncio.sleep(0.2)

    uvloop.run(main())

    assert handle.state == NodeState.STOPPED


class TimerBurstActor(DataActor):
    """
    Schedules many alerts for one instant, saturating the runner's time-event channel.
    """

    count: int = 0
    fired: int = 0

    @classmethod
    def configure(cls, count: int) -> None:
        cls.count = count
        cls.fired = 0

    def on_start(self) -> None:
        cls = type(self)
        due = datetime.now(UTC) + timedelta(milliseconds=300)
        for index in range(cls.count):
            self.clock.set_time_alert(f"burst-{index}", due, self._on_alert)

    def _on_alert(self, event) -> None:
        type(self).fired += 1


@pytest.mark.parametrize("loop_runner", LOOP_RUNNERS)
def test_host_loop_stall_under_a_timer_burst(loop_runner):
    """
    Measure how long a dispatch batch holds the host loop when the runner is saturated.

    This is the evidence behind `DISPATCHES_PER_YIELD`: the budget bounds the stall a host
    application sees between its own callbacks.

    """
    burst = 5_000
    TimerBurstActor.configure(burst)

    node = build_node(f"LOOPS-{'A' if loop_runner is asyncio.run else 'U'}05")
    handle = node.handle()
    node.add_actor_from_config(
        ImportableActorConfig(
            actor_path="tests.integration.test_live_node_loops:TimerBurstActor",
            config_path="nautilus_trader.common:DataActorConfig",
            config={"actor_id": "BURST"},
        ),
    )
    gaps: list[float] = []

    async def main() -> None:
        task = asyncio.create_task(node.run_async())
        loop = asyncio.get_running_loop()

        # Startup registers the alerts inside the first poll; sample the drain, not the setup.
        startup_deadline = loop.time() + 15.0
        while not handle.is_running and loop.time() < startup_deadline:  # noqa: ASYNC110
            await asyncio.sleep(0.01)
        while TimerBurstActor.fired == 0 and loop.time() < startup_deadline:  # noqa: ASYNC110
            await asyncio.sleep(0.001)

        deadline = time.perf_counter() + 10.0
        previous = time.perf_counter()
        while time.perf_counter() < deadline and TimerBurstActor.fired < burst:
            await asyncio.sleep(0)
            now = time.perf_counter()
            gaps.append(now - previous)
            previous = now

        handle.stop()
        async with asyncio.timeout(15.0):
            await task

    loop_runner(main())

    gaps.sort()
    p99 = gaps[int(len(gaps) * 0.99)]

    assert TimerBurstActor.fired == burst, "burst did not drain"

    # Measured p99 on both loops sits under 250us, so 64 dispatches hold the loop well inside a
    # millisecond. p99 is asserted because it is the stable signal; an occasional scheduler
    # outlier says nothing about the budget. The loose max still catches a starvation regression,
    # which drains the whole burst in one poll and costs hundreds of milliseconds.
    assert p99 < 0.005, f"host loop p99 stall {p99 * 1000:.2f}ms under burst"
    assert gaps[-1] < 0.1, f"host loop stalled {gaps[-1] * 1000:.1f}ms under burst"
