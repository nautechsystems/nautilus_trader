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

from decimal import Decimal

from libc.stdint cimport int64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.objects cimport Quantity


cdef class OrderStatusReport:
    """
    Represents an orders state at a point in time.
    """
    def __init__(
        self,
        ClientOrderId cl_ord_id not None,
        OrderId order_id not None,
        OrderState order_state,
        Quantity filled_qty not None,
        int64_t timestamp_ns,
    ):
        """
        Initializes a new instance of the `OrderStatusReport` class.

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
        timestamp_ns : int64
            The Unix timestamp (nanos) of the report.

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
        self.timestamp_ns = timestamp_ns


cdef class PositionStatusReport:
    """
    Represents a positions state at a point in time.
    """
    def __init__(
        self,
        InstrumentId instrument_id not None,
        PositionSide position_side,
        Quantity qty not None,
        int64_t timestamp_ns,
    ):
        """
        Initializes a new instance of the `PositionStatusReport` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The reported instrument identifier.
        position_side : PositionSide
            The reported position side at the exchange.
        qty : Quantity
            The reported position quantity at the exchange.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the report.

        Raises
        ------
        ValueError
            If position_side is UNDEFINED.

        """
        Condition.not_equal(position_side, PositionSide.UNDEFINED, "position_side", "UNDEFINED")

        self.instrument_id = instrument_id
        self.side = position_side
        self.qty = qty
        self.timestamp_ns = timestamp_ns


cdef class ExecutionReport:
    """
    Represents a report of execution state by order identifier.
    """

    def __init__(
        self,
        ClientOrderId cl_ord_id not None,
        OrderId order_id not None,
        ExecutionId execution_id not None,
        last_qty not None: Decimal,
        last_px not None: Decimal,
        commission_amount: Decimal,  # Can be None
        str commission_currency,     # Can be None
        LiquiditySide liquidity_side,
        int64_t execution_ns,
        int64_t timestamp_ns,
    ):
        """
        Initializes a new instance of the `ExecutionReport` class.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.
        order_id : OrderId
            The order identifier.
        execution_id : ExecutionId
            The execution identifier for the trade.
        last_qty : Decimal
            The quantity of the last fill.
        last_px : Decimal
            The price of the last fill.
        commission_amount : Decimal, optional
            The commission for the transaction (can be None).
        commission_currency : str, optional
            The commission currency for the transaction (can be None).
        liquidity_side : LiquiditySide
            The liquidity side for the fill.
        execution_ns : int64
            The Unix timestamp (nanos) of the execution.

        """
        Condition.type(last_qty, Decimal, "last_qty")
        Condition.type(last_px, Decimal, "last_qty")
        Condition.type_or_none(commission_amount, Decimal, "commission_amount")

        self.cl_ord_id = cl_ord_id
        self.order_id = order_id
        self.id = execution_id
        self.last_qty = last_qty
        self.last_px = last_px
        self.commission_amount = commission_amount
        self.commission_currency = commission_currency
        self.liquidity_side = liquidity_side
        self.execution_ns = execution_ns
        self.timestamp_ns = timestamp_ns


cdef class ExecutionMassStatus:
    """
    Represents a mass status report of execution status.
    """

    def __init__(
        self,
        str client not None,
        AccountId account_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initializes a new instance of the `ExecutionMassStatus` class.

        Parameters
        ----------
        client : str
            The client name for the report.
        account_id : AccountId
            The account identifier for the report.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the report.

        Raises
        ------
        ValueError
            If client is not a valid string.

        """
        Condition.valid_string(client, "client")

        self.client = client
        self.account_id = account_id
        self.timestamp_ns = timestamp_ns

        self._order_reports = {}    # type: dict[OrderId, OrderStatusReport]
        self._exec_reports = {}     # type: dict[OrderId, list[ExecutionReport]]
        self._position_reports = {}  # type: dict[InstrumentId, PositionStatusReport]

    cpdef dict order_reports(self):
        """
        Return the order state reports.

        Returns
        -------
        dict[OrderId, OrderStatusReport]

        """
        return self._order_reports.copy()

    cpdef dict exec_reports(self):
        """
        Return the execution reports.

        Returns
        -------
        dict[OrderId, list[ExecutionReport]

        """
        return self._exec_reports.copy()

    cpdef dict position_reports(self):
        """
        Return the position state reports.

        Returns
        -------
        dict[InstrumentId, PositionStatusReport]

        """
        return self._position_reports.copy()

    cpdef void add_order_report(self, OrderStatusReport report) except *:
        """
        Add the order state report.

        Parameters
        ----------
        report : OrderStatusReport
            The report to add.

        """
        Condition.not_none(report, "report")

        self._order_reports[report.order_id] = report

    cpdef void add_exec_reports(self, OrderId order_id, list reports) except *:
        """
        Add the list of trades for the given order identifier.

        Parameters
        ----------
        order_id : OrderId
            The order identifier for the reports.
        reports : list[ExecutionReport]
            The list of execution reports to add.

        Raises
        -------
        TypeError
            If trades contains a type other than `ExecutionReport`.

        """
        Condition.not_none(order_id, "order_id")
        Condition.list_type(reports, ExecutionReport, "reports")

        self._exec_reports[order_id] = reports

    cpdef void add_position_report(self, PositionStatusReport report) except *:
        """
        Add the position state report.

        Parameters
        ----------
        report : PositionStatusReport
            The report to add.

        """
        Condition.not_none(report, "report")

        self._position_reports[report.instrument_id] = report
