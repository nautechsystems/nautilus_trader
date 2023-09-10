import asyncio

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import Logger
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestBybitFactories:
    def setup(self):
        self.loop = asyncio.get_event_loop()
        self.clock = LiveClock()
        self.logger = Logger(
            clock=self.clock,
            level_stdout=LogLevel.DEBUG,
            bypass=True,
        )

        self.trader_id = TestIdStubs.trader_id()
        self.strategy_id = TestIdStubs.strategy_id()
        self.account_id = TestIdStubs.account_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache_db = MockCacheDatabase(
            logger=self.logger,
        )

        self.cache = Cache(
            database=self.cache_db,
            logger=self.logger,
        )
