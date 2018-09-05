#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="requests.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import abc

from datetime import datetime
from uuid import UUID

from inv_trader.core.typing import typechecking
from inv_trader.model.enums import Broker


class Request:
    """
    The abstract base class for all requests.
    """

    __metaclass__ = abc.ABCMeta

    @typechecking
    def __init__(self,
                 identifier: UUID,
                 timestamp: datetime):
        """
        Initializes a new instance of the Request abstract class.

        :param: identifier: The requests identifier.
        :param: uuid: The requests timestamp.
        """
        self._request_id = identifier
        self._request_timestamp = timestamp

    @property
    def request_id(self) -> UUID:
        """
        :return: The requests identifier.
        """
        return self._request_id

    @property
    def requests_timestamp(self) -> datetime:
        """
        :return: The requests timestamp (the time the request was created).
        """
        return self._request_timestamp

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.request_id == other.request_id
        else:
            return False

    def __ne__(self, other):
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the request.
        """
        attrs = vars(self)
        props = ', '.join("%s=%s" % item for item in attrs.items()).replace(', _', ', ')
        return f"{self.__class__.__name__}({props[1:]})"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the request.
        """
        return f"<{str(self)} object at {id(self)}>"


class RequestCollateralInquiry(Request):
    """
    Represents a request for a FIX collateral inquiry.
    """

    __metaclass__ = abc.ABCMeta

    @typechecking
    def __init__(self,
                 broker: Broker,
                 account_number: int,
                 identifier: UUID,
                 timestamp: datetime):
        """
        Initializes a new instance of the RequestCollateralInquiry class.

        :param: order: The commands order.
        :param: event_id: The commands identifier.
        :param: event_timestamp: The order commands timestamp.
        """
        super().__init__(identifier, timestamp)
        self._broker = broker
        self._account_number = account_number

    @property
    def broker(self) -> Broker:
        """
        :return: The brokerage for the collateral inquiry.
        """
        return self._broker

    @property
    def account_number(self) -> int:
        """
        :return: The account number for the collateral inquiry.
        """
        return self._account_number
