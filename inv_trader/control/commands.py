#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="commands.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import abc

from datetime import datetime
from decimal import Decimal
from uuid import UUID

from inv_trader.core.preconditions import Precondition
from inv_trader.model.enums import Broker
from inv_trader.model.order import Order


class Command:
    """
    The abstract base class for all commands.
    """

    __metaclass__ = abc.ABCMeta

    def __init__(self,
                 identifier: UUID,
                 timestamp: datetime):
        """
        Initializes a new instance of the Command abstract class.

        :param: identifier: The commands identifier.
        :param: uuid: The commands timestamp.
        """
        self._command_id = identifier
        self._command_timestamp = timestamp

    @property
    def command_id(self) -> UUID:
        """
        :return: The commands identifier.
        """
        return self._command_id

    @property
    def command_timestamp(self) -> datetime:
        """
        :return: The commands timestamp (the time the command was created).
        """
        return self._command_timestamp

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

    def __str__(self) -> str:
        """
        :return: The str() string representation of the command.
        """
        attrs = vars(self)
        props = ', '.join("%s=%s" % item for item in attrs.items()).replace(', _', ', ')
        return f"{self.__class__.__name__}({props[1:]})"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the command.
        """
        return f"<{str(self)} object at {id(self)}>"


class OrderCommand(Command):
    """
    The abstract base class for all order commands.
    """

    __metaclass__ = abc.ABCMeta

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
    Represents a command to submit the given order.
    """

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
    Represents a command to cancel the given order.
    """

    def __init__(self,
                 order: Order,
                 cancel_reason: str,
                 command_id: UUID,
                 command_timestamp: datetime):
        """
        Initializes a new instance of the CancelOrder class.

        :param: order: The commands order to cancel.
        :param: cancel_reason: The reason for cancellation.
        :param: event_id: The commands identifier.
        :param: event_timestamp: The commands timestamp.
        """
        Precondition.valid_string(cancel_reason, 'cancel_reason')

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
        Precondition.positive(modified_price, 'modified_price')

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


class CollateralInquiry(Command):
    """
    Represents a request for a FIX collateral inquiry of all connected accounts.
    """

    def __init__(self,
                 identifier: UUID,
                 timestamp: datetime):
        """
        Initializes a new instance of the CollateralInquiry class.

        :param: event_id: The commands identifier.
        :param: event_timestamp: The order commands timestamp.
        """
        super().__init__(identifier, timestamp)
