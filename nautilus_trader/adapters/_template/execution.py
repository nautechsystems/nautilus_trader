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


from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient


# The 'pragma: no cover' comment excludes a method from test coverage.
# https://coverage.readthedocs.io/en/coverage-4.3.3/excluding.html
# The reason for their use is to reduce redundant/needless tests which simply
# assert that a `NotImplementedError` is raised when calling abstract methods.
# These tests are expensive to maintain (as they must be kept in line with any
# refactorings), and offer little to no benefit in return. The intention
# is for all method implementations to be fully covered by tests.

# *** THESE PRAGMA: NO COVER COMMENTS MUST BE REMOVED IN ANY IMPLEMENTATION. ***


class TemplateLiveExecutionClient(LiveExecutionClient):
    """
    An example of a ``LiveExecutionClient`` highlighting the method requirements.

    +--------------------------------------------+-------------+
    | Method                                     | Requirement |
    +--------------------------------------------+-------------+
    | _connect                                   | required    |
    | _disconnect                                | required    |
    | reset                                      | optional    |
    | dispose                                    | optional    |
    +--------------------------------------------+-------------+
    | _submit_order                              | required    |
    | _submit_order_list                         | required    |
    | _modify_order                              | optional    |
    | _cancel_order                              | required    |
    | _cancel_all_orders                         | required    |
    | _batch_cancel_orders                       | optional    |
    | generate_order_status_report               | required    |
    | generate_order_status_reports              | required    |
    | generate_fill_reports                      | required    |
    | generate_position_status_reports           | required    |
    +--------------------------------------------+-------------+

    """

    async def _connect(self) -> None:
        raise NotImplementedError(
            "method `_connect` must be implemented in the subclass",
        )  # pragma: no cover

    async def _disconnect(self) -> None:
        raise NotImplementedError(
            "method `_disconnect` must be implemented in the subclass",
        )  # pragma: no cover

    def reset(self) -> None:
        raise NotImplementedError(
            "method `reset` must be implemented in the subclass",
        )  # pragma: no cover

    def dispose(self) -> None:
        raise NotImplementedError(
            "method `dispose` must be implemented in the subclass",
        )  # pragma: no cover

    # -- EXECUTION REPORTS ------------------------------------------------------------------------

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        raise NotImplementedError(
            "method `generate_order_status_report` must be implemented in the subclass",
        )  # pragma: no cover

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        raise NotImplementedError(
            "method `generate_order_status_reports` must be implemented in the subclass",
        )  # pragma: no cover

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        raise NotImplementedError(
            "method `generate_fill_reports` must be implemented in the subclass",
        )  # pragma: no cover

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        raise NotImplementedError(
            "method `generate_position_status_reports` must be implemented in the subclass",
        )  # pragma: no cover

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    async def _submit_order(self, command: SubmitOrder) -> None:
        raise NotImplementedError(
            "method `_submit_order` must be implemented in the subclass",
        )  # pragma: no cover

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        raise NotImplementedError(
            "method `_submit_order_list` must be implemented in the subclass",
        )  # pragma: no cover

    async def _modify_order(self, command: ModifyOrder) -> None:
        raise NotImplementedError(
            "method `_modify_order` must be implemented in the subclass",
        )  # pragma: no cover

    async def _cancel_order(self, command: CancelOrder) -> None:
        raise NotImplementedError(
            "method `_cancel_order` must be implemented in the subclass",
        )  # pragma: no cover

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        raise NotImplementedError(
            "method `_cancel_all_orders` must be implemented in the subclass",
        )  # pragma: no cover

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        raise NotImplementedError(
            "method `_batch_cancel_orders` must be implemented in the subclass",
        )  # pragma: no cover

    async def _query_order(self, command: QueryOrder) -> None:
        raise NotImplementedError(
            "method `_query_order` must be implemented in the subclass",
        )  # pragma: no cover
