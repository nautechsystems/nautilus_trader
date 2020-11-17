# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.c_enums.position_side cimport PositionSideParser
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport QuoteTick


cdef class Position:
    """
    Represents a position in a financial market.
    """

    def __init__(self, OrderFilled event not None):
        """
        Initialize a new instance of the `Position` class.

        Parameters
        ----------
        event : OrderFillEvent
            The order fill event which opened the position.

        """
        self._events = []  # type: [OrderFilled]
        self._buy_quantity = Decimal()
        self._sell_quantity = Decimal()

        # Identifiers
        self.id = event.position_id
        self.account_id = event.account_id
        self.from_order = event.cl_ord_id
        self.strategy_id = event.strategy_id

        # Properties
        self.symbol = event.symbol
        self.entry = event.order_side
        self.side = PositionSide.UNDEFINED
        self.relative_quantity = Decimal()
        self.quantity = Quantity()
        self.peak_quantity = Quantity()
        self.timestamp = event.execution_time
        self.opened_time = event.execution_time
        self.closed_time = None    # Can be None
        self.open_duration = None  # Can be None
        self.avg_open = event.fill_price.as_decimal()
        self.avg_close = Decimal()
        self.quote_currency = event.quote_currency
        self.settlement_currency = event.settlement_currency
        self.is_inverse = event.is_inverse
        self.realized_points = Decimal()
        self.realized_return = Decimal()
        self.realized_pnl = Money(0, self.settlement_currency)
        self.commissions = Money(0, self.settlement_currency)

        self.apply(event)

    def __eq__(self, Position other) -> bool:
        return self.id.value == other.id.value

    def __ne__(self, Position other) -> bool:
        return self.id.value != other.id.value

    def __hash__(self) -> int:
        return hash(self.id.value)

    def __repr__(self) -> str:
        return f"{type(self).__name__}(id={self.id.value}, {self.status_string_c()})"

    cdef list cl_ord_ids_c(self):
        cdef OrderFilled event
        return sorted(list({event.cl_ord_id for event in self._events}))

    cdef list order_ids_c(self):
        cdef OrderFilled event
        return sorted(list({event.order_id for event in self._events}))

    cdef list execution_ids_c(self):
        cdef OrderFilled event
        return [event.execution_id for event in self._events]

    cdef list events_c(self):
        return self._events.copy()

    cdef OrderFilled last_event_c(self):
        return self._events[-1]

    cdef ExecutionId last_execution_id_c(self):
        return self._events[-1].execution_id

    cdef int event_count_c(self) except *:
        return len(self._events)

    cdef str status_string_c(self):
        cdef str quantity = " " if self.relative_quantity == 0 else f" {self.quantity.to_string()} "
        return f"{PositionSideParser.to_string(self.side)}{quantity}{self.symbol}"

    cdef bint is_open_c(self) except *:
        return self.side != PositionSide.FLAT

    cdef bint is_closed_c(self) except *:
        return self.side == PositionSide.FLAT

    cdef bint is_long_c(self) except *:
        return self.side == PositionSide.LONG

    cdef bint is_short_c(self) except *:
        return self.side == PositionSide.SHORT

    @property
    def cl_ord_ids(self):
        """
        The client order identifiers associated with the position.

        Returns
        -------
        list[OrderId]

        Notes
        -----
        Guaranteed not to contain duplicate identifiers.

        """
        return self.cl_ord_ids_c()

    @property
    def order_ids(self):
        """
        The order identifiers associated with the position.

        Returns
        -------
        list[OrderId]

        Notes
        -----
        Guaranteed not to contain duplicate identifiers.

        """
        return self.order_ids_c()

    @property
    def execution_ids(self):
        """
        The execution identifiers associated with the position.

        Returns
        -------
        list[ExecutionId]

        Notes
        -----
        Assumption that all `ExecutionId`s were, unique, so the list
        may contain duplicates.

        """
        return self.execution_ids_c()

    @property
    def events(self):
        """
        The order fill events of the position.

        Returns
        -------
        list[Event]

        """
        return self.events_c()

    @property
    def last_event(self):
        """
        The last order fill event.

        Returns
        -------
        OrderFilled

        """
        return self.last_event_c()

    @property
    def last_execution_id(self):
        """
        The last execution identifier for the position.

        Returns
        -------
        ExecutionId

        """
        return self.last_execution_id_c()

    @property
    def event_count(self):
        """
        The count of order fill events applied to the position.

        Returns
        -------
        int

        """
        return self.event_count_c()

    @property
    def is_open(self):
        """
        If the position side is not `FLAT`.

        Returns
        -------
        bool
            True if FLAT, else False.

        """
        return self.is_open_c()

    @property
    def is_closed(self):
        """
        If the position side is `FLAT`.

        Returns
        -------
        bool
            True if not FLAT, else False.

        """
        return self.is_closed_c()

    @property
    def is_long(self):
        """
        If the position side is `LONG`.

        Returns
        -------
        bool
            True if LONG, else False.

        """
        return self.is_long_c()

    @property
    def is_short(self):
        """
        If the position side is short.

        Returns
        -------
        bool
            True if SHORT, else False.

        """
        return self.is_short_c()

    @staticmethod
    cdef inline PositionSide side_from_order_side_c(OrderSide side) except *:
        Condition.not_equal(side, OrderSide.UNDEFINED, "side", "UNDEFINED")

        return PositionSide.LONG if side == OrderSide.BUY else PositionSide.SHORT

    @staticmethod
    def side_from_order_side(OrderSide side):
        """
        Return the position side resulting from the given order side (from FLAT).

        Parameters
        ----------
        side : OrderSide
            The order side

        Returns
        -------
        PositionSide

        Raises
        ------
        ValueError
            If side is UNDEFINED.

        """
        return Position.side_from_order_side_c(side)

    cpdef void apply(self, OrderFilled event) except *:
        """
        Applies the given order fill event to the position.

        Parameters
        ----------
        event : OrderFillEvent
            The order fill event to apply.

        """
        Condition.not_none(event, "event")

        self._events.append(event)

        # Check currencies match
        assert event.commission.currency == self.commissions.currency
        assert event.commission.currency == self.settlement_currency
        self.commissions = Money(self.commissions + event.commission, event.commission.currency)

        # Calculate avg prices, points, return, PNL
        if event.order_side == OrderSide.BUY:
            self._handle_buy_order_fill(event)
        else:  # event.order_side == OrderSide.SELL:
            self._handle_sell_order_fill(event)

        # Set quantities
        self.quantity = Quantity(abs(self.relative_quantity))
        if self.quantity > self.peak_quantity:
            self.peak_quantity = self.quantity

        # Set state
        if self.relative_quantity > 0:
            self.side = PositionSide.LONG
        elif self.relative_quantity < 0:
            self.side = PositionSide.SHORT
        else:
            self.side = PositionSide.FLAT
            self.closed_time = event.execution_time
            self.open_duration = self.closed_time - self.opened_time

    cpdef Money calculate_pnl(
            self,
            avg_open: Decimal,
            avg_close: Decimal,
            quantity: Decimal,
    ):
        """
        Return the calculated PNL from the given parameters.

        Parameters
        ----------
        avg_open : Decimal or Price
            The average open price.
        avg_close : Decimal or Price
            The average close price.
        quantity : Decimal or Quantity
            The quantity for the calculation.

        Returns
        -------
        Money
            In the settlement currency.

        """
        Condition.type(avg_open, (Decimal, Price), "avg_open")
        Condition.type(avg_close, (Decimal, Price), "avg_close")
        Condition.type(quantity, (Decimal, Quantity), "quantity")

        if self.is_inverse:
            points = self._calculate_points_inverse(avg_open, avg_close)
        else:
            points = self._calculate_points(avg_open, avg_close)

        return Money(points * quantity, self.settlement_currency)

    cpdef Money unrealized_pnl(self, QuoteTick last):
        """
        Return the unrealized PNL from the given last quote tick.

        Parameters
        ----------
        last : QuoteTick
            The last tick for the calculation.

        Returns
        -------
        Money
            In the settlement currency.

        Raises
        ------
        ValueError
            If last.symbol != self.symbol

        """
        Condition.not_none(last, "last")
        Condition.equal(last.symbol, self.symbol, "last.symbol", "self.symbol")

        if self.side == PositionSide.FLAT:
            return Money(0, self.settlement_currency)

        return self.calculate_pnl(
            avg_open=self.avg_open,
            avg_close=self._get_close_price(last),
            quantity=self.quantity,
        )

    cpdef Money total_pnl(self, QuoteTick last):
        """
        Return the total PNL from the given last quote tick.

        Parameters
        ----------
        last : QuoteTick
            The last tick for the calculation.

        Returns
        -------
        Money
            In the settlement currency.

        Raises
        ------
        ValueError
            If last.symbol != self.symbol

        """
        Condition.not_none(last, "last")
        Condition.equal(last.symbol, self.symbol, "last.symbol", "self.symbol")

        return Money(self.realized_pnl + self.unrealized_pnl(last), self.settlement_currency)

    cdef inline void _handle_buy_order_fill(self, OrderFilled event) except *:
        realized_pnl: Decimal = event.commission.as_decimal()
        # LONG POSITION
        if self.relative_quantity > 0:
            self.avg_open = self._calculate_avg_open_price(event)
        # SHORT POSITION
        elif self.relative_quantity < 0:
            self.avg_close = self._calculate_avg_close_price(event)
            self.realized_points = self._calculate_points(self.avg_open, self.avg_close)
            self.realized_return = self._calculate_return(self.avg_open, self.avg_close)
            realized_pnl += self.calculate_pnl(self.avg_open, event.fill_price, event.fill_qty)

        self.realized_pnl = Money(self.realized_pnl + realized_pnl, self.settlement_currency)

        # Update quantities
        self._buy_quantity = self._buy_quantity + event.fill_qty
        self.relative_quantity = self.relative_quantity + event.fill_qty

    cdef inline void _handle_sell_order_fill(self, OrderFilled event) except *:
        realized_pnl: Decimal = event.commission.as_decimal()
        # SHORT POSITION
        if self.relative_quantity < 0:
            self.avg_open = self._calculate_avg_open_price(event)
        # LONG POSITION
        elif self.relative_quantity > 0:
            self.avg_close = self._calculate_avg_close_price(event)
            self.realized_points = self._calculate_points(self.avg_open, self.avg_close)
            self.realized_return = self._calculate_return(self.avg_open, self.avg_close)
            realized_pnl += self.calculate_pnl(self.avg_open, event.fill_price, event.fill_qty)

        self.realized_pnl = Money(self.realized_pnl + realized_pnl, self.settlement_currency)

        # Update quantities
        self._sell_quantity = self._sell_quantity + event.fill_qty
        self.relative_quantity = self.relative_quantity - event.fill_qty

    cdef inline object _calculate_cost(self, avg_price: Decimal, total_quantity: Decimal):
        return avg_price * total_quantity

    cdef inline object _calculate_avg_open_price(self, OrderFilled event):
        if not self.avg_open:
            return event.fill_price

        return self._calculate_avg_price(self.avg_open, self.quantity, event)

    cdef inline object _calculate_avg_close_price(self, OrderFilled event):
        if not self.avg_close:
            return event.fill_price

        close_quantity = self._sell_quantity if self.side == PositionSide.LONG else self._buy_quantity
        return self._calculate_avg_price(self.avg_close, close_quantity, event)

    cdef inline object _calculate_avg_price(
        self,
        avg_price: Decimal,
        quantity: Decimal,
        OrderFilled event,
    ):
        start_cost: Decimal = self._calculate_cost(avg_price, quantity)
        event_cost: Decimal = self._calculate_cost(event.fill_price, event.fill_qty)
        cumulative_quantity: Decimal = quantity + event.fill_qty
        return (start_cost + event_cost) / cumulative_quantity

    cdef inline object _calculate_points(self, avg_open: Decimal, avg_close: Decimal):
        if self.side == PositionSide.LONG:
            return avg_close - avg_open
        elif self.side == PositionSide.SHORT:
            return avg_open - avg_close
        else:
            return Decimal()  # FLAT

    cdef inline object _calculate_points_inverse(self, avg_open: Decimal, avg_close: Decimal):
        if self.side == PositionSide.LONG:
            return (1 / avg_open) - (1 / avg_close)
        elif self.side == PositionSide.SHORT:
            return (1 / avg_close) - (1 / avg_open)
        else:
            return Decimal()  # FLAT

    cdef inline object _calculate_return(self, avg_open: Decimal, avg_close: Decimal):
        return self._calculate_points(avg_open, avg_close) / avg_open

    cdef inline object _get_close_price(self, QuoteTick last):
        if self.side == PositionSide.LONG:
            return last.bid
        elif self.side == PositionSide.SHORT:
            return last.ask
        else:
            raise RuntimeError(f"invalid PositionSide, "
                               f"was {PositionSideParser.to_string(self.side)}")
