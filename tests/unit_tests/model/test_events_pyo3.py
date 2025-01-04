# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.nautilus_pyo3 import OrderAccepted
from nautilus_trader.core.nautilus_pyo3 import OrderCanceled
from nautilus_trader.core.nautilus_pyo3 import OrderCancelRejected
from nautilus_trader.core.nautilus_pyo3 import OrderDenied
from nautilus_trader.core.nautilus_pyo3 import OrderEmulated
from nautilus_trader.core.nautilus_pyo3 import OrderExpired
from nautilus_trader.core.nautilus_pyo3 import OrderFilled
from nautilus_trader.core.nautilus_pyo3 import OrderInitialized
from nautilus_trader.core.nautilus_pyo3 import OrderModifyRejected
from nautilus_trader.core.nautilus_pyo3 import OrderPendingCancel
from nautilus_trader.core.nautilus_pyo3 import OrderPendingUpdate
from nautilus_trader.core.nautilus_pyo3 import OrderRejected
from nautilus_trader.core.nautilus_pyo3 import OrderReleased
from nautilus_trader.core.nautilus_pyo3 import OrderSubmitted
from nautilus_trader.core.nautilus_pyo3 import OrderTriggered
from nautilus_trader.core.nautilus_pyo3 import OrderUpdated
from nautilus_trader.test_kit.rust.events_pyo3 import TestEventsProviderPyo3


def test_order_denied():
    event = TestEventsProviderPyo3.order_denied_max_submit_rate()
    result_dict = OrderDenied.to_dict(event)
    order_denied = OrderDenied.from_dict(result_dict)
    assert order_denied == event
    assert (
        str(event)
        == "OrderDenied(instrument_id=AUD/USD.SIM, client_order_id=O-20210410-022422-001-001-1, "
        + "reason='Exceeded MAX_ORDER_SUBMIT_RATE')"
    )
    assert (
        repr(event)
        == "OrderDenied(trader_id=TESTER-001, strategy_id=S-001, "
        + "instrument_id=AUD/USD.SIM, client_order_id=O-20210410-022422-001-001-1, "
        + "reason='Exceeded MAX_ORDER_SUBMIT_RATE', event_id=91762096-b188-49ea-8562-8d8a4cc22ff2, ts_init=0)"
    )


def test_order_filled():
    event = TestEventsProviderPyo3.order_filled_buy_limit()
    assert event.is_buy
    assert not event.is_sell
    result_dict = OrderFilled.to_dict(event)
    order_filled = OrderFilled.from_dict(result_dict)
    assert order_filled == event
    assert (
        str(event)
        == "OrderFilled(instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, "
        + "venue_order_id=123456, account_id=SIM-000, trade_id=1, position_id=2, order_side=BUY, order_type=LIMIT, "
        + "last_qty=0.561000, last_px=15_600.12445 USDT, commission=12.20000000 USDT, liquidity_side=MAKER, ts_event=0)"
    )
    assert (
        repr(event)
        == "OrderFilled(trader_id=TESTER-001, strategy_id=S-001, instrument_id=ETHUSDT.BINANCE, "
        + "client_order_id=O-20210410-022422-001-001-1, venue_order_id=123456, account_id=SIM-000, trade_id=1, "
        + "position_id=2, order_side=BUY, order_type=LIMIT, last_qty=0.561000, last_px=15_600.12445 USDT, "
        + "commission=12.20000000 USDT, liquidity_side=MAKER, "
        + "event_id=91762096-b188-49ea-8562-8d8a4cc22ff2, ts_event=0, ts_init=0)"
    )


def test_order_initialized():
    event = TestEventsProviderPyo3.order_initialized()
    result_dict = OrderInitialized.to_dict(event)
    order_initialized = OrderInitialized.from_dict(result_dict)
    assert order_initialized == event
    assert (
        str(event)
        == "OrderInitialized(instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, side=BUY, "
        + "type=LIMIT, quantity=0.561000, time_in_force=DAY, post_only=true, reduce_only=true, quote_quantity=false, "
        + "price=1520.10, emulation_trigger=BID_ASK, trigger_instrument_id=ETHUSDT.BINANCE, "
        + "contingency_type=OTO, order_list_id=1, linked_order_ids=[O-2020872378424], parent_order_id=None, "
        + "exec_algorithm_id=None, exec_algorithm_params=None, exec_spawn_id=None, tags=ENTRY)"
    )
    assert (
        repr(event)
        == "OrderInitialized(trader_id=TESTER-001, strategy_id=S-001, instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, "
        + "side=BUY, type=LIMIT, quantity=0.561000, time_in_force=DAY, post_only=true, reduce_only=true, "
        + "quote_quantity=false, price=1520.10, emulation_trigger=BID_ASK, trigger_instrument_id=ETHUSDT.BINANCE, "
        + "contingency_type=OTO, order_list_id=1, linked_order_ids=[O-2020872378424], "
        + "parent_order_id=None, exec_algorithm_id=None, exec_algorithm_params=None, exec_spawn_id=None, "
        + "tags=ENTRY, event_id=91762096-b188-49ea-8562-8d8a4cc22ff2, ts_init=0)"
    )


