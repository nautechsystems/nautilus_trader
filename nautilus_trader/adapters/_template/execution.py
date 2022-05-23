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

from datetime import datetime
from typing import List, Optional

from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import VenueOrderId


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

    +----------------------------------+-------------+
    | Method                           | Requirement |
    +----------------------------------+-------------+
    | connect                          | required    |
    | disconnect                       | required    |
    | reset                            | optional    |
    | dispose                          | optional    |
    +------------------------------------------------+
    | submit_order                     | required    |
    | submit_order_list                | required    |
    | modify_order                     | required    |
    | cancel_order                     | required    |
    | cancel_all_orders                | required    |
    | generate_order_status_report     | required    |
    | generate_order_status_reports    | required    |
    | generate_trade_reports           | required    |
    | generate_position_status_reports | required    |
    +------------------------------------------------+
    """

    def connect(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def disconnect(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def reset(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def dispose(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    # -- EXECUTION REPORTS ------------------------------------------------------------------------

    async def generate_order_status_report(
        self,
        instrument_id: InstrumentId,
        client_order_id: Optional[ClientOrderId] = None,
        venue_order_id: Optional[VenueOrderId] = None,
    ) -> Optional[OrderStatusReport]:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def generate_order_status_reports(
        self,
        instrument_id: InstrumentId = None,
        start: datetime = None,
        end: datetime = None,
        open_only: bool = False,
    ) -> List[OrderStatusReport]:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def generate_trade_reports(
        self,
        instrument_id: InstrumentId = None,
        venue_order_id: VenueOrderId = None,
        start: datetime = None,
        end: datetime = None,
    ) -> List[TradeReport]:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def generate_position_status_reports(
        self,
        instrument_id: InstrumentId = None,
        start: datetime = None,
        end: datetime = None,
    ) -> List[PositionStatusReport]:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    def submit_order(self, command: SubmitOrder) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def submit_order_list(self, command: SubmitOrderList) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def modify_order(self, command: ModifyOrder) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def cancel_order(self, command: CancelOrder) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def cancel_all_orders(self, command: CancelAllOrders) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover
