import asyncio
from typing import Optional, Union

import pytest

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.message import Event
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.live.data_client import LiveDataClient
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


class TestBaseClient:
    def setup(
        self,
        venue: Venue,
        instrument: Instrument,
        exec_client: Optional[LiveExecutionClient] = None,
        data_client: Optional[Union[LiveDataClient, LiveMarketDataClient]] = None,
        instrument_provider: Optional[InstrumentProvider] = None,
    ):
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.venue = venue
        self.instrument = instrument
        self.instrument_provider = instrument_provider
        self.exec_client = exec_client
        self.data_client = data_client

        self.clock = LiveClock()
        self.logger = Logger(clock=self.clock)
        self.instrument_id = self.instrument.id
        self.trader_id = TestIdStubs.trader_id()
        self.account_id = AccountId(f"{self.venue.value}-001")
        self.venue_order_id = VenueOrderId("V-1")
        self.client_order_id = ClientOrderId("C-1")

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

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_engine.register_client(self.exec_client)

        self.strategy = Strategy()
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Capture events flowing through engines
        self.order_events: list[Event] = []
        self.msgbus.subscribe("events.order*", self.order_events.append)

    async def submit_order(self, order):
        self.strategy.submit_order(order)
        await asyncio.sleep(0)

    async def accept_order(self, order, venue_order_id: Optional[VenueOrderId] = None):
        self.strategy.submit_order(order)
        await asyncio.sleep(0)
        self.exec_client.generate_order_accepted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id or order.venue_order_id,
            ts_event=0,
        )
        return order


class TestBaseDataClient(TestBaseClient):
    def setup(
        self,
        venue: Venue,
        instrument: Instrument,
        exec_client: Optional[LiveDataClient] = None,
        data_client: Optional[LiveDataClient] = None,
        instrument_provider: Optional[InstrumentProvider] = None,
    ):
        super().setup(
            venue=venue,
            instrument=instrument,
            exec_client=None,
            data_client=data_client,
            instrument_provider=instrument_provider,
        )

    @pytest.mark.asyncio
    async def test_connect(self):
        self.data_client.connect()
        await asyncio.sleep(0)
        assert self.data_client.is_connected

    def test_subscribe_trade_ticks(self):
        self.data_client.subscribe_trade_ticks(self.instrument)


class TestBaseExecClient(TestBaseClient):
    def setup(
        self,
        venue: Venue,
        instrument: Instrument,
        exec_client: Optional[LiveDataClient] = None,
        data_client: Optional[LiveDataClient] = None,
        instrument_provider: Optional[InstrumentProvider] = None,
    ):
        super().setup(
            venue=venue,
            instrument=instrument,
            exec_client=exec_client,
            data_client=None,
            instrument_provider=instrument_provider,
        )

    @pytest.mark.asyncio
    async def test_connect(self):
        self.exec_client.connect()
        await asyncio.sleep(0)
        assert self.exec_client.is_connected

    # TODO - do we want to do something like this
    # @pytest.mark.asyncio
    # async def test_submit_order(self):
    #     # Arrange
    #     order = TestExecStubs.market_order(instrument_id=self.instrument.id)
    #     command = TestCommandStubs.submit_order_command(order=order)
    #     self.exec_client.submit_order(command)
    #     await asyncio.sleep(0)
    #
    # def assert_order_submitted(self):
    #     raise NotImplementedError