def test_order_rejected():
    event = TestEventsProviderPyo3.order_rejected_insufficient_margin()
    result_dict = OrderRejected.to_dict(event)
    order_denied = OrderRejected.from_dict(result_dict)
    assert order_denied == event
    assert (
        str(event)
        == "OrderRejected(instrument_id=AUD/USD.SIM, client_order_id=O-20210410-022422-001-001-1, "
        + "account_id=SIM-000, reason='INSUFFICIENT_MARGIN', ts_event=0)"
    )
    assert (
        repr(event)
        == "OrderRejected(trader_id=TESTER-001, strategy_id=S-001, "
        + "instrument_id=AUD/USD.SIM, client_order_id=O-20210410-022422-001-001-1, account_id=SIM-000, "
        + "reason='INSUFFICIENT_MARGIN', event_id=91762096-b188-49ea-8562-8d8a4cc22ff2, ts_event=0, ts_init=0)"
    )


def test_order_triggered():
    event = TestEventsProviderPyo3.order_triggered()
    result_dict = OrderTriggered.to_dict(event)
    order_triggered = OrderTriggered.from_dict(result_dict)
    assert order_triggered == event
    assert (
        str(event)
        == "OrderTriggered(instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, "
        + "venue_order_id=123456, account_id=SIM-000, ts_event=0)"
    )
    assert (
        repr(event)
        == "OrderTriggered(trader_id=TESTER-001, strategy_id=S-001, instrument_id=ETHUSDT.BINANCE, "
        + "client_order_id=O-20210410-022422-001-001-1, venue_order_id=123456, account_id=SIM-000, "
        + "event_id=91762096-b188-49ea-8562-8d8a4cc22ff2, ts_event=0, ts_init=0)"
    )


def test_order_submitted():
    event = TestEventsProviderPyo3.order_submitted()
    result_dict = OrderSubmitted.to_dict(event)
    order_submitted = OrderSubmitted.from_dict(result_dict)
    assert order_submitted == event
    assert (
        str(event)
        == "OrderSubmitted(instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, account_id=SIM-000, ts_event=0)"
    )
    assert (
        repr(event)
        == "OrderSubmitted(trader_id=TESTER-001, strategy_id=S-001, instrument_id=ETHUSDT.BINANCE, "
        + "client_order_id=O-20210410-022422-001-001-1, account_id=SIM-000, "
        + "event_id=91762096-b188-49ea-8562-8d8a4cc22ff2, ts_event=0, ts_init=0)"
    )


def test_order_emulated():
    event = TestEventsProviderPyo3.order_emulated()
    result_dict = OrderEmulated.to_dict(event)
    order_emulated = OrderEmulated.from_dict(result_dict)
    assert order_emulated == event
    assert (
        str(event)
        == "OrderEmulated(instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1)"
    )
    assert (
        repr(event)
        == "OrderEmulated(trader_id=TESTER-001, strategy_id=S-001, instrument_id=ETHUSDT.BINANCE, "
        + "client_order_id=O-20210410-022422-001-001-1, "
        + "event_id=91762096-b188-49ea-8562-8d8a4cc22ff2, ts_init=0)"
    )


def test_order_released():
    event = TestEventsProviderPyo3.order_released()
    result_dict = OrderReleased.to_dict(event)
    order_released = OrderReleased.from_dict(result_dict)
    assert order_released == event
    assert (
        str(event)
        == "OrderReleased(instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, released_price=22_000.0)"
    )
    assert (
        repr(event)
        == "OrderReleased(trader_id=TESTER-001, strategy_id=S-001, instrument_id=ETHUSDT.BINANCE, "
        + "client_order_id=O-20210410-022422-001-001-1, released_price=22_000.0, event_id=91762096-b188-49ea-8562-8d8a4cc22ff2, ts_init=0)"
    )


def test_order_updated():
    event = TestEventsProviderPyo3.order_updated()
    result_dict = OrderUpdated.to_dict(event)
    order_updated = OrderUpdated.from_dict(result_dict)
    assert order_updated == event
    assert (
        str(event)
        == "OrderUpdated(instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, venue_order_id=123456, "
        + "account_id=SIM-000, quantity=1.5, price=1_500.0, trigger_price=None, ts_event=0)"
    )
    assert (
        repr(event)
        == "OrderUpdated(trader_id=TESTER-001, strategy_id=S-001, instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, "
        + "venue_order_id=123456, account_id=SIM-000, quantity=1.5, price=1_500.0, trigger_price=None, "
        + "event_id=91762096-b188-49ea-8562-8d8a4cc22ff2, ts_event=0, ts_init=0)"
    )


def test_order_pending_update():
    event = TestEventsProviderPyo3.order_pending_update()
    result_dict = OrderPendingUpdate.to_dict(event)
    order_pending_update = OrderPendingUpdate.from_dict(result_dict)
    assert order_pending_update == event
    assert (
        str(event)
        == "OrderPendingUpdate(instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, "
        + "venue_order_id=123456, account_id=SIM-000, ts_event=0)"
    )
    assert (
        repr(event)
        == "OrderPendingUpdate(trader_id=TESTER-001, strategy_id=S-001, instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, "
        + "venue_order_id=123456, account_id=SIM-000, event_id=91762096-b188-49ea-8562-8d8a4cc22ff2, ts_event=0, ts_init=0)"
    )


