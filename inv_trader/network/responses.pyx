#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="responses.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime

from inv_trader.core.message cimport Message
from inv_trader.model.objects cimport Symbol, BarSpecification
from inv_trader.model.identifiers cimport GUID


cdef class Response(Message):
    """
    The base class for all responses.
    """

    def __init__(self,
                 GUID correlation_id,
                 GUID identifier,
                 datetime timestamp):
        """
        Initializes a new instance of the Response abstract class.

        :param identifier: The correlation identifier.
        :param identifier: The response identifier.
        :param timestamp: The response timestamp.
        """
        super().__init__(correlation_id, identifier, timestamp)


cdef class TickDataResponse(Response):
    """
    Represents a response of historical tick data.
    """

    def __init__(self,
                 Symbol symbol,
                 bytes[:] ticks,
                 GUID correlation_id,
                 GUID response_id,
                 datetime response_timestamp):
        """
        Initializes a new instance of the TickDataResponse class.

        :param symbol: The response symbol.
        :param ticks: The response ticks.
        :param correlation_id: The correlation identifier.
        :param response_id: The response identifier.
        :param response_timestamp: The response timestamp.
        """
        super().__init__(correlation_id, response_id, response_timestamp)
        self.symbol = symbol
        self.ticks = ticks


cdef class BarDataResponse(Response):
    """
    Represents a response of historical bar data.
    """

    def __init__(self,
                 Symbol symbol,
                 BarSpecification bar_spec,
                 bytes[:] bars,
                 GUID correlation_id,
                 GUID response_id,
                 datetime response_timestamp):
        """
        Initializes a new instance of the BarDataResponse class.

        :param symbol: The response symbol.
        :param bar_spec: The response bar specification.
        :param bars: The response bars.
        :param correlation_id: The correlation identifier.
        :param response_id: The response identifier.
        :param response_timestamp: The response timestamp.
        """
        super().__init__(correlation_id, response_id, response_timestamp)
        self.symbol = symbol
        self.bars = bars


cdef class InstrumentResponse(Response):
    """
    Represents a response of instrument data.
    """

    def __init__(self,
                 bytes[:] instruments,
                 GUID correlation_id,
                 GUID response_id,
                 datetime response_timestamp):
        """
        Initializes a new instance of the InstrumentResponse class.

        :param instruments: The response instruments.
        :param correlation_id: The correlation identifier.
        :param response_id: The response identifier.
        :param response_timestamp: The response timestamp.
        """
        super().__init__(correlation_id, response_id, response_timestamp)
        self.instruments = instruments
