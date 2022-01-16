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

from decimal import Decimal
from typing import Optional

from libc.stdint cimport int64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.contingency_type cimport ContingencyTypeParser
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySideParser
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_status cimport OrderStatus
from nautilus_trader.model.c_enums.order_status cimport OrderStatusParser
from nautilus_trader.model.c_enums.order_type cimport OrderTypeParser
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.c_enums.position_side cimport PositionSideParser
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForceParser
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Quantity


cdef class OrderStatusReport:
    """
    Represents an order status at a point in time.

    Parameters
    ----------
    instrument_id : InstrumentId
        The reported instrument ID for the order.
    client_order_id : ClientOrderId, optional
        The reported client order ID.
    order_list_id : OrderListId, optional
        The reported order list ID associated with the order.
    venue_order_id : VenueOrderId
        The reported order ID (assigned by the venue).
    order_side : OrderSide {``BUY``, ``SELL``}
        The reported order side.
    order_type : OrderType
        The reported order type.
    contingency : ContingencyType
        The reported order contingency type.
    time_in_force : TimeInForce
        The reported order time in force.
    order_status : OrderStatus
        The reported order status at the exchange.
    price : Price, optional
        The reported order price (STOP or LIMIT).
    trigger : Price, optional
        The reported trigger price (STOP).
    quantity : Quantity
        The reported order original quantity.
    filled_qty : Quantity
        The reported filled quantity at the exchange.
    display_qty : Quantity, optional
        The reported order quantity to display on the public book (iceberg).
    avg_px : Decimal, optional
        The reported order average fill price.
    is_reduce_only : bool
        If the reported order carries the 'reduce-only' execution instruction.
    is_post_only : bool
        If the reported order will only provide liquidity (make a market).
    reject_reason : str, optional
        The reported reason for order rejection.
    ts_accepted : int64
        The UNIX timestamp (nanoseconds) when the reported order was accepted.
    ts_last : int64
        The UNIX timestamp (nanoseconds) of the last order status change.
    ts_init : int64
        The UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id,  # Can be None (external order)
        OrderListId order_list_id,  # Can be None
        VenueOrderId venue_order_id not None,
        OrderSide order_side,
        OrderType order_type,
        ContingencyType contingency,
        TimeInForce time_in_force,
        OrderStatus order_status,
        Price price,  # Can be None
        Price trigger,  # Can be None
        Quantity quantity not None,
        Quantity filled_qty not None,
        Quantity display_qty,  # Can be None
        avg_px: Optional[Decimal],
        bint is_reduce_only,
        bint is_post_only,
        str reject_reason,  # Can be None
        int64_t ts_accepted,
        int64_t ts_last,
        int64_t ts_init,
    ):
        self.instrument_id = instrument_id
        self.client_order_id = client_order_id
        self.order_list_id = order_list_id
        self.venue_order_id = venue_order_id
        self.order_side = order_side
        self.order_type = order_type
        self.contingency = contingency
        self.time_in_force = time_in_force
        self.order_status = order_status
        self.price = price
        self.trigger = trigger
        self.quantity = quantity
        self.filled_qty = filled_qty
        self.leaves_qty = Quantity(self.quantity - self.filled_qty, self.quantity.precision)
        self.display_qty = display_qty
        self.avg_px = avg_px
        self.is_reduce_only = is_reduce_only
        self.is_post_only = is_post_only
        self.reject_reason = reject_reason
        self.ts_accepted = ts_accepted
        self.ts_last = ts_last
        self.ts_init = ts_init

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_order_id={self.client_order_id.value}, "
            f"order_list_id={self.order_list_id}, "
            f"venue_order_id={self.venue_order_id.value}, "
            f"order_side={OrderSideParser.to_str(self.order_side)}, "
            f"order_type={OrderTypeParser.to_str(self.order_type)}, "
            f"contingency={ContingencyTypeParser.to_str(self.contingency)}, "
            f"time_in_force={TimeInForceParser.to_str(self.time_in_force)}, "
            f"order_status={OrderStatusParser.to_str(self.order_status)}, "
            f"price={self.price}, "
            f"trigger={self.trigger}, "
            f"quantity={self.quantity}, "
            f"filled_qty={self.filled_qty}, "
            f"leaves_qty={self.leaves_qty}, "
            f"display_qty={self.display_qty}, "
            f"avg_px={self.avg_px}, "
            f"is_reduce_only={self.is_reduce_only}, "
            f"is_post_only={self.is_post_only}, "
            f"reject_reason={self.reject_reason}, "
            f"ts_accepted={self.ts_accepted}, "
            f"ts_last={self.ts_last}, "
            f"ts_init={self.ts_init})"
        )


cdef class TradeReport:
    """
    Represents a report of a single trade.

    Parameters
    ----------
    instrument_id : InstrumentId
        The reported instrument ID for the trade.
    client_order_id : ClientOrderId, optional
        The reported client order ID for the trade.
    venue_order_id : VenueOrderId
        The reported venue order ID (assigned by the venue) for the trade.
    venue_position_id : PositionId, optional
        The reported venue position ID for the trade. If the trading venue has
        assigned a position ID / ticket for the trade then pass that here,
        otherwise pass ``None`` and the execution engine OMS will handle
        position ID resolution.
    trade_id : TradeId
        The reported trade match ID.
    order_side : OrderSide {``BUY``, ``SELL``}
        The reported order side for the trade.
    last_qty : Quantity
        The reported quantity of the trade.
    last_px : Price
        The reported price of the trade.
    commission : Money, optional
        The reported commission for the trade (can be ``None``).
    liquidity_side : LiquiditySide {``NONE``, ``MAKER``, ``TAKER``}
        The reported liquidity side for the trade.
    ts_event : int64
        The UNIX timestamp (nanoseconds) when the trade occurred.
    ts_init : int64
        The UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id,  # Can be None (external order)
        VenueOrderId venue_order_id not None,
        PositionId venue_position_id,  # Can be None
        TradeId trade_id not None,
        OrderSide order_side,
        Quantity last_qty not None,
        Price last_px not None,
        Money commission,  # Can be None
        LiquiditySide liquidity_side,
        int64_t ts_event,
        int64_t ts_init,
    ):
        self.instrument_id = instrument_id
        self.client_order_id = client_order_id
        self.venue_order_id = venue_order_id
        self.venue_position_id = venue_position_id
        self.trade_id = trade_id
        self.order_side = order_side
        self.last_qty = last_qty
        self.last_px = last_px
        self.commission = commission
        self.liquidity_side = liquidity_side
        self.ts_event = ts_event
        self.ts_init = ts_init

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.value}, "
            f"client_order_id={self.client_order_id.value}, "
            f"venue_order_id={self.venue_order_id.value}, "
            f"venue_position_id={self.venue_position_id}, "
            f"trade_id={self.trade_id.value}, "
            f"order_side={OrderSideParser.to_str(self.order_side)}, "
            f"last_qty={self.last_qty}, "
            f"last_px={self.last_px}, "
            f"commission={self.commission.to_str()}, "
            f"liquidity_side={LiquiditySideParser.to_str(self.liquidity_side)}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )


