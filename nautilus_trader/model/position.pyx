# -------------------------------------------------------------------------------------------------
# <copyright file="position.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from decimal import Decimal
from typing import Set, List

from nautilus_trader.model.c_enums.market_position cimport MarketPosition, market_position_to_string
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.events cimport OrderFillEvent
from nautilus_trader.model.identifiers cimport PositionId, ExecutionId, ExecutionTicket


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
        self._order_ids = {event.order_id}                  # type: Set[OrderId]
        self._execution_ids = {event.execution_id}          # type: Set[ExecutionId]
        self._execution_tickets = {event.execution_ticket}  # type: Set[ExecutionTicket]
        self._events = [event]                              # type: List[OrderFillEvent]
        self.last_event = event
        self.event_count = 1

        self.symbol = event.symbol
        self.id = position_id
        self.account_id = event.account_id
        self.from_order_id = event.order_id
        self.last_order_id = event.order_id
        self.last_execution_id = event.execution_id
        self.last_execution_ticket = event.execution_ticket
        self.timestamp = event.execution_time
        self.entry_direction = event.order_side
        self.entry_time = event.execution_time
        self.exit_time = None  # Can be none
        self.average_entry_price = event.average_price
        self.average_exit_price = None  # Can be none
        self.points_realized = Decimal(0)
        self.return_realized = 0.0

        self.relative_quantity = 0                   # Initialized in _fill_logic
        self.quantity = Quantity(0)                  # Initialized in _fill_logic
        self.peak_quantity = Quantity(0)             # Initialized in _fill_logic
        self.market_position = MarketPosition.FLAT   # Initialized in _fill_logic

        self._fill_logic(event)

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

    cdef str status_string(self):
        """
        Return the positions status as a string.

        :return str.
        """
        cdef str quantity = '' if self.relative_quantity == 0 else ' {:,}'.format(self.quantity.value)
        return f"{self.symbol} {market_position_to_string(self.market_position)}{quantity}"

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

    cpdef list get_execution_tickets(self):
        """
        Return a list of all execution tickets.
        
        :return List[ExecutionTicket]. 
        """
        return sorted(self._execution_tickets)

    cpdef list get_events(self):
        """
        Return a list of all order fill events.
        
        :return List[Event].
        """
        return self._events.copy()

    cpdef void apply(self, OrderFillEvent event):
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
        self._execution_tickets.add(event.execution_ticket)
        self.last_order_id = event.order_id
        self.last_execution_id = event.execution_id
        self.last_execution_ticket = event.execution_ticket

        # Apply event
        self._increment_returns(event)
        self._fill_logic(event)

    cpdef object points_unrealized(self, Price current_price):
        """
        Return the calculated unrealized points from the given current price.
         
        :param current_price: The current price of the position instrument.
        :return Decimal.
        """
        if self.is_closed:
            return Decimal(0)
        return self._calculate_points(self.average_entry_price, current_price)

    cpdef float return_unrealized(self, Price current_price):
        """
        Return the calculated unrealized return from the given current price.
         
        :param current_price: The current price of the position instrument.
        :return float.
        """
        if self.is_closed:
            return 0.0
        return self._calculate_return(self.average_entry_price, current_price)

    @staticmethod
    cdef int _calculate_relative_quantity(OrderFillEvent event):
        if event.order_side == OrderSide.BUY:
            return event.filled_quantity.value
        elif event.order_side == OrderSide.SELL:
            return - event.filled_quantity.value
        return 0

    cdef void _fill_logic(self, OrderFillEvent event):
        # Set quantities
        self.relative_quantity += Position._calculate_relative_quantity(event)
        self.quantity = Quantity(abs(self.relative_quantity))
        if self.quantity > self.peak_quantity:
            self.peak_quantity = self.quantity

        # Set state
        if self.relative_quantity > 0:
            self.market_position = MarketPosition.LONG
            self.is_open = True
            self.is_long = True
            self.is_closed = False
            self.is_flat = False
            self.is_short = False
        elif self.relative_quantity < 0:
            self.market_position = MarketPosition.SHORT
            self.is_open = True
            self.is_short = True
            self.is_closed = False
            self.is_flat = False
            self.is_long = False
        else:
            self.market_position = MarketPosition.FLAT
            self.exit_time = event.timestamp
            self.average_exit_price = event.average_price
            self.is_closed = True
            self.is_flat = True
            self.is_open = False
            self.is_long = False
            self.is_short = False

    cdef void _increment_returns(self, OrderFillEvent event):
        if event.order_side == OrderSide.BUY:
            if self.market_position == MarketPosition.SHORT:
                # Increment realized points and return of a short position
                self.points_realized += self._calculate_points(self.average_entry_price, event.average_price)
                self.return_realized += self._calculate_return(self.average_entry_price, event.average_price)
        elif event.order_side == OrderSide.SELL:
            if self.market_position == MarketPosition.LONG:
                # Increment realized points and return of a long position
                self.points_realized += self._calculate_points(self.average_entry_price, event.average_price)
                self.return_realized += self._calculate_return(self.average_entry_price, event.average_price)

    cdef object _calculate_points(self, Price entry_price, Price exit_price):
        if self.entry_direction == OrderSide.BUY:
            return exit_price.value - entry_price.value
        elif self.entry_direction == OrderSide.SELL:
            return entry_price.value - exit_price.value
        else:
            raise ValueError(f'Cannot calculate the points of a {self.entry_direction} entry direction.')

    cdef float _calculate_return(self, Price entry_price, Price exit_price):
        if self.market_position == MarketPosition.LONG:
            return (exit_price.as_float() - entry_price.as_float()) / entry_price.as_float()
        elif self.market_position == MarketPosition.SHORT:
            return (entry_price.as_float() - exit_price.as_float()) / entry_price.as_float()
        else:
            raise ValueError(f'Cannot calculate the return of a {self.market_position} market position.')
