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
Test retained allocations across repeated in-process live node lifecycles.
"""

import asyncio
import gc
import weakref

import pytest
from tests.unit.test_component_ownership import OwnershipExecutionAlgorithm
from tests.unit.test_component_ownership import OwnershipStrategy

from nautilus_trader.common import Environment
from nautilus_trader.config import ImportableActorConfig
from nautilus_trader.config import LiveExecutionEngineConfig
from nautilus_trader.config import LiveNodeConfig
from nautilus_trader.config import LoggerConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.live import NodeState
from nautilus_trader.model import TraderId


pytest.importorskip("pytest_memray")

_RUNS = 16
_NODE_CONFIG = LiveNodeConfig(
    trader_id=TraderId("MEMRAY-001"),
    environment=Environment.SANDBOX,
    logging=LoggerConfig(bypass_logging=True),
    exec_engine=LiveExecutionEngineConfig(reconciliation=False),
    timeout_connection_secs=0,
    timeout_reconciliation_secs=0,
    timeout_portfolio_secs=0,
    timeout_disconnection_secs=0,
    delay_post_stop_secs=0,
    timeout_shutdown_secs=0,
)
_ACTOR_CONFIG = ImportableActorConfig(
    actor_path="tests.unit.common.actor:TestActor",
    config_path="tests.unit.common.actor:TestActorConfig",
    config={},
)


@pytest.fixture(scope="module", autouse=True)
def _warm_up_live_node() -> None:
    asyncio.run(_run_live_node_lifecycles(1))


@pytest.mark.limit_leaks("32 KB")
def test_live_node_lifecycles_release_python_and_native_allocations() -> None:
    """
    Test repeated live node startup and shutdown releases owned resources.
    """
    asyncio.run(_run_live_node_lifecycles(_RUNS))
    gc.collect()


async def _run_live_node_lifecycles(runs: int) -> None:
    for _ in range(runs):
        sentinels = await _run_live_node()
        gc.collect()

        assert tuple(sentinel() for sentinel in sentinels) == (None, None)


async def _run_live_node() -> tuple[
    weakref.ReferenceType[OwnershipStrategy],
    weakref.ReferenceType[OwnershipExecutionAlgorithm],
]:
    node = LiveNode.build("MEMRAY", _NODE_CONFIG)
    node.add_actor_from_config(_ACTOR_CONFIG)

    strategy = OwnershipStrategy()
    exec_algorithm = OwnershipExecutionAlgorithm()
    node.add_strategy(strategy)
    node.add_exec_algorithm(exec_algorithm)

    sentinels = weakref.ref(strategy), weakref.ref(exec_algorithm)
    del strategy, exec_algorithm

    cache = node.cache
    portfolio = node.portfolio
    handle = node.handle()
    task = asyncio.create_task(node.run_async())

    try:
        async with asyncio.timeout(10.0):
            while not handle.is_running:
                if task.done():
                    await task
                await asyncio.sleep(0)

        assert cache.orders() == []
        assert cache.positions() == []
        assert portfolio.is_initialized() is False

        handle.stop()
        async with asyncio.timeout(10.0):
            await task

        assert handle.state == NodeState.STOPPED
        assert handle.is_running is False
        assert task.done() is True
        assert task.exception() is None
    finally:
        try:
            if not task.done():
                handle.stop()
                async with asyncio.timeout(10.0):
                    await task
        finally:
            node.dispose()

    return sentinels
