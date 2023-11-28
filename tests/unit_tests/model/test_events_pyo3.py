# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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


from nautilus_trader.core.nautilus_pyo3 import OrderDenied
from nautilus_trader.core.nautilus_pyo3 import OrderFilled
from nautilus_trader.core.nautilus_pyo3 import OrderInitialized
from nautilus_trader.test_kit.rust.events_pyo3 import TestEventsProviderPyo3


def test_order_denied():
    event = TestEventsProviderPyo3.order_denied_max_submit_rate()
    result_dict = OrderDenied.to_dict(event)
    order_denied = OrderDenied.from_dict(result_dict)
    assert order_denied == event
    assert (
        str(event)
        == "OrderDenied(instrument_id=AUD/USD.SIM, client_order_id=O-20210410-022422-001-001-1, "
        + "reason=Exceeded MAX_ORDER_SUBMIT_RATE)"
    )
    assert (
        repr(event)
        == "OrderDenied(trader_id=TESTER-001, strategy_id=S-001, "
        + "instrument_id=AUD/USD.SIM, client_order_id=O-20210410-022422-001-001-1, "
        + "reason=Exceeded MAX_ORDER_SUBMIT_RATE, event_id=91762096-b188-49ea-8562-8d8a4cc22ff2, ts_init=0)"
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
        + "last_qty=0.561000, last_px=15600.12445 USDT, commission=12.20000000 USDT, liquidity_side=MAKER, ts_event=0)"
    )
    assert (
        repr(event)
        == "OrderFilled(trader_id=TESTER-001, strategy_id=S-001, instrument_id=ETHUSDT.BINANCE, "
        + "client_order_id=O-20210410-022422-001-001-1, venue_order_id=123456, account_id=SIM-000, trade_id=1, "
        + "position_id=2, order_side=BUY, order_type=LIMIT, last_qty=0.561000, last_px=15600.12445 USDT, "
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
