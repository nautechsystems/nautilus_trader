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

import asyncio
import sys
import time

import msgspec
import pytest

from nautilus_trader.cache.database import CacheDatabaseAdapter
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.config import CacheConfig
from nautilus_trader.config import DatabaseConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.serialization.serializer import MsgSpecSerializer
from nautilus_trader.test_kit.functions import ensure_all_tasks_completed
from nautilus_trader.test_kit.functions import eventually
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.trading.strategy import Strategy


_AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")

# Requirements:
# - A Redis service listening on the default port 6379

pytestmark = pytest.mark.skipif(
    sys.platform != "linux",
    reason="databases only supported on Linux",
)


@pytest.mark.xdist_group(name="redis_integration")
class TestTradingNodeCacheFlushOnStart:
    """
    Tests that kernel skips load_cache() when flush_on_start=True.
    """

    def setup(self) -> None:
        self.trader_id = TraderId("TESTER-000")
        self.clock = TestClock()

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

        self.strategy = Strategy()
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.database = CacheDatabaseAdapter(
            trader_id=self.trader_id,
            instance_id=UUID4(),
            serializer=MsgSpecSerializer(encoding=msgspec.msgpack, timestamps_as_str=True),
            config=CacheConfig(database=DatabaseConfig()),
        )

    def teardown(self):
        time.sleep(0.2)
        self.database.flush()
        time.sleep(0.5)
        ensure_all_tasks_completed()

    async def _populate_redis_with_position(self):
        """
        Pre-populate Redis with an instrument, order, and open position.
        """
        self.database.add_instrument(_AUDUSD_SIM)
        await eventually(lambda: self.database.load_instrument(_AUDUSD_SIM.id))

        order = self.strategy.order_factory.stop_market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )
        self.database.add_order(order)
        await eventually(lambda: self.database.load_order(order.client_order_id))

        position_id = PositionId("P-1")
        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))
        order.apply(
            TestEventStubs.order_filled(
                order,
                instrument=_AUDUSD_SIM,
                position_id=position_id,
                last_px=Price.from_str("1.00001"),
            ),
        )

        position = Position(instrument=_AUDUSD_SIM, fill=order.last_event)
        self.database.add_position(position)
        await eventually(lambda: self.database.load_position(position.id))

    @pytest.mark.asyncio
    async def test_flush_on_start_true_skips_cache_loading(self):
        # Arrange: Pre-populate Redis with an open position
        await self._populate_redis_with_position()

        # Verify data exists in Redis
        assert len(self.database.load_orders()) > 0
        assert len(self.database.load_positions()) > 0

        # Act: Create node with flush_on_start=True (cache loading should be skipped)
        loop = asyncio.get_running_loop()
        config = TradingNodeConfig(
            trader_id=self.trader_id,
            logging=LoggingConfig(bypass_logging=True),
            cache=CacheConfig(database=DatabaseConfig(), flush_on_start=True),
        )
        node = TradingNode(config=config, loop=loop)

        # Assert: In-memory cache should be empty
        assert node.kernel.cache.orders() == []
        assert node.kernel.cache.positions() == []

    @pytest.mark.asyncio
    async def test_flush_on_start_false_loads_cache(self):
        # Arrange: Pre-populate Redis with an open position
        await self._populate_redis_with_position()

        # Verify data exists in Redis
        assert len(self.database.load_orders()) > 0
        assert len(self.database.load_positions()) > 0

        # Act: Create node with flush_on_start=False (cache should be loaded normally)
        loop = asyncio.get_running_loop()
        config = TradingNodeConfig(
            trader_id=self.trader_id,
            logging=LoggingConfig(bypass_logging=True),
            cache=CacheConfig(database=DatabaseConfig(), flush_on_start=False),
        )
        node = TradingNode(config=config, loop=loop)

        # Assert: In-memory cache should have the position loaded from Redis
        assert len(node.kernel.cache.orders()) > 0
        assert len(node.kernel.cache.positions()) > 0
