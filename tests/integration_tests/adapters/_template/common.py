import asyncio
from typing import Optional

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.core.message import Event
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory
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
        exec_client_factory: Optional[LiveExecClientFactory] = None,
        exec_client_config: Optional[LiveExecClientConfig] = None,
        data_client_factory: Optional[LiveDataClientFactory] = None,
        data_client_config: Optional[LiveDataClientConfig] = None,
        instrument_provider: Optional[InstrumentProvider] = None,
    ):
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.venue = venue
        self.instrument = instrument
        self.instrument_provider = instrument_provider

        # Identifiers
        self.instrument_id = self.instrument.id
        self.trader_id = TestIdStubs.trader_id()
        self.account_id = AccountId(f"{self.venue.value}-001")
        self.venue_order_id = VenueOrderId("V-1")
        self.client_order_id = ClientOrderId("C-1")

        # Components
        self.clock = LiveClock()
        self.logger: Logger = Logger(clock=self.clock)
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

        # Create clients & strategy
        self.exec_client = None
        if exec_client_factory is not None and exec_client_config is not None:
            self.exec_client = exec_client_factory.create(
                loop=self.loop,
                name=self.venue.value,
                config=exec_client_config,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
            )
            self.exec_engine.register_client(self.exec_client)

        self.data_client = None
        if data_client_factory is not None and data_client_config is not None:
            self.data_client = data_client_factory.create(
                loop=self.loop,
                name=self.venue.value,
                config=data_client_config,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
            )
            self.data_engine.register_client(self.data_client)

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

        self.logs: list[str] = []
        self.logger.register_sink(self.logs.append)
