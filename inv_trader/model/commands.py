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

from inv_trader.core.checks import typechecking
from inv_trader.model.order import Order


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
                 order: Order,
                 command_id: UUID,
                 command_timestamp: datetime):
        """
        Initializes a new instance of the OrderCommand abstract class.

        :param: order: The commands order.
        :param: event_id: The commands identifier.
        :param: event_timestamp: The order commands timestamp.
        """
        super().__init__(command_id, command_timestamp)
        self._order = order

    @property
    def order(self) -> Order:
        """
        :return: The commands order.
        """
        return self._order


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
            order,
            command_id,
            command_timestamp)


class CancelOrder(OrderCommand):
    """
    Represents a command to cancel the order corresponding to the given order
    identifier.
    """

    @typechecking
    def __init__(self,
                 order: Order,
                 cancel_reason: str,
                 command_id: UUID,
                 command_timestamp: datetime):
        """
        Initializes a new instance of the CancelOrder class.

        :param: order: The commands order to cancel.
        :param: cancel_reason: The order cancel reason.
        :param: event_id: The commands identifier.
        :param: event_timestamp: The commands timestamp.
        """
        super().__init__(
            order,
            command_id,
            command_timestamp)

        self._cancel_reason = cancel_reason

    @property
    def cancel_reason(self) -> str:
        """
        :return: The commands order cancel reason.
        """
        return self._cancel_reason


class ModifyOrder(OrderCommand):
    """
    Represents a command to modify the given order with the given modified price.
    """

    @typechecking
    def __init__(self,
                 order: Order,
                 modified_price: Decimal,
                 command_id: UUID,
                 command_timestamp: datetime):
        """
        Initializes a new instance of the ModifyOrder class.

        :param: order: The commands order to modify.
        :param: modified_price: The commands modified price for the order.
        :param: event_id: The commands identifier.
        :param: event_timestamp: The commands timestamp.
        """
        super().__init__(
            order,
            command_id,
            command_timestamp)

        self._modified_price = modified_price

    @property
    def modified_price(self) -> Decimal:
        """
        :return: The commands modified price for the order.
        """
        return self._modified_price
