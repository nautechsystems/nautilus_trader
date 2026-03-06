import msgspec

from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Quantity
from nautilus_trader.serialization.serializer import MsgSpecSerializer
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestSerializationPerformance:
    def setup(self):
        # Fixture Setup
        self.venue = Venue("SIM")
        self.trader_id = TestIdStubs.trader_id()
        self.account_id = TestIdStubs.account_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

        self.order = self.order_factory.market(
            TestIdStubs.audusd_id(),
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        self.command = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=StrategyId("SCALPER-001"),
            position_id=PositionId("P-123456"),
            order=self.order,
            command_id=UUID4(),
            ts_init=0,
        )

        self.serializer = MsgSpecSerializer(encoding=msgspec.msgpack)

    def test_serialize_submit_order(self, benchmark):
        benchmark(self.serializer.serialize, self.command)
