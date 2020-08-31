# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import pandas as pd

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.currency cimport currency_to_string
from nautilus_trader.model.c_enums.order_side cimport order_side_to_string
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.order_type cimport order_type_to_string
from nautilus_trader.model.events cimport AccountStateEvent
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.position cimport Position


cdef class ReportProvider:
    """
    Provides various trading reports.
    """

    def __init__(self):
        """
        Initialize a new instance of the ReportProvider class.
        """

    cpdef object generate_orders_report(self, dict orders: {OrderId, Order}):
        """
        Return an orders report dataframe.

        :param orders: The dictionary of order_ids and order objects.
        :return pd.DataFrame.
        """
        Condition.not_none(orders, "orders")

        if not orders:
            return pd.DataFrame()

        cdef list orders_all = [self._order_to_dict(o) for o in orders.values()]

        return pd.DataFrame(data=orders_all).set_index("order_id")

    cpdef object generate_order_fills_report(self, dict orders: {OrderId, Order}):
        """
        Return an order fills report dataframe.

        :param orders: The dictionary of order_ids and order objects.
        :return pd.DataFrame.
        """
        Condition.not_none(orders, "orders")

        if not orders:
            return pd.DataFrame()

        cdef list filled_orders = [
            self._order_to_dict(o) for o in orders.values() if o.state() == OrderState.FILLED
        ]

        return pd.DataFrame(data=filled_orders).set_index("order_id")

    cpdef object generate_positions_report(self, dict positions: {PositionId, Position}):
        """
        Return a positions report dataframe.

        :param positions: The dictionary of position_ids and objects.
        :return pd.DataFrame.
        """
        Condition.not_none(positions, "positions")

        if not positions:
            return pd.DataFrame()

        cdef list trades = [self._position_to_dict(p) for p in positions.values() if p.is_closed()]

        return pd.DataFrame(data=trades).set_index("position_id")

    cpdef object generate_account_report(self, list events, datetime start=None, datetime end=None):
        """
        Generate an account report for the given optional time range.

        :param events: The accounts state events list.
        :param start: The start of the account reports period.
        :param end: The end of the account reports period.
        :return: pd.DataFrame.
        """
        Condition.not_none(events, "events")

        if start is None:
            start = events[0].timestamp
        if end is None:
            end = events[-1].timestamp

        cdef list account_events = [self._account_state_to_dict(e) for e in events
                                    if start <= e.timestamp <= end]

        if not account_events:
            return pd.DataFrame()

        return pd.DataFrame(data=account_events).set_index("timestamp")

    cdef dict _order_to_dict(self, Order order):
        return {"order_id": order.id.value,
                "symbol": order.symbol.code,
                "side": order_side_to_string(order.side),
                "type": order_type_to_string(order.type),
                "quantity": order.quantity,
                "avg_price": "None" if order.average_price is None
                else order.average_price.as_double(),
                "slippage": order.slippage.as_double(),
                "timestamp": order.last_event().timestamp}

    cdef dict _position_to_dict(self, Position position):
        return {"position_id": position.id.value,
                "symbol": position.symbol.code,
                "direction": order_side_to_string(position.entry_direction),
                "peak_quantity": position.peak_quantity,
                "opened_time": position.opened_time,
                "closed_time": position.closed_time,
                "duration": position.open_duration,
                "avg_open_price": position.average_open_price,
                "avg_close_price": position.average_close_price,
                "realized_points": position.realized_points,
                "realized_return": position.realized_return,
                "realized_pnl": position.realized_pnl.as_double(),
                "currency": currency_to_string(position.quote_currency)}

    cdef dict _account_state_to_dict(self, AccountStateEvent event):
        return {"timestamp": event.timestamp,
                "cash_balance": event.cash_balance.as_double(),
                "margin_used": event.margin_used_maintenance.as_double()}
