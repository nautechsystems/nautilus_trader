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
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.events cimport AccountState
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

        cdef list orders_all = [o.to_dict() for o in orders]

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

        cdef list filled_orders = [o.to_dict() for o in orders if o.state == OrderState.FILLED]
        if not filled_orders:
            return pd.DataFrame()

        report = pd.DataFrame(data=filled_orders).set_index("client_order_id").sort_index()
        report["timestamp_ns"] = [nanos_to_unix_dt(row) for row in report["timestamp_ns"]]
        report["ts_filled_ns"] = [nanos_to_unix_dt(row) for row in report["ts_filled_ns"]]
        report.rename(
            columns={
                "timestamp_ns": "timestamp",
                "ts_filled_ns": "ts_filled",
            },
            inplace=True,
        )

        return report

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

        cdef list trades = [p.to_dict() for p in positions if p.is_closed]
        if not trades:
            return pd.DataFrame()

        sort = ["ts_opened_ns", "ts_closed_ns", "position_id"]
        report = pd.DataFrame(data=trades).set_index("position_id").sort_values(sort)
        del report["net_qty"]
        del report["quantity"]
        del report["quote_currency"]
        del report["base_currency"]
        del report["cost_currency"]
        report["ts_opened_ns"] = [nanos_to_unix_dt(row) for row in report["ts_opened_ns"]]
        report["ts_closed_ns"] = [nanos_to_unix_dt(row) for row in report["ts_closed_ns"]]
        report["duration_ns"] = [nanos_to_timedelta(row) for row in report["duration_ns"]]
        report.rename(
            columns={
                "ts_opened_ns": "ts_opened",
                "ts_closed_ns": "ts_closed",
                "duration_ns": "duration",
            },
            inplace=True,
        )

        return report

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

        cdef list states = account.events_c()

        if not states:
            return pd.DataFrame()

        cdef list account_states = [AccountState.to_dict_c(s) for s in states]

        if not account_states:
            return pd.DataFrame()

        report = pd.DataFrame(data=account_states).set_index("ts_updated_ns").sort_index()
        report.index = [nanos_to_unix_dt(row) for row in report.index]
        report.index.rename("timestamp", inplace=True)
        del report["timestamp_ns"]
        del report["type"]
        del report["event_id"]

        return report
