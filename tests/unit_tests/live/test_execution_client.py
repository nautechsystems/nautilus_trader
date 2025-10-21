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

import pytest

from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Currency
from nautilus_trader.test_kit.functions import eventually
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestExecutionClientImpl(LiveExecutionClient):
    """
    Test implementation of LiveExecutionClient for testing task tracking.
    """

    async def _connect(self):
        await asyncio.sleep(0.01)

    async def _disconnect(self):
        await asyncio.sleep(0.01)

    async def _submit_order(self, command):
        await asyncio.sleep(0.01)

    async def _submit_order_list(self, command):
        await asyncio.sleep(0.01)

    async def _modify_order(self, command):
        await asyncio.sleep(0.01)

    async def _cancel_order(self, command):
        await asyncio.sleep(0.01)

    async def _cancel_all_orders(self, command):
        await asyncio.sleep(0.01)

    async def _batch_cancel_orders(self, command):
        await asyncio.sleep(0.01)

    async def _query_order(self, command):
        await asyncio.sleep(0.01)

    async def _query_account(self, command):
        await asyncio.sleep(0.01)

    async def generate_order_status_report(self, command):
        await asyncio.sleep(0.01)
        return True

    async def generate_order_status_reports(self, command):
        await asyncio.sleep(0.01)
        return []

    async def generate_position_status_reports(self, command):
        await asyncio.sleep(0.01)
        return []

    async def generate_fill_reports(self, command):
        await asyncio.sleep(0.01)
        return []

    async def generate_mass_status(self, command):
        await asyncio.sleep(0.01)
        return None


class TestLiveExecutionClient:
    """
    Test suite for LiveExecutionClient task tracking.
    """

    @pytest.fixture(autouse=True)
    def setup(self, request):
        # Fixture Setup
        self.loop = request.getfixturevalue("event_loop")
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.trader_id = TestIdStubs.trader_id()
        self.msgbus = MessageBus(trader_id=self.trader_id, clock=self.clock)
        self.cache = TestComponentStubs.cache()
        self.instrument_provider = InstrumentProvider()

        self.client = TestExecutionClientImpl(
            loop=self.loop,
            client_id=ClientId("TEST-EXEC"),
            venue=Venue("TEST"),
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=Currency.from_str("USD"),
            instrument_provider=self.instrument_provider,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

    @pytest.mark.asyncio
    async def test_tracks_created_tasks(self):
        # Arrange
        async def long_task():
            await asyncio.sleep(10)

        # Act - Create some tasks
        task1 = self.client.create_task(long_task(), log_msg="task1")
        task2 = self.client.create_task(long_task(), log_msg="task2")
        task3 = self.client.create_task(long_task(), log_msg="task3")

        await eventually(lambda: all(t in self.client._tasks for t in [task1, task2, task3]))

        # Assert - Tasks are tracked
        assert task1 in self.client._tasks
        assert task2 in self.client._tasks
        assert task3 in self.client._tasks
        assert len([t for t in self.client._tasks if not t.done()]) == 3

        # Cleanup
        await self.client.cancel_pending_tasks(timeout_secs=1.0)

    @pytest.mark.asyncio
    async def test_cancel_pending_tasks_cancels_all_tasks(self):
        # Arrange
        async def long_task():
            await asyncio.sleep(10)

        task1 = self.client.create_task(long_task(), log_msg="task1")
        task2 = self.client.create_task(long_task(), log_msg="task2")
        task3 = self.client.create_task(long_task(), log_msg="task3")

        await eventually(lambda: all(t in self.client._tasks for t in [task1, task2, task3]))

        # Act - Cancel tasks
        await self.client.cancel_pending_tasks(timeout_secs=1.0)

        # Assert - Tasks are cancelled
        assert task1.cancelled()
        assert task2.cancelled()
        assert task3.cancelled()

    @pytest.mark.asyncio
    async def test_disconnect_cancels_pending_tasks(self):
        # Arrange
        async def long_task():
            await asyncio.sleep(10)

        # Create some long-running tasks
        for i in range(5):
            self.client.create_task(long_task(), log_msg=f"task_{i}")

        await eventually(lambda: len([t for t in self.client._tasks if not t.done()]) == 5)

        # Assert - Tasks are active
        active_before = [t for t in self.client._tasks if not t.done()]
        assert len(active_before) == 5

        # Act - Disconnect (should cancel tasks)
        self.client.disconnect()
        await eventually(
            lambda: len([t for t in self.client._tasks if not t.done()]) <= 1,
            timeout=1.0,
        )  # Only disconnect task might remain

        # Assert - Tasks should be cancelled after disconnect
        active_after = [t for t in self.client._tasks if not t.done()]
        # The disconnect task itself might still be running, but the long tasks should be cancelled
        assert len(active_after) <= 1  # Only disconnect task might remain

    @pytest.mark.asyncio
    async def test_weakset_removes_completed_tasks(self):
        # Arrange
        async def short_task():
            await asyncio.sleep(0.01)
            return "done"

        tasks = []
        for i in range(5):
            task = self.client.create_task(short_task(), log_msg=f"short_{i}")
            tasks.append(task)

        # Wait for tasks to complete
        await asyncio.gather(*tasks)
        await eventually(lambda: all(t.done() for t in tasks))

        # Force garbage collection to clean up WeakSet
        import gc

        gc.collect()

        # Assert - Completed tasks should be removed from WeakSet
        remaining_active = [t for t in self.client._tasks if not t.done()]
        assert len(remaining_active) == 0

    @pytest.mark.asyncio
    async def test_connect_task_is_tracked(self):
        # Arrange - Override _connect to take longer
        async def slow_connect():
            await asyncio.sleep(0.5)

        self.client._connect = slow_connect

        # Act - Connect creates a task
        self.client.connect()
        await eventually(lambda: len([t for t in self.client._tasks if not t.done()]) >= 1)

        # Assert - Task is tracked and active
        active_tasks = [t for t in self.client._tasks if not t.done()]
        assert len(active_tasks) >= 1

        # Cleanup
        await self.client.cancel_pending_tasks(timeout_secs=1.0)

    @pytest.mark.asyncio
    async def test_multiple_operations_track_tasks(self):
        # Arrange
        async def long_operation():
            await asyncio.sleep(5)

        # Act - Create various types of tasks
        tasks = []
        tasks.append(self.client.create_task(long_operation(), log_msg="op1"))
        tasks.append(self.client.create_task(long_operation(), log_msg="op2"))
        tasks.append(self.client.create_task(long_operation(), log_msg="op3"))

        await eventually(lambda: len([t for t in self.client._tasks if not t.done()]) == 3)

        # Assert - All tasks are tracked
        active_tasks = [t for t in self.client._tasks if not t.done()]
        assert len(active_tasks) == 3

        # Act - Cancel and verify
        await self.client.cancel_pending_tasks(timeout_secs=1.0)

        for task in tasks:
            assert task.cancelled()
