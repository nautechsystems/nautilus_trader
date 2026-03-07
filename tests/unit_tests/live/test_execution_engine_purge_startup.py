from __future__ import annotations

import pytest

from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.test_kit.functions import eventually
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


@pytest.mark.asyncio
async def test_run_startup_purges_executes_configured_purges(event_loop):
    clock = LiveClock()
    trader_id = TraderId("TESTER-001")
    msgbus = MessageBus(
        trader_id=trader_id,
        clock=clock,
    )
    cache = TestComponentStubs.cache()
    cache.add_instrument(AUDUSD_SIM)

    order_factory = OrderFactory(
        trader_id=trader_id,
        strategy_id=StrategyId("S-001"),
        clock=clock,
    )
    position_id = PositionId("P-STARTUP-PURGE")

    opening_order = order_factory.market(
        AUDUSD_SIM.id,
        OrderSide.BUY,
        Quantity.from_int(100_000),
    )
    cache.add_order(opening_order)
    opening_order.apply(TestEventStubs.order_submitted(opening_order, ts_event=0))
    cache.update_order(opening_order)
    opening_order.apply(TestEventStubs.order_accepted(opening_order, ts_event=0))
    cache.update_order(opening_order)
    opening_fill = TestEventStubs.order_filled(
        opening_order,
        instrument=AUDUSD_SIM,
        position_id=position_id,
        last_px=Price.from_str("1.00001"),
        trade_id=TradeId("1"),
        ts_event=0,
    )
    opening_order.apply(opening_fill)
    cache.update_order(opening_order)

    position = Position(instrument=AUDUSD_SIM, fill=opening_fill)
    cache.add_position(position, OmsType.NETTING)

    closing_order = order_factory.market(
        AUDUSD_SIM.id,
        OrderSide.SELL,
        Quantity.from_int(100_000),
    )
    cache.add_order(closing_order)
    closing_order.apply(TestEventStubs.order_submitted(closing_order, ts_event=0))
    cache.update_order(closing_order)
    closing_order.apply(TestEventStubs.order_accepted(closing_order, ts_event=0))
    cache.update_order(closing_order)
    closing_fill = TestEventStubs.order_filled(
        closing_order,
        instrument=AUDUSD_SIM,
        position_id=position_id,
        last_px=Price.from_str("1.00002"),
        trade_id=TradeId("2"),
        ts_event=0,
    )
    closing_order.apply(closing_fill)
    cache.update_order(closing_order)
    position.apply(closing_fill)
    cache.update_position(position)

    account = TestExecStubs.cash_account()
    account.apply(TestEventStubs.cash_account_state(account_id=account.id))
    account.apply(TestEventStubs.cash_account_state(account_id=account.id))
    cache.add_account(account)

    assert cache.orders_closed_count() == 2
    assert cache.position_exists(position.id)
    assert account.event_count == 3

    engine = LiveExecutionEngine(
        loop=event_loop,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=LiveExecEngineConfig(
            debug=True,
            purge_closed_orders_interval_mins=10,
            purge_closed_orders_buffer_mins=15,
            purge_closed_positions_interval_mins=10,
            purge_closed_positions_buffer_mins=15,
            purge_account_events_interval_mins=10,
            purge_account_events_lookback_mins=15,
            purge_from_database=True,
        ),
    )

    engine._run_startup_purges()

    try:
        await eventually(
            lambda: not cache.order_exists(opening_order.client_order_id)
            and not cache.order_exists(closing_order.client_order_id)
            and not cache.position_exists(position.id)
            and account.event_count == 1,
        )
    finally:
        if not engine.is_stopped:
            engine.stop()


@pytest.mark.asyncio
async def test_start_does_not_run_startup_purges_inline(event_loop):
    clock = LiveClock()
    trader_id = TraderId("TESTER-001")
    msgbus = MessageBus(
        trader_id=trader_id,
        clock=clock,
    )
    cache = TestComponentStubs.cache()

    engine = LiveExecutionEngine(
        loop=event_loop,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=LiveExecEngineConfig(
            debug=True,
            purge_closed_orders_interval_mins=10,
            purge_closed_positions_interval_mins=10,
            purge_account_events_interval_mins=10,
        ),
    )

    calls: list[str] = []

    def record_startup_purge() -> None:
        calls.append("startup")

    engine._run_startup_purges = record_startup_purge

    engine.start()

    try:
        assert calls == []
        assert engine._purge_closed_orders_task is not None
        assert engine._purge_closed_positions_task is not None
        assert engine._purge_account_events_task is not None
    finally:
        if not engine.is_stopped:
            engine.stop()
