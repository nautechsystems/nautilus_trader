#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="position.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from datetime import datetime
from decimal import Decimal
from typing import List, Optional

from inv_trader.core.checks import typechecking
from inv_trader.model.enums import MarketPosition, OrderSide
from inv_trader.model.objects import Symbol
from inv_trader.model.events import OrderEvent
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled

# Constants
OrderId = str
PositionId = str
ExecutionId = str


class Position:
    """
    Represents a position in a financial market.
    """

    @typechecking
    def __init__(self,
                 symbol: Symbol,
                 position_id: PositionId,
                 timestamp: datetime):
        """
        Initializes a new instance of the Position class.

        :param: symbol: The orders symbol.
        :param: position_id: The positions identifier.
        :param: timestamp: The positions initialization timestamp.
        """
        self._symbol = symbol
        self._id = position_id
        self._timestamp = timestamp
        self._relative_quantity = 0
        self._peak_quantity = 0
        self._entry_time = None
        self._exit_time = None
        self._average_entry_price = None
        self._average_exit_price = None
        self._events = []               # type: List[OrderEvent]
        self._execution_ids = []        # type: List[ExecutionId]
        self._execution_tickets = []    # type: List[ExecutionId]

    @property
    def symbol(self) -> Symbol:
        """
        :return: The positions symbol.
        """
        return self._symbol

    @property
    def id(self) -> str:
        """
        :return: The positions identifier.
        """
        return self._id

    @property
    def from_entry_order_id(self) -> str:
        """
        :return: The position from entry orders identifier.
        """
        return self.from_entry_order_id

    @property
    def execution_ids(self) -> List[str]:
        """
        :return: The positions list of execution identifiers.
        """
        return self._execution_ids

    @property
    def execution_tickets(self) -> List[str]:
        """
        :return: The positions list of execution tickets.
        """
        return self._execution_tickets

    @property
    def quantity(self) -> int:
        """
        :return: The positions quantity.
        """
        return abs(self._relative_quantity)

    @property
    def timestamp(self) -> datetime:
        """
        :return: The positions initialization timestamp.
        """
        return self._timestamp

    @property
    def average_entry_price(self) -> Optional[Decimal]:
        """
        :return: The positions average filled entry price (optional could be None).
        """
        return self._average_entry_price

    @property
    def average_exit_price(self) -> Optional[Decimal]:
        """
        :return: The positions average filled exit price (optional could be None).
        """
        return self._average_exit_price

    @property
    def entry_time(self) -> Optional[datetime]:
        """
        :return: The positions market entry time (optional could be None).
        """
        return self._entry_time

    @property
    def exit_time(self) -> Optional[datetime]:
        """
        :return: The positions market exit time (optional could be None).
        """
        return self._exit_time

    @property
    def is_entered(self) -> bool:
        """
        :return: A value indicating whether the position has entered into the market.
        """
        return self._entry_time is not None

    @property
    def is_exited(self) -> bool:
        """
        :return: A value indicating whether the position has exited from the market.
        """
        return self._exit_time is not None

    @property
    def market_position(self) -> MarketPosition:
        """
        :return: The positions current market position.
        """
        return self._calculate_market_position()

    @property
    def event_count(self) -> int:
        """
        :return: The count of events since the position was initialized.
        """
        return len(self._events)

    @property
    def events(self) -> List[OrderEvent]:
        """
        :return: The positions internal events list.
        """
        return self._events

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.id == other.id
        else:
            return False

    def __ne__(self, other):
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the position.
        """
        return f"Position: {self._id}"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the position.
        """
        return f"<{str(self)} object at {id(self)}>"

    @typechecking
    def apply(self, event: OrderEvent):
        """
        Applies the given order event to the position.

        :param event: The order event to apply.
        """
        self._events.append(event)

        # Handle event
        if isinstance(event, OrderFilled):
            self._update_position(
                event.order_side,
                event.filled_quantity,
                event.average_price,
                event.execution_time)
            self._execution_ids.append(event.execution_id)
            self._execution_tickets.append(event.execution_ticket)
        elif isinstance(event, OrderPartiallyFilled):
            self._update_position(
                event.order_side,
                event.filled_quantity,
                event.average_price,
                event.execution_time)
            self._execution_ids.append(event.execution_id)
            self._execution_tickets.append(event.execution_ticket)
        else:
            raise TypeError("Cannot apply event (unrecognized event).")

    @typechecking
    def _update_position(
            self,
            order_side: OrderSide,
            quantity: int,
            average_price: Decimal,
            event_time: datetime):
        if order_side is OrderSide.BUY:
            self._relative_quantity += quantity
        elif order_side is OrderSide.SELL:
            self._relative_quantity -= quantity

        # Update the peak quantity
        if abs(self._relative_quantity) > self._peak_quantity:
            self._peak_quantity = self._relative_quantity

        # Capture the first time of entry
        if self._entry_time is None:
            self._entry_time = event_time

        self._average_entry_price = average_price

        # Position was exited
        if self.is_entered and self._relative_quantity == 0:
            self._exit_time = event_time
            self._average_exit_price = average_price

    def _calculate_market_position(self) -> MarketPosition:
        if self._relative_quantity > 0:
            return MarketPosition.LONG
        elif self._relative_quantity < 0:
            return MarketPosition.SHORT
        else:
            return MarketPosition.FLAT
