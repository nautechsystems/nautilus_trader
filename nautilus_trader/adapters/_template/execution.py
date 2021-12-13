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
from typing import List

from nautilus_trader.execution.messages import ExecutionReport
from nautilus_trader.execution.messages import OrderStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.commands.trading import CancelAllOrders
from nautilus_trader.model.commands.trading import CancelOrder
from nautilus_trader.model.commands.trading import ModifyOrder
from nautilus_trader.model.commands.trading import SubmitOrder
from nautilus_trader.model.commands.trading import SubmitOrderList
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.orders.base import Order


# The 'pragma: no cover' comment excludes a method from test coverage.
# https://coverage.readthedocs.io/en/coverage-4.3.3/excluding.html
# The reason for their use is to reduce redundant/needless tests which simply
# assert that a `NotImplementedError` is raised when calling abstract methods.
# These tests are expensive to maintain (as they must be kept in line with any
# refactorings), and offer little to no benefit in return. However, the intention
# is for all method implementations to be fully covered by tests.

# *** THESE PRAGMA: NO COVER COMMENTS MUST BE REMOVED IN ANY IMPLEMENTATION. ***


class TemplateLiveExecutionClient(LiveExecutionClient):
    """
    An example of a ``LiveExecutionClient`` highlighting the method requirements.

    +------------------------------+-------------+
    | Method                       | Requirement |
    +------------------------------+-------------+
    | connect                      | required    |
    | disconnect                   | required    |
    | reset                        | optional    |
    | dispose                      | optional    |
    +--------------------------------------------+
    | submit_order                 | required    |
    | submit_order_list            | required    |
    | modify_order                 | required    |
    | cancel_order                 | required    |
    | cancel_all_orders            | required    |
    | generate_order_status_report | required    |
    | generate_order_status_report | required    |
    | generate_exec_reports        | required    |
    +--------------------------------------------+

    """

    def connect(self) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def disconnect(self) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def reset(self) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def dispose(self) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    # -- COMMAND HANDLERS --------------------------------------------------------------------------

    def submit_order(self, command: SubmitOrder) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def submit_order_list(self, command: SubmitOrderList) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def modify_order(self, command: ModifyOrder) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def cancel_order(self, command: CancelOrder) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def cancel_all_orders(self, command: CancelAllOrders) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    # -- RECONCILIATION ----------------------------------------------------------------------------

    async def generate_order_status_report(self, order: Order) -> OrderStatusReport:
        """
        Generate an order status report for the given order.

        If an error occurs then logs and returns ``None``.

        Parameters
        ----------
        order : Order
            The order for the report.

        Returns
        -------
        OrderStatusReport or ``None``

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def generate_exec_reports(
        self,
        venue_order_id: VenueOrderId,
        symbol: Symbol,
        since: datetime = None,
    ) -> List[ExecutionReport]:
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
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover
