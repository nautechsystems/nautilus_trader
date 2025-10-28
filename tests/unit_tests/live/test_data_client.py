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
from nautilus_trader.live.data_client import LiveDataClient
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.functions import eventually
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


BITMEX = Venue("BITMEX")
BINANCE = Venue("BINANCE")
XBTUSD_BITMEX = TestInstrumentProvider.xbtusd_bitmex()
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestDataClientImpl(LiveDataClient):
    """
    Test implementation of LiveDataClient for testing task tracking.
    """

    async def _connect(self):
        await asyncio.sleep(0.01)

    async def _disconnect(self):
        await asyncio.sleep(0.01)

    async def _subscribe(self, command):
        await asyncio.sleep(0.01)

    async def _unsubscribe(self, command):
        await asyncio.sleep(0.01)

    async def _request(self, request):
        await asyncio.sleep(0.01)


class TestMarketDataClientImpl(LiveMarketDataClient):
    """
    Test implementation of LiveMarketDataClient for testing task tracking.
    """

    async def _connect(self):
        await asyncio.sleep(0.01)

    async def _disconnect(self):
        await asyncio.sleep(0.01)

    async def _subscribe(self, command):
        await asyncio.sleep(0.01)

    async def _unsubscribe(self, command):
        await asyncio.sleep(0.01)

    async def _subscribe_instruments(self, command):
        await asyncio.sleep(0.01)

    async def _subscribe_instrument(self, command):
        await asyncio.sleep(0.01)

    async def _subscribe_order_book_deltas(self, command):
        await asyncio.sleep(0.01)

    async def _subscribe_order_book_snapshots(self, command):
        await asyncio.sleep(0.01)

    async def _subscribe_quote_ticks(self, command):
        await asyncio.sleep(0.01)

    async def _subscribe_trade_ticks(self, command):
        await asyncio.sleep(0.01)

    async def _subscribe_mark_prices(self, command):
        await asyncio.sleep(0.01)

    async def _subscribe_index_prices(self, command):
        await asyncio.sleep(0.01)

    async def _subscribe_funding_rates(self, command):
        await asyncio.sleep(0.01)

    async def _subscribe_bars(self, command):
        await asyncio.sleep(0.01)

    async def _subscribe_instrument_status(self, command):
        await asyncio.sleep(0.01)

    async def _subscribe_instrument_close(self, command):
        await asyncio.sleep(0.01)

    async def _unsubscribe_instruments(self, command):
        await asyncio.sleep(0.01)

    async def _unsubscribe_instrument(self, command):
        await asyncio.sleep(0.01)

    async def _unsubscribe_order_book_deltas(self, command):
        await asyncio.sleep(0.01)

    async def _unsubscribe_order_book_snapshots(self, command):
        await asyncio.sleep(0.01)

    async def _unsubscribe_quote_ticks(self, command):
        await asyncio.sleep(0.01)

    async def _unsubscribe_trade_ticks(self, command):
        await asyncio.sleep(0.01)

    async def _unsubscribe_mark_prices(self, command):
        await asyncio.sleep(0.01)

    async def _unsubscribe_index_prices(self, command):
        await asyncio.sleep(0.01)

    async def _unsubscribe_funding_rates(self, command):
        await asyncio.sleep(0.01)

    async def _unsubscribe_bars(self, command):
        await asyncio.sleep(0.01)

    async def _unsubscribe_instrument_status(self, command):
        await asyncio.sleep(0.01)

    async def _unsubscribe_instrument_close(self, command):
        await asyncio.sleep(0.01)

    async def _request(self, request):
        await asyncio.sleep(0.01)

    async def _request_instrument(self, request):
        await asyncio.sleep(0.01)

    async def _request_instruments(self, request):
        await asyncio.sleep(0.01)

    async def _request_quote_ticks(self, request):
        await asyncio.sleep(0.01)

    async def _request_trade_ticks(self, request):
        await asyncio.sleep(0.01)

    async def _request_bars(self, request):
        await asyncio.sleep(0.01)

    async def _request_order_book_snapshot(self, request):
        await asyncio.sleep(0.01)


