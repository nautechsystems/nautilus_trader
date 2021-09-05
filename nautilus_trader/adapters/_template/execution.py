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

from datetime import datetime

from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.commands.trading import CancelOrder
from nautilus_trader.model.commands.trading import SubmitBracketOrder
from nautilus_trader.model.commands.trading import SubmitOrder
from nautilus_trader.model.commands.trading import UpdateOrder
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.orders.base import Order


class TemplateLiveExecutionClient(LiveExecutionClient):
    def connect(self):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def disconnect(self):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def reset(self):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def dispose(self):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    # -- COMMAND HANDLERS ------------------------------------------------------------------------------

    def submit_order(self, command: SubmitOrder):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def submit_bracket_order(self, command: SubmitBracketOrder):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def update_order(self, command: UpdateOrder):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def cancel_order(self, command: CancelOrder):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    # -- RECONCILIATION ------------------------------------------------------------------------------

    async def generate_order_status_report(self, order: Order):  # pragma: no cover
        """
        Generate an order status report for the given order.

        If an error occurs then logs and returns None.

        Parameters
        ----------
        order : Order
            The order for the report.

        Returns
        -------
        OrderStatusReport or None

        """
        raise NotImplementedError("method must be implemented in the subclass")

    async def generate_exec_reports(  # pragma: no cover
        self, venue_order_id: VenueOrderId, symbol: Symbol, since: datetime = None
    ):
        """
        Generate a list of execution reports.

        The returned list may be empty if no trades match the given parameters.

        Parameters
        ----------
        venue_order_id : VenueOrderId
            The venue order ID for the trades.
        symbol : Symbol
            The symbol for the trades.
        since : datetime, optional
            The timestamp to filter trades on.

        Returns
        -------
        list[ExecutionReport]

        """
        raise NotImplementedError("method must be implemented in the subclass")
