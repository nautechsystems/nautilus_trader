# -------------------------------------------------------------------------------------------------
# <copyright file="position.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from decimal import Decimal
from typing import List

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
        :param event: The order fill event which created the position.
        """
        self._order_ids = [event.order_id]                  # type: List[OrderId]
        self._execution_ids = [event.execution_id]          # type: List[ExecutionId]
        self._execution_tickets = [event.execution_ticket]  # type: List[ExecutionTicket]
        self._events = [event]                              # type: List[OrderFillEvent]

        self.symbol = event.symbol
        self.id = position_id
        self.account_id = event.account_id
        self.from_order_id = event.order_id
        self.last_order_id = event.order_id
        self.last_execution_id = event.execution_id
        self.last_execution_ticket = event.execution_ticket
        self.timestamp = event.execution_time
        self.relative_quantity = self._calculate_relative_quantity(event)
        self.quantity = event.filled_quantity
        self.peak_quantity = event.filled_quantity
        self.entry_direction = event.order_side
        self.entry_time = event.execution_time
        self.exit_time = None
        self.average_entry_price = event.average_price
        self.average_exit_price = None
        self.points_realized = Decimal(0)
        self.return_realized = 0.0
        self.is_closed = False
        self.last_event = event

        self._fill_logic(event)

    cdef bint equals(self, Position other):
        """
        Return a value indicating whether the object equals the given object.
        
        :param other: The other object to compare
        :return True if the objects are equal, otherwise False.
        """
        return self.id.equals(other.id)

    def __eq__(self, other) -> bool:
        """
        Return a value indicating whether this object is equal to the given object.

        :return bool.
        """
        return self.equals(other)

    def __ne__(self, other) -> bool:
        """
        Return a value indicating whether this object is not equal to the given object.

        :return bool.
        """
        return not self.equals(other)

    def __str__(self) -> str:
        """
        :return The str() string representation of the position.
        """
        return f"Position(id={self.id.value}) {self.status_string()}"

    def __repr__(self) -> str:
        """
        :return The repr() string representation of the position.
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
        Return a list of all order identifiers.
        
        :return List[OrderId]. 
        """
        return self._order_ids.copy()

    cpdef list get_execution_ids(self):
        """
        Return a list of all execution identifiers.
        
        :return List[ExecutionId]. 
        """
        return self._execution_ids.copy()

    cpdef list get_execution_tickets(self):
        """
        Return a list of all execution tickets.
        
        :return List[ExecutionTicket]. 
        """
        return self._execution_tickets.copy()

    cpdef list get_events(self):
        """
        Return a list of all order fill events.
        
        :return List[Event].
        """
        return self._events.copy()

    cpdef int event_count(self):
        """
        Return the count of events applied to the position.
        
        :return int.
        """
        return len(self._events)

    cpdef void apply(self, OrderFillEvent event):
        """
        Applies the given order fill event to the position.

        :param event: The order fill event to apply.
        """
        # Update events
        self._events.append(event)
        self.last_event = event

        # Update identifiers
        if event.order_id not in self._order_ids:
            self._order_ids.append(event.order_id)
        self._execution_ids.append(event.execution_id)
        self._execution_tickets.append(event.execution_ticket)
        self.last_order_id = event.order_id
        self.last_execution_id = event.execution_id
        self.last_execution_ticket = event.execution_ticket

        # Apply event
        self._set_quantities(event)
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

    cdef int _calculate_relative_quantity(self, OrderFillEvent event):
        if event.order_side == OrderSide.BUY:
            return event.filled_quantity.value
        elif event.order_side == OrderSide.SELL:
            return - event.filled_quantity.value

    cdef void _set_quantities(self, OrderFillEvent event):
        # Set relative quantity
        self.relative_quantity += self._calculate_relative_quantity(event)

        # Set quantity
        self.quantity = Quantity(abs(self.relative_quantity))

        # Set peak quantity
        if self.quantity > self.peak_quantity:
            self.peak_quantity = self.quantity

    cdef void _fill_logic(self, OrderFillEvent event):
        # Set market position
        if self.relative_quantity > 0:
            self.market_position = MarketPosition.LONG
            self.is_long = True
            self.is_flat = False
            self.is_short = False
        elif self.relative_quantity < 0:
            self.market_position = MarketPosition.SHORT
            self.is_short = True
            self.is_flat = False
            self.is_long = False
        else:
            self.market_position = MarketPosition.FLAT
            self.is_flat = True
            self.is_long = False
            self.is_short = False

        # Exit logic
        if self.relative_quantity == 0:
            self._exit_logic(event)

        if self.is_closed and self.relative_quantity != 0:
            self.is_closed = False

    cdef void _exit_logic(self, OrderFillEvent event):
            self.exit_time = event.timestamp
            self.average_exit_price = event.average_price
            self.is_closed = True

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
