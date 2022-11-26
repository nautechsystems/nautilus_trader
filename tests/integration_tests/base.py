import asyncio

import pytest

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from tests.test_kit.stubs.commands import TestCommandStubs
from tests.test_kit.stubs.component import TestComponentStubs
from tests.test_kit.stubs.execution import TestExecStubs
from tests.test_kit.stubs.identifiers import TestIdStubs


class BaseClient:
    @property
    def instrument(self) -> Instrument:
        raise NotImplementedError

    @property
    def instrument_id(self) -> InstrumentId:
        return self.instrument.id


class TestBaseDataClient(BaseClient):
    def setup(self):
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)
        self.clock = LiveClock()
        self.trader_id = TestIdStubs.trader_id()
        self.uuid = UUID4()
        self.logger = LiveLogger(loop=self.loop, clock=self.clock, level_stdout=LogLevel.ERROR)
        self._log = LoggerAdapter("TestBaseDataClient", self.logger)
        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )
        self.cache = TestComponentStubs.cache()
        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )
        self.data_engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

    @property
    def data_client(self) -> LiveMarketDataClient:
        raise NotImplementedError

    @pytest.mark.asyncio
    async def test_connect(self):
        self.data_client.connect()
        await asyncio.sleep(0)
        assert self.data_client.is_connected

    def test_subscribe_trade_ticks(self):
        self.data_client.subscribe_trade_ticks(self.instrument)


class TestBaseExecClient(BaseClient):
    def setup(self):
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)
        self.clock = LiveClock()
        self.trader_id = TestIdStubs.trader_id()
        self.uuid = UUID4()
        self.logger = LiveLogger(loop=self.loop, clock=self.clock, level_stdout=LogLevel.ERROR)
        self._log = LoggerAdapter("TestBaseDataClient", self.logger)
        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )
        self.cache = TestComponentStubs.cache()
        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )
        config = LiveExecEngineConfig()
        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=config,
        )

        # Re-route exec engine messages through `handler`
        self.messages = []

        def handler(func):
            def inner(x):
                self.messages.append(x)
                return func(x)

            return inner

        def listener(x):
            print(x)

        self.msgbus.subscribe("*", handler)

    @property
    def exec_client(self) -> LiveExecutionClient:
        raise NotImplementedError

    @pytest.mark.asyncio
    async def test_connect(self):
        self.exec_engine.connect()
        await asyncio.sleep(0)
        assert self.exec_client.is_connected

    @pytest.mark.asyncio
    async def test_submit_order(self):
        # Arrange
        order = TestExecStubs.market_order(instrument_id=self.instrument.id)
        command = TestCommandStubs.submit_order_command(order=order)
        self.exec_client.submit_order(command)
        await asyncio.sleep(0)

    def assert_order_submitted(self):
        raise NotImplementedError
