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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.c_enums.position_side cimport position_side_to_string
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport QuoteTick


cdef class Position:
    """
    Represents a position in a financial market.
    """

    def __init__(self, OrderFilled event not None):
        """
        Initialize a new instance of the Position class.

        Parameters
        ----------
        event : OrderFillEvent
            The order fill event which opened the position.

        """
        self._events = []                    # type: [OrderFilled]
        self._buy_quantity = Quantity()      # Initialized in _update()
        self._sell_quantity = Quantity()     # Initialized in _update()
        self._relative_quantity = Decimal()  # Initialized in _update()

        # Identifiers
        self.id = event.position_id
        self.account_id = event.account_id
        self.from_order = event.cl_ord_id
        self.strategy_id = event.strategy_id

        # Properties
        self.symbol = event.symbol
        self.entry = event.order_side
        self.side = PositionSide.UNDEFINED  # Initialized in _update()
        self.quantity = Quantity()          # Initialized in _update()
        self.peak_quantity = Quantity()     # Initialized in _update()
        self.base_currency = event.base_currency
        self.quote_currency = event.quote_currency
        self.timestamp = event.execution_time
        self.opened_time = event.execution_time
        self.closed_time = None    # Can be none
        self.open_duration = None  # Can be none
        self.avg_open_price = Decimal(event.avg_price)
        self.avg_close_price = Decimal()
        self.realized_points = Decimal()
        self.realized_return = Decimal()
        self.realized_pnl = Money(0, event.base_currency)
        self.commission = Money(0, event.base_currency)
        self.last_tick = None  # Can be none

        self.apply(event)

    def __eq__(self, Position other) -> bool:
        return self.id == other.id

    def __ne__(self, Position other) -> bool:
        return self.id != other.id

    def __hash__(self) -> int:
        """
        Return the hash code of this object.

        Returns
        -------
        int

        """
        return hash(self.id.value)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return self.to_string()

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        Returns
        -------
        str

        """
        return f"<{str(self)} object at {id(self)}>"

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

        # Update total commission
        self.commission = Money(self.commission + event.commission, self.commission.currency)

        # Calculate avg prices, points, return, PNL
        if event.order_side == OrderSide.BUY:
            self._handle_buy_order_fill(event)
        else:  # event.order_side == OrderSide.SELL:
            self._handle_sell_order_fill(event)

        # Set quantities
        self.quantity = Quantity(abs(self._relative_quantity))
        if self.quantity > self.peak_quantity:
            self.peak_quantity = self.quantity

        # Set state
        if self._relative_quantity > 0:
            self.side = PositionSide.LONG
        elif self._relative_quantity < 0:
            self.side = PositionSide.SHORT
        else:
            self.side = PositionSide.FLAT
            self.closed_time = event.execution_time
            self.open_duration = self.closed_time - self.opened_time

    @staticmethod
    cdef inline PositionSide side_from_order_side_c(OrderSide side) except *:
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

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return f"Position(id={self.id.value}, {self.status_string()})"

    cpdef str side_as_string(self):
        """
        Return the position side as a string.

        Returns
        -------
        str

        """
        return position_side_to_string(self.side)

    cpdef str status_string(self):
        """
        Return the positions status as a string.

        Returns
        -------
        str

        """
        cdef str quantity = " " if self._relative_quantity == 0 else f" {self.quantity.to_string()} "
        return f"{position_side_to_string(self.side)}{quantity}{self.symbol}"

    cpdef set cl_ord_ids(self):
        """
        Return a list of all client order identifiers.

        Returns
        -------
        Set[OrderId]

        """
        cdef OrderFilled event
        return {event.cl_ord_id for event in self._events}

    cpdef set order_ids(self):
        """
        Return a list of all client order identifiers.

        Returns
        -------
        Set[OrderId]

        """
        cdef OrderFilled event
        return {event.order_id for event in self._events}

    cpdef set execution_ids(self):
        """
        Return a list of all execution identifiers.

        Returns
        -------
        Set[ExecutionId]

        """
        cdef OrderFilled event
        return {event.execution_id for event in self._events}

    cpdef list events(self):
        """
        Return a list of all order fill events.

        Returns
        -------
        List[Event]

        """
        return self._events.copy()

    cpdef OrderFilled last_event(self):
        """
        Return the last fill event.

        Returns
        -------
        OrderFilled

        """
        return self._events[-1]

    cpdef ExecutionId last_execution_id(self):
        """
        Return the last execution identifier for the position.

        Returns
        -------
        ExecutionId

        """
        return self._events[-1].execution_id

    cpdef int event_count(self) except *:
        """
        Return the count of order fill events.

        Returns
        -------
        int

        """
        return len(self._events)

    cpdef bint is_open(self) except *:
        """
        Return a value indicating whether the position is open.

        Returns
        -------
        bool

        """
        return self.side != PositionSide.FLAT

    cpdef bint is_closed(self) except *:
        """
        Return a value indicating whether the position is closed.

        Returns
        -------
        bool

        """
        return self.side == PositionSide.FLAT

    cpdef bint is_long(self) except *:
        """
        Return a value indicating whether the position is long.

        Returns
        -------
        bool

        """
        return self.side == PositionSide.LONG

    cpdef bint is_short(self) except *:
        """
        Return a value indicating whether the position is short.

        Returns
        -------
        bool

        """
        return self.side == PositionSide.SHORT

    cpdef Decimal relative_quantity(self):
        """
        Return the relative quantity of the position.

        With positive values for long, negative values for short.

        Returns
        -------
        double

        """
        return self._relative_quantity

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

        """
        Condition.not_none(last, "last")

        if self.side == PositionSide.LONG:
            return self._calculate_pnl(self.avg_open_price, last.bid, self.quantity)
        elif self.side == PositionSide.SHORT:
            return self._calculate_pnl(self.avg_open_price, last.ask, self.quantity)
        else:
            return Money(0, self.base_currency)

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

        """
        Condition.not_none(last, "last")

        return Money(self.realized_pnl + self.unrealized_pnl(last), self.base_currency)

    cdef inline void _handle_buy_order_fill(self, OrderFilled event) except *:
        cdef Money realized_pnl = event.commission
        # LONG POSITION
        if self._relative_quantity > 0:
            self.avg_open_price = self._calculate_avg_open_price(event)
        # SHORT POSITION
        elif self._relative_quantity < 0:
            self.avg_close_price = self._calculate_avg_close_price(event)
            self.realized_points = self._calculate_points(self.avg_open_price, self.avg_close_price)
            self.realized_return = self._calculate_return(self.avg_open_price, self.avg_close_price)
            realized_pnl = self._calculate_pnl(self.avg_open_price, event.avg_price, event.filled_qty)

        self.realized_pnl = Money(self.realized_pnl + realized_pnl, self.base_currency)

        # Update quantities
        self._buy_quantity = Quantity(self._buy_quantity + event.filled_qty)
        self._relative_quantity = Decimal(self._relative_quantity + event.filled_qty)

    cdef inline void _handle_sell_order_fill(self, OrderFilled event) except *:
        cdef Money realized_pnl = event.commission
        # SHORT POSITION
        if self._relative_quantity < 0:
            self.avg_open_price = Decimal(self._calculate_avg_open_price(event))
        # LONG POSITION
        elif self._relative_quantity > 0:
            self.avg_close_price = self._calculate_avg_close_price(event)
            self.realized_points = self._calculate_points(self.avg_open_price, self.avg_close_price)
            self.realized_return = self._calculate_return(self.avg_open_price, self.avg_close_price)
            realized_pnl = self._calculate_pnl(self.avg_open_price, event.avg_price, event.filled_qty)

        self.realized_pnl = Money(self.realized_pnl + realized_pnl, self.base_currency)

        # Update quantities
        self._sell_quantity = Quantity(self._sell_quantity + event.filled_qty)
        self._relative_quantity = Decimal(self._relative_quantity - event.filled_qty)

    cdef inline Decimal _calculate_cost(self, Decimal avg_price, Quantity total_quantity):
        return avg_price * total_quantity

    cdef inline Decimal _calculate_avg_open_price(self, OrderFilled event):
        if not self.avg_open_price:
            return event.avg_price

        return self._calculate_avg_price(self.avg_open_price, self.quantity, event)

    cdef inline Decimal _calculate_avg_close_price(self, OrderFilled event):
        if not self.avg_close_price:
            return event.avg_price

        cdef Quantity close_quantity = Quantity(self._sell_quantity) if self.side == PositionSide.LONG else self._buy_quantity
        return self._calculate_avg_price(self.avg_close_price, close_quantity, event)

    cdef inline Decimal _calculate_avg_price(
        self,
        Decimal open_price,
        Quantity open_quantity,
        OrderFilled event,
    ):
        cdef Decimal start_cost = self._calculate_cost(open_price, open_quantity)
        cdef Decimal event_cost = self._calculate_cost(event.avg_price, event.filled_qty)
        cdef Decimal cumulative_quantity = open_quantity + event.filled_qty
        return (start_cost + event_cost) / cumulative_quantity

    cdef inline Decimal _calculate_points(self, Decimal open_price, Decimal close_price):
        if self.side == PositionSide.LONG:
            return close_price - open_price
        elif self.side == PositionSide.SHORT:
            return open_price - close_price
        else:
            return Decimal()  # FLAT

    cdef inline Decimal _calculate_return(self, Decimal open_price, Decimal close_price):
        if self.side == PositionSide.LONG:
            return (close_price - open_price) / open_price
        elif self.side == PositionSide.SHORT:
            return (open_price - close_price) / open_price
        else:
            return Decimal()  # FLAT

    cdef inline Money _calculate_pnl(
        self,
        Decimal open_price,
        Decimal close_price,
        Quantity filled_qty,
    ):
        return Money(
            self._calculate_return(open_price, close_price) * filled_qty,
            self.base_currency,
        )
