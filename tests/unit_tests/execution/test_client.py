from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.execution.client import ExecutionClient
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import OrderDenied
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestExecutionClient:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
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

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.venue = Venue("SIM")

        self.client = ExecutionClient(
            client_id=ClientId(self.venue.value),
            venue=self.venue,
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.order_factory = OrderFactory(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            clock=TestClock(),
        )

    def test_venue_when_brokerage_returns_client_id_value_as_venue(self):
        assert self.client.venue == self.venue

    def test_venue_when_routing_venue_returns_none(self):
        # Arrange
        client = ExecutionClient(
            client_id=ClientId("IB"),
            venue=None,  # Multi-venue
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert
        assert client.venue is None

    def test_generate_order_denied_emits_order_denied_event(self):
        # Arrange
        clock = TestClock()
        trader_id = TestIdStubs.trader_id()
        msgbus = MessageBus(trader_id=trader_id, clock=clock)
        cache = TestComponentStubs.cache()
        client = ExecutionClient(
            client_id=ClientId(self.venue.value),
            venue=self.venue,
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        captured: dict[str, OrderDenied] = {}

        def handler(event):
            captured["event"] = event

        msgbus.register("ExecEngine.process", handler)

        order_factory = OrderFactory(
            trader_id=trader_id,
            strategy_id=StrategyId("S-UNIT"),
            clock=TestClock(),
        )
        order = order_factory.market(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=AUDUSD_SIM.make_qty(1.0),
        )

        reason = "quote quantity required"
        ts_event = 123456789

        # Act
        client.generate_order_denied(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            reason=reason,
            ts_event=ts_event,
        )

        # Assert
        event = captured.get("event")
        assert event is not None
        assert isinstance(event, OrderDenied)
        assert event.instrument_id == order.instrument_id
        assert event.client_order_id == order.client_order_id
        assert event.reason == reason
        assert event.ts_event == ts_event
        msgbus.deregister("ExecEngine.process", handler)
