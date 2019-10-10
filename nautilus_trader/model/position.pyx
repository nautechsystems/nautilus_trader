# -------------------------------------------------------------------------------------------------
# <copyright file="position.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from decimal import Decimal
from typing import Set, List

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.market_position cimport MarketPosition, market_position_to_string
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.objects cimport Quantity, Tick
from nautilus_trader.model.events cimport OrderFillEvent
from nautilus_trader.model.identifiers cimport PositionId, ExecutionId


cdef class Position:
    """
    Represents a position in a financial market.
    """

    def __init__(self, PositionId position_id, OrderFillEvent event):
        """
        Initializes a new instance of the Position class.

        :param position_id: The positions identifier.
        :param event: The order fill event which opened the position.
        """
        self._order_ids = {event.order_id}          # type: Set[OrderId]
        self._execution_ids = {event.execution_id}  # type: Set[ExecutionId]
        self._events = [event]                      # type: List[OrderFillEvent]
        self._buy_quantities = {}                    # type: Dict[OrderId, int]
        self._sell_quantities = {}                   # type: Dict[OrderId, int]
        self._fill_prices = {}                       # type: Dict[OrderId, Decimal]
        self.last_event = event
        self.event_count = 1

        self.id = position_id
        self.id_broker = event.position_id_broker
        self.account_id = event.account_id
        self.from_order_id = event.order_id
        self.last_order_id = event.order_id
        self.last_execution_id = event.execution_id
        self.symbol = event.symbol
        self.base_currency = event.transaction_currency
        self.entry_direction = event.order_side
        self.timestamp = event.execution_time
        self.opened_time = event.execution_time
        self.closed_time = None  # Can be none
        self.open_duration = None  # Can be none
        self.average_open_price = event.average_price.value
        self.average_close_price = None  # Can be none
        self.realized_points = Decimal(0)
        self.realized_return = 0
        self.realized_pnl = Money.zero()
        self.realized_pnl_last = Money.zero()

        self._relative_quantity = 0                  # Initialized in _update()
        self.quantity = Quantity(0)                 # Initialized in _update()
        self.peak_quantity = Quantity(0)            # Initialized in _update()
        self.market_position = MarketPosition.FLAT  # Initialized in _update()

        self._update(event)

    cdef bint equals(self, Position other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.id.equals(other.id)

    def __eq__(self, Position other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.equals(other)

    def __ne__(self, Position other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return not self.equals(other)

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return f"Position(id={self.id.value}) {self.status_string()}"

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{str(self)} object at {id(self)}>"

    cpdef str status_string(self):
        """
        Return the positions status as a string.

        :return str.
        """
        cdef str quantity = ' ' if self._relative_quantity == 0 else f' {self.quantity.to_string_formatted()} '
        return f"{market_position_to_string(self.market_position)}{quantity}{self.symbol}"

    cpdef list get_order_ids(self):
        """
        Return a list of all order_ids.
        
        :return List[OrderId]. 
        """
        return sorted(self._order_ids)

    cpdef list get_execution_ids(self):
        """
        Return a list of all execution identifiers.
        
        :return List[ExecutionId]. 
        """
        return sorted(self._execution_ids)

    cpdef list get_events(self):
        """
        Return a list of all order fill events.
        
        :return List[Event].
        """
        return self._events.copy()

    cpdef void apply(self, OrderFillEvent event) except *:
        """
        Applies the given order fill event to the position.

        :param event: The order fill event to apply.
        """
        # Update events
        self._events.append(event)
        self.last_event = event
        self.event_count += 1

        # Update identifiers
        self._order_ids.add(event.order_id)
        self._execution_ids.add(event.execution_id)
        self.last_order_id = event.order_id
        self.last_execution_id = event.execution_id

        # Apply event
        self._update(event)

    cpdef object unrealized_points(self, Tick last):
        """
        Return the calculated unrealized points for the position from the given current price.
         
        :param last: The position symbols last tick.
        :return Decimal.
        """
        Condition.equal(self.symbol, last.symbol)

        if self.market_position == MarketPosition.LONG:
            return self._calculate_points(self.average_open_price, last.bid.value)
        elif self.market_position == MarketPosition.SHORT:
            return self._calculate_points(self.average_open_price, last.ask.value)
        else:
            return Decimal(0)

    cpdef float unrealized_return(self, Tick last) except *:
        """
        Return the calculated unrealized return for the position from the given current price.
         
        :param last: The position symbols last tick.
        :return float.
        """
        Condition.equal(self.symbol, last.symbol)

        if self.market_position == MarketPosition.LONG:
            return self._calculate_return(self.average_open_price, last.bid.value)
        elif self.market_position == MarketPosition.SHORT:
            return self._calculate_return(self.average_open_price, last.ask.value)
        else:
            return 0

    cpdef Money unrealized_pnl(self, Tick last):
        """
        Return the calculated unrealized return for the position from the given current price.
         
        :param last: The position symbols last tick.
        :return Money.
        """
        Condition.equal(self.symbol, last.symbol)

        if self.market_position == MarketPosition.LONG:
            return self._calculate_pnl(self.average_open_price, last.bid.value, self.quantity.value)
        elif self.market_position == MarketPosition.SHORT:
            return self._calculate_pnl(self.average_open_price, last.ask.value, self.quantity.value)
        else:
            return Money(0)

    cpdef object total_points(self, Tick last):
        """
        Return the calculated unrealized points for the position from the given current price.
         
        :param last: The position symbols last tick.
        :return Decimal.
        """
        Condition.equal(self.symbol, last.symbol)

        return self.realized_points + self.unrealized_points(last)

    cpdef float total_return(self, Tick last) except *:
        """
        Return the calculated unrealized return for the position from the given current price.
         
        :param last: The position symbols last tick.
        :return float.
        """
        Condition.equal(self.symbol, last.symbol)

        return self.realized_return + self.unrealized_return(last)

    cpdef Money total_pnl(self, Tick last):
        """
        Return the calculated unrealized return for the position from the given current price.
         
        :param last: The position symbols last tick.
        :return Money.
        """
        Condition.equal(self.symbol, last.symbol)

        return self.realized_pnl + self.unrealized_pnl(last)

    cdef void _update(self, OrderFillEvent event) except *:
        self._fill_prices[event.order_id] = event.average_price.value

        if event.order_side == OrderSide.BUY:
            self._handle_buy_order_fill(event)
        elif event.order_side == OrderSide.SELL:
            self._handle_sell_order_fill(event)
        else:
            raise RuntimeError(f"Cannot update position (event order side invalid {event.order_side})")

        # Set quantities
        self._relative_quantity = self._buy_quantity - self._sell_quantity
        self.quantity = Quantity(abs(self._relative_quantity))
        if self.quantity > self.peak_quantity:
            self.peak_quantity = self.quantity

        # Set state
        if self._relative_quantity > 0:
            self.market_position = MarketPosition.LONG
            self.is_open = True
            self.is_long = True
            self.is_closed = False
            self.is_short = False
        elif self._relative_quantity < 0:
            self.market_position = MarketPosition.SHORT
            self.is_open = True
            self.is_short = True
            self.is_closed = False
            self.is_long = False
        else:
            self.market_position = MarketPosition.FLAT
            self.closed_time = event.execution_time
            self.open_duration = self.closed_time - self.opened_time
            self.is_closed = True
            self.is_open = False
            self.is_long = False
            self.is_short = False

    cdef void _handle_buy_order_fill(self, OrderFillEvent event):
        self._buy_quantities[event.order_id] = event.filled_quantity.value
        self._buy_quantity = sum(self._buy_quantities.itervalues())

        # LONG POSITION
        if self._relative_quantity > 0:
            self.average_open_price = self._calculate_average_price(self._buy_quantities, self._buy_quantity)
        # SHORT POSITION
        elif self._relative_quantity < 0:
            self.average_close_price = self._calculate_average_price(self._buy_quantities, self._buy_quantity)
            self.realized_points = self._calculate_points(self.average_open_price, self.average_close_price)
            self.realized_return = self._calculate_return(self.average_open_price, self.average_close_price)
            self.realized_pnl = self._calculate_pnl(self.average_open_price, self.average_close_price, self._buy_quantity)

    cdef void _handle_sell_order_fill(self, OrderFillEvent event):
        self._sell_quantities[event.order_id] = event.filled_quantity.value
        self._sell_quantity = sum(self._sell_quantities.itervalues())

        # SHORT POSITION
        if self._relative_quantity < 0:
            self.average_open_price = self._calculate_average_price(self._sell_quantities, self._sell_quantity)
        # LONG POSITION
        elif self._relative_quantity > 0:
            self.average_close_price = self._calculate_average_price(self._sell_quantities, self._sell_quantity)
            self.realized_points = self._calculate_points(self.average_open_price, self.average_close_price)
            self.realized_return = self._calculate_return(self.average_open_price, self.average_close_price)
            self.realized_pnl = self._calculate_pnl(self.average_open_price, self.average_close_price, self._sell_quantity)

    cdef object _calculate_average_price(self, dict fills, long total_quantity):
        cdef object cumulative_price = Decimal(0)
        for order_id, quantity in fills.items():
            cumulative_price += self._fill_prices[order_id] * quantity
        return cumulative_price / total_quantity

    cdef object _calculate_points(self, opened_price, closed_price):
        if self.market_position == MarketPosition.LONG:
            return closed_price - opened_price
        elif self.market_position == MarketPosition.SHORT:
            return opened_price - closed_price
        elif self.market_position == MarketPosition.FLAT:
            return Decimal(0)

    cdef float _calculate_return(self, opened_price, closed_price):
        if self.market_position == MarketPosition.LONG:
            return (float(closed_price) - float(opened_price)) / float(opened_price)
        elif self.market_position == MarketPosition.SHORT:
            return (float(opened_price) - float(closed_price)) / float(opened_price)
        else:
            return 0

    cdef Money _calculate_pnl(self, opened_price, closed_price, long filled_quantity):
        return Money(self._calculate_points(opened_price, closed_price) * filled_quantity)
