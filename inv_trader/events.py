#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="events.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import abc
import datetime
import uuid


class Event:
    """
    The base class for all events.
    """

    __metaclass__ = abc.ABCMeta

    def __init__(self,
                 identifier: uuid,
                 timestamp: datetime.datetime):
        """
        Initializes a new instance of the TradeStrategy abstract class.

        :param: identifier: The events identifier.
        :param: uuid: The events timestamp.
        """
        self._id = identifier
        self._timestamp = timestamp

    @property
    def id(self) -> uuid:
        """
        :return: The events identifier.
        """
        return self._id

    @property
    def timestamp(self) -> datetime.datetime:
        """
        :return: The events timestamp (ISO8601).
        """
        return self._timestamp

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
        :return: The str() string representation of the tick.
        """
        return f"({self._id}){self._timestamp.isoformat()}"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the tick.
        """
        return f"<{str(self)} object at {id(self)}>"


class TimeEvent(Event):
    """
    Represents a time event occurring at the event timestamp.
    """

    def __init__(self,
                 label: str,
                 identifier: uuid,
                 timestamp: datetime.datetime):
        """
        Initializes a new instance of the TradeStrategy abstract class.

        :param: identifier: The time events identifier.
        :param: uuid: The time events timestamp.
        """
        super().__init__(identifier, timestamp)
        self._label = label

    @property
    def label(self) -> str:
        """
        :return: The time events label.
        """
        return self._label
