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
from uuid import UUID

from inv_trader.core.checks import typechecking
from inv_trader.model.enums import OrderSide, OrderType, TimeInForce
from inv_trader.model.objects import Symbol


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
