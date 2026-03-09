from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.generators import ClientOrderIdGenerator
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestOrderPerformance:
    def setup(self):
        self.generator = ClientOrderIdGenerator(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            clock=LiveClock(),
        )

        self.order_factory = OrderFactory(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            clock=TestClock(),
        )

    def test_order_id_generator(self, benchmark):
        benchmark(self.generator.generate)

    def test_market_order_creation(self, benchmark):
        benchmark(
            self.order_factory.market,
            TestIdStubs.audusd_id(),
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

    def test_limit_order_creation(self, benchmark):
        benchmark(
            self.order_factory.limit,
            TestIdStubs.audusd_id(),
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("0.80010"),
        )

    def test_to_own_book_order(self, benchmark):
        order = self.order_factory.limit(
            TestIdStubs.audusd_id(),
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("0.80010"),
        )
        benchmark(order.to_own_book_order)
