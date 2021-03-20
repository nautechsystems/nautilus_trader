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

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.objects cimport Quantity


cdef class ExecutionStateReport:
    """
    Represents a report of execution state by order identifier.
    """

    def __init__(
        self,
        str client not None,
        AccountId account_id not None,
        datetime timestamp not None,
    ):
        """
        Initializes a new instance of the `ExecutionStateReport` class.

        Parameters
        ----------
        client : str
            The client name for the report.
        account_id : AccountId
            The account identifier for the report.
        timestamp : datetime
            The report timestamp.

        Raises
        ------
        ValueError
            If client is not a valid string.

        """
        Condition.valid_string(client, "client")

        self.client = client
        self.account_id = account_id
        self.timestamp = timestamp

        self._order_states = {}     # type: dict[ClientOrderId, OrderStateReport]
        self._position_states = {}  # type: dict[InstrumentId, PositionStateReport]

    cpdef dict order_states(self):
        """
        Return the order state reports.

        Returns
        -------
        dict[ClientOrderId, OrderStateReport]

        """
        return self._order_states.copy()

    cpdef dict position_states(self):
        """
        Return the position state reports.

        Returns
        -------
        dict[InstrumentId, PositionStateReport]

        """
        return self._position_states.copy()

    cpdef void add_order_report(self, OrderStateReport report) except *:
        """
        Add the order state report.

        Parameters
        ----------
        report : OrderStateReport
            The report to add.

        """
        Condition.not_none(report, "report")

        self._order_states[report.cl_ord_id] = report

    cpdef void add_position_report(self, PositionStateReport report) except *:
        """
        Add the position state report.

        Parameters
        ----------
        report : PositionStateReport
            The report to add.

        """
        Condition.not_none(report, "report")

        self._position_states[report.instrument_id] = report


cdef class OrderStateReport:
    """
    Represents an orders state at a point in time.
    """
    def __init__(
        self,
        ClientOrderId cl_ord_id not None,
        OrderId order_id not None,
        OrderState order_state,
        Quantity filled_qty not None,
        datetime timestamp not None,
    ):
        """
        Initializes a new instance of the `OrderStateReport` class.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The reported client order identifier.
        order_id : OrderId
            The reported order identifier.
        order_state : OrderState
            The reported order state at the exchange.
        filled_qty : Quantity
            The reported filled quantity at the exchange.
        timestamp : datetime
            The report timestamp.

        Raises
        ------
        ValueError
            If order_state is UNDEFINED.

        """
        Condition.not_equal(order_state, OrderState.UNDEFINED, "order_state", "UNDEFINED")

        self.cl_ord_id = cl_ord_id
        self.order_id = order_id
        self.order_state = order_state
        self.filled_qty = filled_qty
        self.timestamp = timestamp


cdef class PositionStateReport:
    """
    Represents a positions state at a point in time.
    """
    def __init__(
        self,
        InstrumentId instrument_id not None,
        PositionSide position_side,
        Quantity qty not None,
        datetime timestamp not None,
    ):
        """
        Initializes a new instance of the `PositionStateReport` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The reported instrument identifier.
        position_side : PositionSide
            The reported position side at the exchange.
        qty : Quantity
            The reported position quantity at the exchange.
        timestamp : datetime
            The report timestamp.

        Raises
        ------
        ValueError
            If position_side is UNDEFINED.

        """
        Condition.not_equal(position_side, PositionSide.UNDEFINED, "position_side", "UNDEFINED")

        self.instrument_id = instrument_id
        self.side = position_side
        self.qty = qty
        self.timestamp = timestamp
