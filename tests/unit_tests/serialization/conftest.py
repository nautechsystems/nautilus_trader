from typing import Any, List

from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.position import Position
from tests.test_kit.stubs import TestStubs


def _make_order_events(order, **kwargs):
    submitted = TestStubs.event_order_submitted(order=order)
    order.apply(submitted)
    accepted = TestStubs.event_order_accepted(order=order)
    order.apply(accepted)
    filled = TestStubs.event_order_filled(order=order, **kwargs)
    return submitted, accepted, filled


def nautilus_objects() -> List[Any]:
    """A list of nautilus instances for testing serialization"""
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
    position_id = PositionId("P-001")
    buy = TestStubs.limit_order()
    buy_submitted, buy_accepted, buy_filled = _make_order_events(
        buy,
        instrument=instrument,
        position_id=position_id,
        trade_id=TradeId("BUY"),
    )
    sell = TestStubs.limit_order(side=OrderSide.SELL)
    _, _, sell_filled = _make_order_events(
        sell,
        instrument=instrument,
        position_id=position_id,
        trade_id=TradeId("SELL"),
    )
    open_position = Position(instrument=instrument, fill=buy_filled)
    closed_position = Position(instrument=instrument, fill=buy_filled)
    closed_position.apply(sell_filled)

    return [
        TestStubs.ticker(),
        TestStubs.quote_tick_5decimal(),
        TestStubs.trade_tick_5decimal(),
        TestStubs.bar_5decimal(),
        TestStubs.venue_status_update(),
        TestStubs.instrument_status_update(),
        TestStubs.event_component_state_changed(),
        TestStubs.event_trading_state_changed(),
        TestStubs.event_betting_account_state(),
        TestStubs.event_cash_account_state(),
        TestStubs.event_margin_account_state(),
        # ORDERS
        TestStubs.event_order_accepted(buy),
        TestStubs.event_order_rejected(buy),
        TestStubs.event_order_pending_update(buy_accepted),
        TestStubs.event_order_pending_cancel(buy_accepted),
        TestStubs.event_order_filled(
            order=buy,
            instrument=instrument,
            position_id=open_position.id,
        ),
        TestStubs.event_order_canceled(buy_accepted),
        TestStubs.event_order_expired(buy),
        TestStubs.event_order_triggered(buy),
        # POSITIONS
        TestStubs.event_position_opened(open_position),
        TestStubs.event_position_changed(open_position),
        TestStubs.event_position_closed(closed_position),
    ]
