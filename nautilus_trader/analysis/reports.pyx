# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport nanos_to_timedelta
from nautilus_trader.core.datetime cimport nanos_to_unix_dt
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.order_type cimport OrderTypeParser
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.objects cimport AccountBalance
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.account cimport Account


cdef class ReportProvider:
    """
    Provides various trading reports.
    """

    def __init__(self):
        """
        Initialize a new instance of the ``ReportProvider`` class.
        """

    cpdef object generate_orders_report(self, list orders):
        """
        Return an orders report dataframe.

        Parameters
        ----------
        orders : dict[VenueOrderId, Order]
            The orders for the report.

        Returns
        -------
        pd.DataFrame

        """
        Condition.not_none(orders, "orders")

        if not orders:
            return pd.DataFrame()

        cdef list orders_all = [self._order_to_dict(o) for o in orders]

        return pd.DataFrame(data=orders_all).set_index("client_order_id").sort_index()

    cpdef object generate_order_fills_report(self, list orders):
        """
        Return an order fills report dataframe.

        Parameters
        ----------
        orders : dict[VenueOrderId, Order]
            The orders for the report.

        Returns
        -------
        pd.DataFrame

        """
        Condition.not_none(orders, "orders")

        if not orders:
            return pd.DataFrame()

        cdef list filled_orders = [
            self._order_to_dict(o) for o in orders if o.state == OrderState.FILLED
        ]

        if not filled_orders:
            return pd.DataFrame()

        return pd.DataFrame(data=filled_orders).set_index("client_order_id").sort_index()

    cpdef object generate_positions_report(self, list positions):
        """
        Return a positions report dataframe.

        Parameters
        ----------
        positions : dict[PositionId, Position]
            The positions for the report.

        Returns
        -------
        pd.DataFrame

        """
        Condition.not_none(positions, "positions")

        if not positions:
            return pd.DataFrame()

        cdef list trades = [self._position_to_dict(p) for p in positions if p.is_closed]

        if not trades:
            return pd.DataFrame()

        return pd.DataFrame(data=trades).set_index("position_id").sort_values(
            ['opened_time', 'closed_time', 'position_id'],
        )

    cpdef object generate_account_report(self, Account account):
        """
        Generate an account report for the given optional time range.

        Parameters
        ----------
        account : Account
            The account for the report.

        Returns
        -------
        pd.DataFrame

        """
        Condition.not_none(account, "account")

        cdef list events = account.events_c()

        if not events:
            return pd.DataFrame()

        cdef list account_events = [self._account_state_to_dict(e) for e in events]

        if not account_events:
            return pd.DataFrame()

        return pd.DataFrame(data=account_events).set_index("timestamp").sort_index()

    cdef dict _order_to_dict(self, Order order):
        return {
            "client_order_id": order.client_order_id.value,
            "venue_order_id": order.venue_order_id.value,
            "instrument_id": order.instrument_id.value,
            "side": OrderSideParser.to_str(order.side),
            "type": OrderTypeParser.to_str(order.type),
            "qty": order.quantity,
            "avg_px": "None" if order.avg_px is None else float(order.avg_px),
            "slippage": float(order.slippage),
            "timestamp": nanos_to_unix_dt(order.last_event_c().timestamp_ns),
        }

    cdef dict _position_to_dict(self, Position position):
        return {
            "position_id": position.id.value,
            "instrument_id": position.instrument_id.value,
            "strategy_id": position.strategy_id.value,
            "entry": OrderSideParser.to_str(position.entry),
            "peak_qty": position.peak_qty,
            "opened_time": nanos_to_unix_dt(position.opened_timestamp_ns),
            "closed_time": nanos_to_unix_dt(position.closed_timestamp_ns),
            "duration": nanos_to_timedelta(position.open_duration_ns),
            "avg_px_open": float(position.avg_px_open),
            "avg_px_close": float(position.avg_px_close),
            "realized_points": float(position.realized_points),
            "realized_return": float(position.realized_return),
            "realized_pnl": float(position.realized_pnl),
            "currency": str(position.quote_currency),
        }

    cdef dict _account_state_to_dict(self, AccountState event):
        cdef dict data = {"timestamp": nanos_to_unix_dt(event.timestamp_ns)}
        cdef AccountBalance balance
        for balance in event.balances:
            data[f"balance_{balance.currency}"] = balance.total

        return data
