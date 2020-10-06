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
from nautilus_trader.model.c_enums.order_side cimport order_side_to_string
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.order_type cimport order_type_to_string
from nautilus_trader.model.events cimport AccountState
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

    cpdef object generate_orders_report(self, list orders):
        """
        Return an orders report dataframe.

        Parameters
        ----------
        orders : Dict[OrderId, Order]
            The dictionary of client order identifiers and order objects.

        Returns
        -------
        pd.DataFrame

        """
        Condition.not_none(orders, "orders")

        if not orders:
            return pd.DataFrame()

        cdef list orders_all = [self._order_to_dict(o) for o in orders]

        return pd.DataFrame(data=orders_all).set_index("cl_ord_id")

    cpdef object generate_order_fills_report(self, list orders):
        """
        Return an order fills report dataframe.

        Parameters
        ----------
        orders : Dict[OrderId, Order]
            The dictionary of client order identifiers and order objects.

        Returns
        -------
        pd.DataFrame

        """
        Condition.not_none(orders, "orders")

        if not orders:
            return pd.DataFrame()

        cdef list filled_orders = [
            self._order_to_dict(o) for o in orders if o.state() == OrderState.FILLED
        ]

        if not filled_orders:
            return pd.DataFrame.empty

        return pd.DataFrame(data=filled_orders).set_index("cl_ord_id")

    cpdef object generate_positions_report(self, list positions):
        """
        Return a positions report dataframe.

        Parameters
        ----------
        positions : Dict[PositionId, Position]

        Returns
        -------
        pd.DataFrame

        """
        Condition.not_none(positions, "positions")

        if not positions:
            return pd.DataFrame()

        cdef list trades = [self._position_to_dict(p) for p in positions if p.is_closed()]

        if not trades:
            return pd.DataFrame.empty

        return pd.DataFrame(data=trades).set_index("position_id")

    cpdef object generate_account_report(self, list events, datetime start=None, datetime end=None):
        """
        Generate an account report for the given optional time range.

        Parameters
        ----------
        events : List[AccountState]
            The accounts state events list.
        start : datetime, optional
            The start of the account reports period.
        end : datetime, optional
            The end of the account reports period.

        Returns
        -------
        pd.DataFrame

        """
        Condition.not_none(events, "events")

        if start is None:
            start = events[0].timestamp
        if end is None:
            end = events[-1].timestamp

        cdef list account_events = [
            self._account_state_to_dict(e) for e in events if start <= e.timestamp <= end
        ]

        if not account_events:
            return pd.DataFrame.empty

        return pd.DataFrame(data=account_events).set_index("timestamp")

    cdef dict _order_to_dict(self, Order order):
        return {
            "cl_ord_id": order.cl_ord_id.value,
            "order_id": order.id.value,
            "symbol": order.symbol.code,
            "side": order_side_to_string(order.side),
            "type": order_type_to_string(order.type),
            "quantity": order.quantity,
            "avg_price": "None" if order.avg_price is None
            else order.avg_price,
            "slippage": order.slippage.as_double(),
            "timestamp": order.last_event().timestamp,
        }

    cdef dict _position_to_dict(self, Position position):
        return {
            "position_id": position.id.value,
            "symbol": position.symbol.code,
            "entry": order_side_to_string(position.entry),
            "peak_quantity": position.peak_quantity,
            "opened_time": position.opened_time,
            "closed_time": position.closed_time,
            "duration": position.open_duration,
            "avg_open_price": position.avg_open_price,
            "avg_close_price": position.avg_close_price,
            "realized_points": position.realized_points,
            "realized_return": position.realized_return,
            "realized_pnl": position.realized_pnl.as_double(),
            "currency": position.quote_currency.code,
        }

    cdef dict _account_state_to_dict(self, AccountState event):
        return {
            "timestamp": event.timestamp,
            "cash_balance": event.cash_balance.as_double(),
            "margin_balance": event.margin_balance.as_double(),
            "margin_available": event.margin_balance.as_double(),
        }
