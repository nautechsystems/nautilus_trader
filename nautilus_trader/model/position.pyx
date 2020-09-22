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
from nautilus_trader.model.c_enums.market_position cimport MarketPosition
from nautilus_trader.model.c_enums.market_position cimport market_position_to_string
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.events cimport OrderFillEvent
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport ClientPositionId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport QuoteTick


cdef class Position:
    """
    Represents a position in a financial market.
    """

    def __init__(self, ClientPositionId cl_pos_id not None, OrderFillEvent event not None):
        """
        Initialize a new instance of the Position class.

        Parameters
        ----------
        cl_pos_id : ClientPositionId
            The client position identifier.
        event : OrderFillEvent
            The order fill event which opened the position.

        """
        self._order_ids = {event.cl_ord_id}         # type: {ClientOrderId}
        self._execution_ids = [event.execution_id]  # type: [ExecutionId]
        self._events = [event]                      # type: [OrderFillEvent]
        self._fill_prices = {}                      # type: {ClientOrderId, Price}
        self._buy_quantities = {}                   # type: {ClientOrderId, Quantity}
        self._sell_quantities = {}                  # type: {ClientOrderId, Quantity}

        self.client_id = cl_pos_id
        self.id = event.position_id
        self.account_id = event.account_id
        self.from_cl_ord_id = event.cl_ord_id
        self.symbol = event.symbol
        self.quote_currency = event.quote_currency
        self.entry_direction = event.order_side
        self.timestamp = event.execution_time
        self.opened_time = event.execution_time
        self.closed_time = None    # Can be none
        self.open_duration = None  # Can be none
        self.average_open_price = event.average_price.as_double()
        self.average_close_price = 0.0
        self.realized_points = 0.0
        self.realized_return = 0.0
        self.realized_pnl = Money(0, event.quote_currency)
        self.realized_pnl_last = Money(0, event.quote_currency)
        self.commission = Money(0, event.quote_currency)

        self.quantity = Quantity.zero()                  # Initialized in _update()
        self.peak_quantity = Quantity.zero()             # Initialized in _update()
        self._buy_quantity = Quantity.zero()             # Initialized in _update()
        self._sell_quantity = Quantity.zero()            # Initialized in _update()
        self._relative_quantity = 0.0                    # Initialized in _update()
        self._precision = event.filled_quantity.precision
        self.market_position = MarketPosition.UNDEFINED  # Initialized in _update()

        self._update(event)

    def __eq__(self, Position other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return self.equals(other)

    def __ne__(self, Position other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return not self.equals(other)

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

    cpdef bint equals(self, Position other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return self.client_id.equals(other.client_id)

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return f"Position(id={self.client_id.value}) {self.status_string()}"

    cpdef str market_position_as_string(self):
        """
        Return the market position as a string.

        Returns
        -------
        str

        """
        return market_position_to_string(self.market_position)

    cpdef str status_string(self):
        """
        Return the positions status as a string.

        Returns
        -------
        str

        """
        cdef str quantity = " " if self._relative_quantity == 0 else f" {self.quantity.to_string_formatted()} "
        return f"{market_position_to_string(self.market_position)}{quantity}{self.symbol}"

    cpdef list get_order_ids(self):
        """
        Return a list of all order identifiers.

        Returns
        -------
        List[OrderId]

        """
        return sorted(list(self._order_ids))

    cpdef list get_execution_ids(self):
        """
        Return a list of all execution identifiers.

        Returns
        -------
        List[ExecutionId]

        """
        return self._execution_ids.copy()

    cpdef list get_events(self):
        """
        Return a list of all order fill events.

        Returns
        -------
        List[Event]

        """
        return self._events.copy()

    cpdef OrderFillEvent last_event(self):
        """
        Return the count of events.

        Returns
        -------
        int

        """
        return self._events[-1]

    cpdef ExecutionId last_execution_id(self):
        """
        Return the last execution identifier for the position.

        Returns
        -------
        ExecutionId

        """
        return self._execution_ids[-1]

    cpdef int event_count(self):
        """
        Return the count of order fill events.

        Returns
        -------
        int

        """
        return len(self._events)

    cpdef bint is_open(self):
        """
        Return a value indicating whether the position is open.

        Returns
        -------
        bool

        """
        return self.market_position != MarketPosition.FLAT

    cpdef bint is_closed(self):
        """
        Return a value indicating whether the position is closed.

        Returns
        -------
        bool

        """
        return self.market_position == MarketPosition.FLAT

    cpdef bint is_long(self):
        """
        Return a value indicating whether the position is long.

        Returns
        -------
        bool

        """
        return self.market_position == MarketPosition.LONG

    cpdef bint is_short(self):
        """
        Return a value indicating whether the position is short.

        Returns
        -------
        bool

        """
        return self.market_position == MarketPosition.SHORT

    cpdef void apply(self, OrderFillEvent event) except *:
        """
        Applies the given order fill event to the position.

        Parameters
        ----------
        event : OrderFillEvent
            The order fill event to apply.

        """
        Condition.not_none(event, "event")

        # Update events
        self._events.append(event)

        # Update identifiers
        self._order_ids.add(event.cl_ord_id)
        self._execution_ids.append(event.execution_id)

        # Apply event
        self._update(event)

    cpdef double relative_quantity(self):
        """
        Return the relative quantity of the position.

        With positive values for long, negative values for short.

        Returns
        -------
        double

        """
        return self._relative_quantity

    cpdef double unrealized_points(self, QuoteTick last):
        """
        Return the calculated unrealized points for the position from the given current price.

        Parameters
        ----------
        last : QuoteTick
            The position symbols last tick.

        Returns
        -------
        double

        """
        Condition.not_none(last, "last")
        Condition.equal(self.symbol, last.symbol, "symbol", "last.symbol")

        if self.market_position == MarketPosition.LONG:
            return self._calculate_points(self.average_open_price, last.bid.as_double())
        elif self.market_position == MarketPosition.SHORT:
            return self._calculate_points(self.average_open_price, last.ask.as_double())
        else:
            return 0.0

    cpdef double total_points(self, QuoteTick last):
        """
        Return the calculated unrealized points for the position from the given current price.

        Parameters
        ----------
        last : QuoteTick
            The position symbols last tick.

        Returns
        -------
        double

        """
        Condition.not_none(last, "last")
        Condition.equal(self.symbol, last.symbol, "symbol", "last.symbol")

        return self.realized_points + self.unrealized_points(last)

    cpdef double unrealized_return(self, QuoteTick last):
        """
        Return the calculated unrealized return for the position from the given current price.

        Parameters
        ----------
        last : QuoteTick
            The position symbols last tick.

        Returns
        -------
        double

        """
        Condition.not_none(last, "last")
        Condition.equal(self.symbol, last.symbol, "symbol", "last.symbol")

        if self.market_position == MarketPosition.LONG:
            return self._calculate_return(self.average_open_price, last.bid.as_double())
        elif self.market_position == MarketPosition.SHORT:
            return self._calculate_return(self.average_open_price, last.ask.as_double())
        else:
            return 0.0

    cpdef double total_return(self, QuoteTick last):
        """
        Return the calculated unrealized return for the position from the given current price.

        Parameters
        ----------
        last : QuoteTick
            The position symbols last tick.

        Returns
        -------
        double

        """
        Condition.not_none(last, "last")
        Condition.equal(self.symbol, last.symbol, "symbol", "last.symbol")

        return self.realized_return + self.unrealized_return(last)

    cpdef Money unrealized_pnl(self, QuoteTick last):
        """
        Return the calculated unrealized return for the position from the given current price.

        Parameters
        ----------
        last : QuoteTick
            The position symbols last tick.

        Returns
        -------
        Money

        """
        Condition.not_none(last, "last")
        Condition.equal(self.symbol, last.symbol, "symbol", "last.symbol")

        if self.market_position == MarketPosition.LONG:
            return self._calculate_pnl(self.average_open_price, last.bid.as_double(), self.quantity)
        elif self.market_position == MarketPosition.SHORT:
            return self._calculate_pnl(self.average_open_price, last.ask.as_double(), self.quantity)
        else:
            return Money(0, self.quote_currency)

    cpdef Money total_pnl(self, QuoteTick last):
        """
        Return the calculated unrealized return for the position from the given current price.

        Parameters
        ----------
        last : QuoteTick
            The position symbols last tick.

        Returns
        -------
        Money

        """
        Condition.not_none(last, "last")
        Condition.equal(self.symbol, last.symbol, "symbol", "last.symbol")

        return self.realized_pnl.add(self.unrealized_pnl(last))

    cdef void _update(self, OrderFillEvent event) except *:
        self._fill_prices[event.cl_ord_id] = event.average_price
        self._precision = max(self._precision, event.filled_quantity.precision)

        if self.quote_currency != event.commission.currency:
            self.commission = self.commission.add(
                Money(event.commission.as_double() * event.average_price.as_double(),
                self.quote_currency)
            )
        else:
            self.commission = self.commission.add(event.commission)

        if event.order_side == OrderSide.BUY:
            self._handle_buy_order_fill(event)
        else:  # event.order_side == OrderSide.SELL:
            self._handle_sell_order_fill(event)

        # Set quantities
        self._relative_quantity = self._buy_quantity.as_double() - self._sell_quantity.as_double()
        self.quantity = Quantity(abs(self._relative_quantity), precision=self._precision)
        if self.quantity.gt(self.peak_quantity):
            self.peak_quantity = self.quantity

        # Set state
        if self._relative_quantity > 0.0:
            self.market_position = MarketPosition.LONG
        elif self._relative_quantity < 0.0:
            self.market_position = MarketPosition.SHORT
        else:
            self.market_position = MarketPosition.FLAT
            self.closed_time = event.execution_time
            self.open_duration = self.closed_time - self.opened_time

    cdef void _handle_buy_order_fill(self, OrderFillEvent event) except *:
        self._buy_quantities[event.cl_ord_id] = event.filled_quantity
        cdef double total_buy_qty = 0.0
        cdef Quantity quantity
        for quantity in self._buy_quantities.values():
            total_buy_qty += quantity.as_double()
        self._buy_quantity = Quantity(total_buy_qty, precision=self._precision)

        # LONG POSITION
        if self._relative_quantity > 0.0:
            self.average_open_price = self._calculate_average_price(self._buy_quantities, self._buy_quantity)
        # SHORT POSITION
        elif self._relative_quantity < 0.0:
            self.average_close_price = self._calculate_average_price(self._buy_quantities, self._buy_quantity)
            self.realized_points = self._calculate_points(self.average_open_price, self.average_close_price)
            self.realized_return = self._calculate_return(self.average_open_price, self.average_close_price)
            self.realized_pnl = self._calculate_pnl(self.average_open_price, self.average_close_price, self._buy_quantity)
            self.realized_pnl = self.realized_pnl.sub(self.commission)
        else:
            self.realized_pnl = self.realized_pnl.sub(self.commission)

    cdef void _handle_sell_order_fill(self, OrderFillEvent event) except *:
        self._sell_quantities[event.cl_ord_id] = event.filled_quantity
        cdef double total_sell_qty = 0.0
        cdef Quantity quantity
        for quantity in self._sell_quantities.values():
            total_sell_qty += quantity.as_double()
        self._sell_quantity = Quantity(total_sell_qty, precision=self._precision)

        # SHORT POSITION
        if self._relative_quantity < 0.0:
            self.average_open_price = self._calculate_average_price(self._sell_quantities, self._sell_quantity)
        # LONG POSITION
        elif self._relative_quantity > 0.0:
            self.average_close_price = self._calculate_average_price(self._sell_quantities, self._sell_quantity)
            self.realized_points = self._calculate_points(self.average_open_price, self.average_close_price)
            self.realized_return = self._calculate_return(self.average_open_price, self.average_close_price)
            self.realized_pnl = self._calculate_pnl(self.average_open_price, self.average_close_price, self._sell_quantity)
            self.realized_pnl = self.realized_pnl.sub(self.commission)
        else:
            self.realized_pnl = self.realized_pnl.sub(self.commission)

    cdef double _calculate_average_price(self, dict fills, Quantity total_quantity):
        cdef double cumulative_price = 0.0
        cdef ClientOrderId order_id
        cdef Quantity quantity
        for order_id, quantity in fills.items():
            cumulative_price += self._fill_prices[order_id].as_double() * quantity.as_double()
        return cumulative_price / total_quantity.as_double()

    cdef double _calculate_points(self, double opened_price, double closed_price):
        if self.market_position == MarketPosition.LONG:
            return closed_price - opened_price
        elif self.market_position == MarketPosition.SHORT:
            return opened_price - closed_price
        else:
            return 0.0  # FLAT

    cdef double _calculate_return(self, double opened_price, double closed_price):
        if self.market_position == MarketPosition.LONG:
            return (closed_price - opened_price) / opened_price
        elif self.market_position == MarketPosition.SHORT:
            return (opened_price - closed_price) / opened_price
        else:
            return 0.0  # FLAT

    cdef Money _calculate_pnl(self, double opened_price, double closed_price, Quantity filled_quantity):
        cdef double value = self._calculate_points(opened_price, closed_price) * filled_quantity.as_double()
        return Money(value, self.quote_currency)
