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

from libc.stdint cimport int64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySideParser
from nautilus_trader.model.c_enums.order_status cimport OrderStatus
from nautilus_trader.model.c_enums.order_status cimport OrderStatusParser
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.c_enums.position_side cimport PositionSideParser
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Quantity


cdef class OrderStatusReport:
    """
    Represents an orders state at a point in time.

    Parameters
    ----------
    client_order_id : ClientOrderId
        The reported client order ID.
    venue_order_id : VenueOrderId
        The reported order ID.
    order_status : OrderStatus
        The reported order status at the exchange.
    filled_qty : Quantity
        The reported filled quantity at the exchange.
    ts_init : int64
        The UNIX timestamp (nanoseconds) when the object was initialized.
    """
    def __init__(
        self,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        OrderStatus order_status,
        Quantity filled_qty not None,
        int64_t ts_init,
    ):
        self.client_order_id = client_order_id
        self.venue_order_id = venue_order_id
        self.order_status = order_status
        self.filled_qty = filled_qty
        self.ts_init = ts_init

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_order_id={self.client_order_id.value}, "
            f"venue_order_id={self.venue_order_id}, "
            f"order_status={OrderStatusParser.to_str(self.order_status)}, "
            f"filled_qty={self.filled_qty}, "
            f"ts_init={self.ts_init})"
        )


cdef class PositionStatusReport:
    """
    Represents a positions state at a point in time.

    Parameters
    ----------
    instrument_id : InstrumentId
        The reported instrument ID.
    position_side : PositionSide
        The reported position side at the exchange.
    qty : Quantity
        The reported position quantity at the exchange.
    ts_init : int64
        The UNIX timestamp (nanoseconds) when the object was initialized.
    """
    def __init__(
        self,
        InstrumentId instrument_id not None,
        PositionSide position_side,
        Quantity qty not None,
        int64_t ts_init,
    ):
        self.instrument_id = instrument_id
        self.side = position_side
        self.qty = qty
        self.ts_init = ts_init

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"side={PositionSideParser.to_str(self.side)}, "
            f"qty={self.qty}, "
            f"ts_init={self.ts_init})"
        )


cdef class ExecutionReport:
    """
    Represents a report of execution state by order ID.

    Parameters
    ----------
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId
        The venue order ID.
    venue_position_id : PositionId, optional
        The venue position ID associated with the order. If the trading
        venue has assigned a position ID / ticket then pass that here,
        otherwise pass ``None`` and the execution engine OMS will handle
        position ID resolution.
    execution_id : ExecutionId
        The execution ID for the trade.
    last_qty : Quantity
        The quantity of the last fill.
    last_px : Price
        The price of the last fill.
    commission : Money, optional
        The commission for the transaction (can be None).
    liquidity_side : LiquiditySide
        The liquidity side for the fill.
    ts_event : int64
        The UNIX timestamp (nanoseconds) when the execution event occurred.
    """

    def __init__(
        self,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        PositionId venue_position_id,  # Can be None
        ExecutionId execution_id not None,
        Quantity last_qty not None,
        Price last_px not None,
        Money commission,  # Can be None
        LiquiditySide liquidity_side,
        int64_t ts_event,
        int64_t ts_init,
    ):
        self.client_order_id = client_order_id
        self.venue_order_id = venue_order_id
        self.venue_position_id = venue_position_id
        self.id = execution_id
        self.last_qty = last_qty
        self.last_px = last_px
        self.commission = commission
        self.liquidity_side = liquidity_side
        self.ts_event = ts_event
        self.ts_init = ts_init

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_order_id={self.client_order_id.value}, "
            f"venue_order_id={self.venue_order_id}, "
            f"venue_position_id={self.venue_position_id}, "
            f"id={self.id.value}, "
            f"last_qty={self.last_qty}, "
            f"last_px={self.last_px}, "
            f"commission={self.commission.to_str()}, "
            f"liquidity_side={LiquiditySideParser.to_str(self.liquidity_side)}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )


cdef class ExecutionMassStatus:
    """
    Represents a mass status report of execution status.

    Parameters
    ----------
    client_id : ClientId
        The client ID for the report.
    account_id : AccountId
        The account ID for the report.
    ts_init : int64
        The UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        ClientId client_id not None,
        AccountId account_id not None,
        int64_t ts_init,
    ):
        self.client_id = client_id
        self.account_id = account_id
        self.ts_init = ts_init

        self._order_reports = {}     # type: dict[VenueOrderId, OrderStatusReport]
        self._exec_reports = {}      # type: dict[VenueOrderId, list[ExecutionReport]]
        self._position_reports = {}  # type: dict[InstrumentId, PositionStatusReport]

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "
            f"account_id={self.account_id}, "
            f"order_reports={self._order_reports}, "
            f"exec_reports={self._exec_reports}, "
            f"position_reports={self._position_reports}, "
            f"ts_init={self.ts_init})"
        )

    cpdef dict order_reports(self):
        """
        Return the order status reports.

        Returns
        -------
        dict[VenueOrderId, OrderStatusReport]

        """
        return self._order_reports.copy()

    cpdef dict exec_reports(self):
        """
        Return the execution reports.

        Returns
        -------
        dict[VenueOrderId, list[ExecutionReport]

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
        Add the order status report.

        Parameters
        ----------
        report : OrderStatusReport
            The report to add.

        """
        Condition.not_none(report, "report")

        self._order_reports[report.venue_order_id] = report

    cpdef void add_exec_reports(self, VenueOrderId venue_order_id, list reports) except *:
        """
        Add the list of trades for the given order ID.

        Parameters
        ----------
        venue_order_id : VenueOrderId
            The venue order ID for the reports.
        reports : list[ExecutionReport]
            The list of execution reports to add.

        Raises
        -------
        TypeError
            If `trades` contains a type other than `ExecutionReport`.

        """
        Condition.not_none(venue_order_id, "venue_order_id")
        Condition.list_type(reports, ExecutionReport, "reports")

        self._exec_reports[venue_order_id] = reports

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
