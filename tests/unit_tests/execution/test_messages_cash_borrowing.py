from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


def _order_factory() -> OrderFactory:
    return OrderFactory(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=StrategyId("S-001"),
        clock=TestClock(),
    )


def test_submit_order_round_trip_preserves_allow_cash_borrowing() -> None:
    clock = TestClock()
    order = _order_factory().limit(
        AUDUSD_SIM.id,
        OrderSide.BUY,
        Quantity.from_int(100_000),
        Price.from_str("1.00000"),
    )

    command = SubmitOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        order=order,
        position_id=PositionId("P-001"),
        command_id=UUID4(),
        ts_init=clock.timestamp_ns(),
        allow_cash_borrowing=True,
    )

    result = SubmitOrder.from_dict(SubmitOrder.to_dict(command))

    assert result.allow_cash_borrowing is True
    assert result == command


def test_submit_order_list_round_trip_preserves_allow_cash_borrowing() -> None:
    clock = TestClock()
    bracket = _order_factory().bracket(
        instrument_id=AUDUSD_SIM.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        sl_trigger_price=Price.from_str("1.00000"),
        tp_price=Price.from_str("1.00100"),
    )

    command = SubmitOrderList(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        order_list=bracket,
        position_id=PositionId("P-001"),
        command_id=UUID4(),
        ts_init=clock.timestamp_ns(),
        allow_cash_borrowing=True,
    )

    result = SubmitOrderList.from_dict(SubmitOrderList.to_dict(command))

    assert result.allow_cash_borrowing is True
    assert result == command
