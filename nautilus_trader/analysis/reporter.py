# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import List

import msgspec
import pandas as pd

from nautilus_trader.accounting.accounts.base import Account
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.orders.base import Order
from nautilus_trader.model.position import Position


class ReportProvider:
    """
    Provides various portfolio analysis reports.
    """

    @staticmethod
    def generate_orders_report(orders: List[Order]) -> pd.DataFrame:
        """
        Generate an orders report.

        Parameters
        ----------
        orders : list[Order]
            The orders for the report.

        Returns
        -------
        pd.DataFrame

        """
        if not orders:
            return pd.DataFrame()

        orders_all = [o.to_dict() for o in orders]

        return pd.DataFrame(data=orders_all).set_index("client_order_id").sort_index()

    @staticmethod
    def generate_order_fills_report(orders: List[Order]) -> pd.DataFrame:
        """
        Generate an order fills report.

        Parameters
        ----------
        orders : list[Order]
            The orders for the report.

        Returns
        -------
        pd.DataFrame

        """
        if not orders:
            return pd.DataFrame()

        filled_orders = [o.to_dict() for o in orders if o.status == OrderStatus.FILLED]
        if not filled_orders:
            return pd.DataFrame()

        report = pd.DataFrame(data=filled_orders).set_index("client_order_id").sort_index()
        report["ts_last"] = [unix_nanos_to_dt(ts_last) for ts_last in report["ts_last"]]
        report["ts_init"] = [unix_nanos_to_dt(ts_init) for ts_init in report["ts_init"]]

        return report

    @staticmethod
    def generate_positions_report(positions: List[Position]) -> pd.DataFrame:
        """
        Generate a positions report.

        Parameters
        ----------
        positions : list[Position]
            The positions for the report.

        Returns
        -------
        pd.DataFrame

        """
        if not positions:
            return pd.DataFrame()

        trades = [p.to_dict() for p in positions if p.is_closed]
        if not trades:
            return pd.DataFrame()

        sort = ["ts_opened", "ts_closed", "position_id"]
        report = pd.DataFrame(data=trades).set_index("position_id").sort_values(sort)
        del report["net_qty"]
        del report["quantity"]
        del report["quote_currency"]
        del report["base_currency"]
        del report["cost_currency"]
        report["ts_opened"] = [unix_nanos_to_dt(ts_opened) for ts_opened in report["ts_opened"]]
        report["ts_closed"] = [unix_nanos_to_dt(ts_closed) for ts_closed in report["ts_closed"]]

        return report

    @staticmethod
    def generate_account_report(account: Account) -> pd.DataFrame:
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
        states = account.events

        if not states:
            return pd.DataFrame()

        account_states = [AccountState.to_dict(s) for s in states]
        balances = [
            {**balance, **state}
            for state in account_states
            for balance in msgspec.json.decode(state.pop("balances", "[]"))
        ]

        if not account_states:
            return pd.DataFrame()

        report = pd.DataFrame(data=balances).set_index("ts_event").sort_index()
        report.index = [unix_nanos_to_dt(ts_event) for ts_event in report.index]
        del report["ts_init"]
        del report["type"]
        del report["event_id"]

        return report
