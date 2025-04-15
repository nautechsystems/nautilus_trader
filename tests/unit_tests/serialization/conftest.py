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

from typing import Any

from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.position import Position
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs


def _make_order_events(order, **kwargs):
    submitted = TestEventStubs.order_submitted(order=order)
    order.apply(submitted)
    accepted = TestEventStubs.order_accepted(order=order)
    order.apply(accepted)
    filled = TestEventStubs.order_filled(order=order, **kwargs)
    return submitted, accepted, filled


def nautilus_objects() -> list[Any]:
    """
    Return a list of nautilus instances for testing serialization.
    """
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
    position_id = PositionId("P-001")
    buy = TestExecStubs.limit_order(instrument)
    buy_submitted, buy_accepted, buy_filled = _make_order_events(
        buy,
        instrument=instrument,
        position_id=position_id,
        trade_id=TradeId("BUY"),
    )
    sell = TestExecStubs.limit_order(order_side=OrderSide.SELL)
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
        # DATA
        TestDataStubs.quote_tick(),
        TestDataStubs.trade_tick(),
        TestDataStubs.bar_5decimal(),
        TestDataStubs.mark_price(),
        TestDataStubs.index_price(),
        TestDataStubs.instrument_status(),
        TestDataStubs.instrument_close(),
        # EVENTS
        TestEventStubs.component_state_changed(),
        TestEventStubs.trading_state_changed(),
        TestEventStubs.betting_account_state(),
        TestEventStubs.cash_account_state(),
        TestEventStubs.margin_account_state(),
        # ORDERS
        TestEventStubs.order_accepted(buy),
        TestEventStubs.order_rejected(buy),
        TestEventStubs.order_pending_update(buy_accepted),
        TestEventStubs.order_pending_cancel(buy_accepted),
        TestEventStubs.order_filled(
            order=buy,
            instrument=instrument,
            position_id=open_position.id,
        ),
        TestEventStubs.order_canceled(buy_accepted),
        TestEventStubs.order_expired(buy),
        TestEventStubs.order_triggered(buy),
        # POSITIONS
        TestEventStubs.position_opened(open_position),
        TestEventStubs.position_changed(open_position),
        TestEventStubs.position_closed(closed_position),
    ]
