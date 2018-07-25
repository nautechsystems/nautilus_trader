#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="commands.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import abc
import uuid

from datetime import datetime
from decimal import Decimal
from uuid import UUID
from typing import List

from inv_trader.core.checks import typechecking
from inv_trader.model.objects import Symbol
from inv_trader.model.order import Order

# Constants
OrderId = str
Ticket = str


class Command:
    """
    The abstract base class for all commands.
    """

    __metaclass__ = abc.ABCMeta

    def __init__(self,
                 identifier: uuid,
                 timestamp: datetime):
        """
        Initializes a new instance of the Command abstract class.

        :param: identifier: The commands identifier.
        :param: uuid: The commands timestamp.
        """
        self._id = identifier
        self._timestamp = timestamp

    @property
    def command_id(self) -> uuid:
        """
        :return: The commands identifier.
        """
        return self._id

    @property
    def command_timestamp(self) -> datetime:
        """
        :return: The commands timestamp (the time the command was created).
        """
        return self._timestamp

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.command_id == other.command_id
        else:
            return False

    def __ne__(self, other):
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)


class OrderCommand(Command):
    """
    The abstract base class for all order commands.
    """

    __metaclass__ = abc.ABCMeta

    @typechecking
    def __init__(self,
                 order_symbol: Symbol,
                 order_id: str,
                 command_id: UUID,
                 command_timestamp: datetime):
        """
        Initializes a new instance of the OrderCommand abstract class.

        :param: order_symbol: The commands order symbol.
        :param: order_id: The commands order identifier.
        :param: event_id: The commands identifier.
        :param: event_timestamp: The order commands timestamp.
        """
        super().__init__(command_id, command_timestamp)
        self._symbol = order_symbol
        self._order_id = order_id

    @property
    def symbol(self) -> Symbol:
        """
        :return: The commands order symbol.
        """
        return self._symbol

    @property
    def order_id(self) -> str:
        """
        :return: The commands order identifier.
        """
        return self._order_id


class SubmitOrder(OrderCommand):
    """
    Represents a command to submit the contained order.
    """

    @typechecking
    def __init__(self,
                 order: Order,
                 command_id: UUID,
                 command_timestamp: datetime):
        """
        Initializes a new instance of the SubmitOrder class.

        :param: order: The commands order to submit.
        :param: event_id: The commands identifier.
        :param: event_timestamp: The commands timestamp.
        """
        super().__init__(
            order.symbol,
            order.id,
            command_id,
            command_timestamp)

        self._order = order

    @property
    def order(self) -> Order:
        """
        :return: The commands order to submit.
        """
        return self._order


class CancelOrder(OrderCommand):
    """
    Represents a command to cancel the order corresponding to the given order
    identifier.
    """

    @typechecking
    def __init__(self,
                 order_symbol: Symbol,
                 order_id: OrderId,
                 command_id: UUID,
                 command_timestamp: datetime):
        """
        Initializes a new instance of the CancelOrder class.

        :param: order_symbol: The commands order symbol.
        :param: order: The commands order identifier to cancel.
        :param: event_id: The commands identifier.
        :param: event_timestamp: The commands timestamp.
        """
        super().__init__(
            order_symbol,
            order_id,
            command_id,
            command_timestamp)


class ModifyOrder(OrderCommand):
    """
    Represents a command to modify the order corresponding to the given order
    identifier with the given modified price.
    """

    @typechecking
    def __init__(self,
                 order_symbol: Symbol,
                 order_id: OrderId,
                 modified_price: Decimal,
                 command_id: UUID,
                 command_timestamp: datetime):
        """
        Initializes a new instance of the ModifyOrder class.

        :param: order_symbol: The commands order symbol.
        :param: order_id: The commands order identifier to modify.
        :param: event_id: The commands identifier.
        :param: event_timestamp: The commands timestamp.
        """
        super().__init__(
            order_symbol,
            order_id,
            command_id,
            command_timestamp)

        self._modified_price = modified_price

    @property
    def modified_price(self) -> Decimal:
        """
        :return: The commands price to modify the order to.
        """
        return self._modified_price


class ClosePosition(OrderCommand):
    """
    Represents a command to close the position corresponding to the given ticket.
    """

    @typechecking
    def __init__(self,
                 symbol: Symbol,
                 from_order_id: OrderId,
                 tickets: list,
                 command_id: UUID,
                 command_timestamp: datetime):
        """
        Initializes a new instance of the ClosePosition class.

        :param: symbol: The commands position symbol.
        :param: from_order_id: The commands order id the position entered from.
        :param: tickets: The commands position tickets to close.
        :param: event_id: The commands identifier.
        :param: event_timestamp: The commands timestamp.
        """
        super().__init__(
            symbol,
            from_order_id,
            command_id,
            command_timestamp)

        self._tickets = tickets

    @property
    def tickets(self) -> List[Ticket]:
        """
        :return: The commands position ticket list.
        """
        return self._tickets