def test_order_pending_cancel():
    event = TestEventsProviderPyo3.order_pending_cancel()
    result_dict = OrderPendingCancel.to_dict(event)
    order_pending_update = OrderPendingCancel.from_dict(result_dict)
    assert order_pending_update == event
    assert (
        str(event)
        == "OrderPendingCancel(instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, "
        + "venue_order_id=123456, account_id=SIM-000, ts_event=0)"
    )
    assert (
        repr(event)
        == "OrderPendingCancel(trader_id=TESTER-001, strategy_id=S-001, instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, "
        + "venue_order_id=123456, account_id=SIM-000, event_id=91762096-b188-49ea-8562-8d8a4cc22ff2, ts_event=0, ts_init=0)"
    )


def test_order_modified_rejected():
    event = TestEventsProviderPyo3.order_modified_rejected()
    result_dict = OrderModifyRejected.to_dict(event)
    order_modified_rejected = OrderModifyRejected.from_dict(result_dict)
    assert order_modified_rejected == event
    assert (
        str(event)
        == "OrderModifyRejected(instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, venue_order_id=123456, "
        + "account_id=SIM-000, reason='ORDER_DOES_NOT_EXIST', ts_event=0)"
    )
    assert (
        repr(event)
        == "OrderModifyRejected(trader_id=TESTER-001, strategy_id=S-001, instrument_id=ETHUSDT.BINANCE, "
        + "client_order_id=O-20210410-022422-001-001-1, venue_order_id=123456, account_id=SIM-000, "
        + "reason='ORDER_DOES_NOT_EXIST', event_id=91762096-b188-49ea-8562-8d8a4cc22ff2, ts_event=0, ts_init=0)"
    )


def test_order_accepted():
    event = TestEventsProviderPyo3.order_accepted()
    result_dict = OrderAccepted.to_dict(event)
    order_accepted = OrderAccepted.from_dict(result_dict)
    assert order_accepted == event
    assert (
        str(event)
        == "OrderAccepted(instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, "
        + "venue_order_id=123456, account_id=SIM-000, ts_event=0)"
    )
    assert (
        repr(event)
        == "OrderAccepted(trader_id=TESTER-001, strategy_id=S-001, instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, "
        + "venue_order_id=123456, account_id=SIM-000, event_id=91762096-b188-49ea-8562-8d8a4cc22ff2, ts_event=0, ts_init=0)"
    )


def test_order_cancel_rejected():
    event = TestEventsProviderPyo3.order_cancel_rejected()
    result_dict = OrderCancelRejected.to_dict(event)
    order_cancel_rejected = OrderCancelRejected.from_dict(result_dict)
    assert order_cancel_rejected == event
    assert (
        str(event)
        == "OrderCancelRejected(instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, venue_order_id=123456, "
        + "account_id=SIM-000, reason='ORDER_DOES_NOT_EXIST', ts_event=0)"
    )
    assert (
        repr(event)
        == "OrderCancelRejected(trader_id=TESTER-001, strategy_id=S-001, instrument_id=ETHUSDT.BINANCE, "
        + "client_order_id=O-20210410-022422-001-001-1, venue_order_id=123456, account_id=SIM-000, "
        + "reason='ORDER_DOES_NOT_EXIST', event_id=91762096-b188-49ea-8562-8d8a4cc22ff2, ts_event=0, ts_init=0)"
    )


def test_order_canceled():
    event = TestEventsProviderPyo3.order_canceled()
    result_dict = OrderCanceled.to_dict(event)
    order_canceled = OrderCanceled.from_dict(result_dict)
    assert order_canceled == event
    assert (
        str(event)
        == "OrderCanceled(instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, venue_order_id=123456, "
        + "account_id=SIM-000, ts_event=0)"
    )
    assert (
        repr(event)
        == "OrderCanceled(trader_id=TESTER-001, strategy_id=S-001, instrument_id=ETHUSDT.BINANCE, "
        + "client_order_id=O-20210410-022422-001-001-1, venue_order_id=123456, account_id=SIM-000, "
        + "event_id=91762096-b188-49ea-8562-8d8a4cc22ff2, ts_event=0, ts_init=0)"
    )


def test_order_expired():
    event = TestEventsProviderPyo3.order_expired()
    result_dict = OrderExpired.to_dict(event)
    order_expired = OrderExpired.from_dict(result_dict)
    assert order_expired == event
    assert (
        str(event)
        == "OrderExpired(instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, "
        + "venue_order_id=123456, account_id=SIM-000, ts_event=0)"
    )
    assert (
        repr(event)
        == "OrderExpired(trader_id=TESTER-001, strategy_id=S-001, instrument_id=ETHUSDT.BINANCE, client_order_id=O-20210410-022422-001-001-1, "
        + "venue_order_id=123456, account_id=SIM-000, event_id=91762096-b188-49ea-8562-8d8a4cc22ff2, ts_event=0, ts_init=0)"
    )