cdef class PositionStatusReport:
    """
    Represents a position status at a point in time.

    Parameters
    ----------
    instrument_id : InstrumentId
        The reported instrument ID for the position.
    venue_position_id : PositionId, optional
        The reported venue position ID (assigned by the venue). If the trading
        venue has assigned a position ID / ticket for the trade then pass that
        here, otherwise pass ``None`` and the execution engine OMS will handle
        position ID resolution.
    position_side : PositionSide {``FLAT``, ``LONG``, ``SHORT``}
        The reported position side at the exchange.
    quantity : Quantity
        The reported position quantity at the exchange.
    ts_last : int64
        The UNIX timestamp (nanoseconds) of the last position change.
    ts_init : int64
        The UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        PositionId venue_position_id,  # Can be None
        PositionSide position_side,
        Quantity quantity not None,
        int64_t ts_last,
        int64_t ts_init,
    ):
        self.instrument_id = instrument_id
        self.venue_position_id = venue_position_id
        self.position_side = position_side
        self.quantity = quantity
        self.ts_last = ts_last
        self.ts_init = ts_init

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.value}, "
            f"venue_position_id={self.venue_position_id}, "
            f"position_side={PositionSideParser.to_str(self.position_side)}, "
            f"quantity={self.quantity}, "
            f"ts_last={self.ts_last}, "
            f"ts_init={self.ts_init})"
        )


cdef class ExecutionMassStatus:
    """
    Represents an execution mass status report including status of all open
    orders, trades for those orders and open positions.

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
        self._trade_reports = {}     # type: dict[VenueOrderId, list[TradeReport]]
        self._position_reports = {}  # type: dict[InstrumentId, PositionStatusReport]

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "
            f"account_id={self.account_id}, "
            f"order_reports={self._order_reports}, "
            f"trade_reports={self._trade_reports}, "
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

    cpdef dict trade_reports(self):
        """
        Return the trade reports.

        Returns
        -------
        dict[VenueOrderId, list[TradeReport]

        """
        return self._trade_reports.copy()

    cpdef dict position_reports(self):
        """
        Return the position status reports.

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

    cpdef void add_trade_reports(self, VenueOrderId venue_order_id, list reports) except *:
        """
        Add the list of trade reports for the given order ID.

        Parameters
        ----------
        venue_order_id : VenueOrderId
            The venue order ID (assigned by the venue) for the reports.
        reports : list[TradeReport]
            The list of reports to add.

        Raises
        -------
        TypeError
            If `trades` contains a type other than `TradeReport`.
        ValueError
            If `reports` is empty.
        ValueError
            If `reports` contains a type other than `TradeReport`.

        """
        Condition.not_none(venue_order_id, "venue_order_id")
        Condition.not_empty(reports, "reports")
        Condition.list_type(reports, TradeReport, "reports")

        self._trade_reports[venue_order_id] = reports

    cpdef void add_position_report(self, PositionStatusReport report) except *:
        """
        Add the position status report.

        Parameters
        ----------
        report : PositionStatusReport
            The report to add.

        """
        Condition.not_none(report, "report")

        self._position_reports[report.instrument_id] = report
