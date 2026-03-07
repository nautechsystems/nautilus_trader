from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.list import OrderList
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.trading.strategy import Strategy


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


def _strategy() -> tuple[Strategy, MessageBus]:
    clock = TestClock()
    trader_id = TraderId("TRADER-001")
    msgbus = MessageBus(trader_id=trader_id, clock=clock)
    cache = TestComponentStubs.cache()
    portfolio = Portfolio(msgbus=msgbus, cache=cache, clock=clock)
    strategy = Strategy()
    strategy.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    return strategy, msgbus


def test_submit_order_sets_allow_cash_borrowing_on_command() -> None:
    strategy, msgbus = _strategy()
    captured = []
    msgbus.register(endpoint="RiskEngine.execute", handler=captured.append)

    order = strategy.order_factory.market(
        AUDUSD_SIM.id,
        OrderSide.BUY,
        Quantity.from_int(100_000),
    )

    strategy.submit_order(order, allow_cash_borrowing=True)

    assert captured
    assert captured[-1].allow_cash_borrowing is True


def test_submit_order_list_sets_allow_cash_borrowing_on_command() -> None:
    strategy, msgbus = _strategy()
    captured = []
    msgbus.register(endpoint="RiskEngine.execute", handler=captured.append)

    order1 = strategy.order_factory.market(
        AUDUSD_SIM.id,
        OrderSide.BUY,
        Quantity.from_int(100_000),
    )
    order2 = strategy.order_factory.market(
        AUDUSD_SIM.id,
        OrderSide.BUY,
        Quantity.from_int(50_000),
    )
    order_list = OrderList(
        order_list_id=strategy.order_factory.generate_order_list_id(),
        orders=[order1, order2],
    )

    strategy.submit_order_list(order_list, allow_cash_borrowing=True)

    assert captured
    assert captured[-1].allow_cash_borrowing is True
