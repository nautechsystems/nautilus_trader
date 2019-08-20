# -------------------------------------------------------------------------------------------------
# <copyright file="position.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from decimal import Decimal
from typing import List

from nautilus_trader.model.c_enums.market_position cimport MarketPosition, market_position_string
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.events cimport OrderFillEvent
from nautilus_trader.model.identifiers cimport PositionId, ExecutionId, ExecutionTicket


cdef class Position:
    """
    Represents a position in a financial market.
    """

    def __init__(self,
                 PositionId position_id,
                 OrderFillEvent fill_event):
        """
        Initializes a new instance of the Position class.

        :param position_id: The positions identifier.
        :param fill_event: The order fill event which created the position.
        """
        self._order_ids = [fill_event.order_id]                  # type: List[OrderId]
        self._execution_ids = [fill_event.execution_id]          # type: List[ExecutionId]
        self._execution_tickets = [fill_event.execution_ticket]  # type: List[ExecutionTicket]
        self._events = [fill_event]                              # type: List[OrderFillEvent]

        self.symbol = fill_event.symbol
        self.id = position_id
        self.from_order_id = fill_event.order_id
        self.last_order_id = fill_event.order_id
        self.last_execution_id = fill_event.execution_id
        self.last_execution_ticket = fill_event.execution_ticket
        self.timestamp = fill_event.execution_time
        self.entry_direction = fill_event.order_side
        self.entry_time = fill_event.execution_time
        self.exit_time = None
        self.average_entry_price = fill_event.average_price
        self.average_exit_price = None
        self.points_realized = Decimal(0)
        self.return_realized = 0.0
        self.is_entered = True
        self.is_exited = False
        self.last_event = fill_event

        self._fill_logic(fill_event)

    cdef bint equals(self, Position other):
        """
        Return a value indicating whether the object equals the given object.
        
        :param other: The other object to compare
        :return: True if the objects are equal, otherwise False.
        """
        return self.id.equals(other.id)

    def __eq__(self, other) -> bool:
        """
        Return a value indicating whether this object is equal to the given object.

        :return: bool.
        """
        return self.equals(other)

    def __ne__(self, other) -> bool:
        """
        Return a value indicating whether this object is not equal to the given object.

        :return: bool.
        """
        return not self.equals(other)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the position.
        """
        return f"Position(id={self.id.value}) {self.status_string()}"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the position.
        """
        return f"<{str(self)} object at {id(self)}>"

    cdef str status_string(self):
        """
        Return the positions status as a string.

        :return: str.
        """
        cdef str quantity = '' if self.relative_quantity == 0 else ' {:,}'.format(self.quantity.value)
        return f"{self.symbol} {market_position_string(self.market_position)}{quantity}"

    cpdef list get_order_ids(self):
        """
        Return a list of all order identifiers.
        
        :return: List[OrderId]. 
        """
        return self._order_ids.copy()

    cpdef list get_execution_ids(self):
        """
        Return a list of all execution identifiers.
        
        :return: List[ExecutionId]. 
        """
        return self._execution_ids.copy()

    cpdef list get_execution_tickets(self):
        """
        Return a list of all execution tickets.
        
        :return: List[ExecutionTicket]. 
        """
        return self._execution_tickets.copy()

    cpdef list get_events(self):
        """
        Return a list of all order fill events.
        
        :return: List[Event].
        """
        return self._events.copy()

    cpdef int event_count(self):
        """
        Return the count of events applied to the position.
        
        :return: int.
        """
        return len(self._events)

    cpdef void apply(self, OrderFillEvent fill_event):
        """
        Applies the given order fill event to the position.

        :param fill_event: The order fill event to apply.
        """
        # Update events
        self._events.append(fill_event)
        self.last_event = fill_event

        # Update identifiers
        if fill_event.order_id not in self._order_ids:
            self._order_ids.append(fill_event.order_id)
        self._execution_ids.append(fill_event.execution_id)
        self._execution_tickets.append(fill_event.execution_ticket)
        self.last_order_id = fill_event.order_id
        self.last_execution_id = fill_event.execution_id
        self.last_execution_ticket = fill_event.execution_ticket

        # Apply event
        self._increment_returns(fill_event)
        self._fill_logic(fill_event)

    cpdef object points_unrealized(self, Price current_price):
        """
        Return the calculated unrealized points from the given current price.
         
        :param current_price: The current price of the position instrument.
        :return: Decimal.
        """
        if self.is_exited:
            return Decimal(0)
        return self._calculate_points(self.average_entry_price, current_price)

    cpdef float return_unrealized(self, Price current_price):
        """
        Return the calculated unrealized return from the given current price.
         
        :param current_price: The current price of the position instrument.
        :return: float.
        """
        if self.is_exited:
            return 0.0
        return self._calculate_return(self.average_entry_price, current_price)

    cdef void _fill_logic(self, OrderFillEvent fill_event):
        # Update relative quantity
        if fill_event.order_side is OrderSide.BUY:
            self.relative_quantity += fill_event.filled_quantity.value
        elif fill_event.order_side is OrderSide.SELL:
            self.relative_quantity -= fill_event.filled_quantity.value

        # Update quantity
        self.quantity = Quantity(abs(self.relative_quantity))

        # Update peak quantity
        if self.quantity > self.peak_quantity:
            self.peak_quantity = self.quantity

        # Update market position
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
            self._exit_logic(fill_event)

        if self.is_exited and self.relative_quantity != 0:
            self.is_exited = False

    cdef void _exit_logic(self, OrderFillEvent fill_event):
            self.exit_time = fill_event.timestamp
            self.average_exit_price = fill_event.average_price
            self.is_exited = True

    cdef void _increment_returns(self, OrderFillEvent fill_event):
        if fill_event.order_side is OrderSide.BUY:
            if self.market_position == MarketPosition.SHORT:
                # Increment realized points and return of a short position
                self.points_realized += self._calculate_points(self.average_entry_price, fill_event.average_price)
                self.return_realized += self._calculate_return(self.average_entry_price, fill_event.average_price)
        elif fill_event.order_side is OrderSide.SELL:
            if self.market_position == MarketPosition.LONG:
                # Increment realized points and return of a long position
                self.points_realized += self._calculate_points(self.average_entry_price, fill_event.average_price)
                self.return_realized += self._calculate_return(self.average_entry_price, fill_event.average_price)

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