class TestLiveDataClientTests:
    @pytest.fixture(autouse=True)
    def setup(self, request):
        # Fixture Setup
        self.loop = request.getfixturevalue("event_loop")
        self.loop.set_debug(True)

        self.clock = LiveClock()

        self.trader_id = TestIdStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.client = LiveDataClient(
            loop=self.loop,
            client_id=ClientId("BLOOMBERG"),
            venue=None,  # Multi-venue
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

    def test_dummy_test(self):
        # Arrange, Act, Assert
        assert True  # No exception raised

    @pytest.mark.asyncio
    async def test_tracks_created_tasks(self):
        # Arrange
        client = TestDataClientImpl(
            loop=self.loop,
            client_id=ClientId("TEST-DATA"),
            venue=BINANCE,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act - Create some tasks
        async def long_task():
            await asyncio.sleep(10)

        task1 = client.create_task(long_task(), log_msg="task1")
        task2 = client.create_task(long_task(), log_msg="task2")
        task3 = client.create_task(long_task(), log_msg="task3")

        await eventually(lambda: all(t in client._tasks for t in [task1, task2, task3]))

        # Assert - Tasks are tracked
        assert task1 in client._tasks
        assert task2 in client._tasks
        assert task3 in client._tasks
        assert len([t for t in client._tasks if not t.done()]) == 3

        # Cleanup
        await client.cancel_pending_tasks(timeout_secs=1.0)

    @pytest.mark.asyncio
    async def test_cancel_pending_tasks_cancels_all_tasks(self):
        # Arrange
        client = TestDataClientImpl(
            loop=self.loop,
            client_id=ClientId("TEST-DATA"),
            venue=BINANCE,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        async def long_task():
            await asyncio.sleep(10)

        task1 = client.create_task(long_task(), log_msg="task1")
        task2 = client.create_task(long_task(), log_msg="task2")
        task3 = client.create_task(long_task(), log_msg="task3")

        await eventually(lambda: all(t in client._tasks for t in [task1, task2, task3]))

        # Act - Cancel tasks
        await client.cancel_pending_tasks(timeout_secs=1.0)

        # Assert - Tasks are cancelled
        assert task1.cancelled()
        assert task2.cancelled()
        assert task3.cancelled()

    @pytest.mark.asyncio
    async def test_disconnect_cancels_pending_tasks(self):
        # Arrange
        client = TestDataClientImpl(
            loop=self.loop,
            client_id=ClientId("TEST-DATA"),
            venue=BINANCE,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act - Create some long-running tasks
        async def long_task():
            await asyncio.sleep(10)

        for i in range(5):
            client.create_task(long_task(), log_msg=f"task_{i}")

        await eventually(lambda: len([t for t in client._tasks if not t.done()]) == 5)

        # Assert - Tasks are active
        active_before = [t for t in client._tasks if not t.done()]
        assert len(active_before) == 5

        # Act - Disconnect (should cancel tasks)
        client.disconnect()
        await eventually(
            lambda: len([t for t in client._tasks if not t.done()]) <= 1,
            timeout=1.0,
        )  # Only disconnect task might remain

        # Assert - Tasks should be cancelled after disconnect
        active_after = [t for t in client._tasks if not t.done()]
        # The disconnect task itself might still be running, but the long tasks should be cancelled
        assert len(active_after) <= 1  # Only disconnect task might remain


class TestLiveMarketDataClientTests:
    @pytest.fixture(autouse=True)
    def setup(self, request):
        # Fixture Setup
        self.loop = request.getfixturevalue("event_loop")
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.trader_id = TestIdStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.client = LiveMarketDataClient(
            loop=self.loop,
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=InstrumentProvider(),
        )

    def test_dummy_test(self):
        # Arrange, Act, Assert
        assert True  # No exception raised
