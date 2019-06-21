#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="requests.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime

from inv_trader.core.message cimport Request
from inv_trader.enums.venue cimport Venue
from inv_trader.model.objects cimport Symbol, BarSpecification
from inv_trader.model.identifiers cimport GUID


cdef class TickDataRequest(Request):
    """
    Represents a request for historical tick data.
    """

    def __init__(self,
                 Symbol symbol,
                 datetime from_datetime,
                 datetime to_datetime,
                 GUID request_id,
                 datetime request_timestamp):
        """
        Initializes a new instance of the TickDataRequest class.

        :param symbol: The request symbol.
        :param from_datetime: The request from datetime.
        :param to_datetime: The request to datetime.
        :param request_id: The request identifier.
        :param request_timestamp: The request timestamp.
        """
        super().__init__(request_id, request_timestamp)
        self.symbol = symbol
        self.from_datetime = from_datetime
        self.to_datetime = to_datetime


cdef class BarDataRequest(Request):
    """
    Represents a request for historical bar data.
    """

    def __init__(self,
                 Symbol symbol,
                 BarSpecification bar_spec,
                 datetime from_datetime,
                 datetime to_datetime,
                 GUID request_id,
                 datetime request_timestamp):
        """
        Initializes a new instance of the BarDataRequest class.

        :param symbol: The request symbol.
        :param bar_spec: The request bar specification.
        :param from_datetime: The request from datetime.
        :param to_datetime: The request to datetime.
        :param request_id: The request identifier.
        :param request_timestamp: The request timestamp.
        """
        super().__init__(request_id, request_timestamp)
        self.symbol = symbol
        self.bar_spec = bar_spec
        self.from_datetime = from_datetime
        self.to_datetime = to_datetime


cdef class InstrumentRequest(Request):
    """
    Represents a request for an instrument.
    """

    def __init__(self,
                 Symbol symbol,
                 GUID request_id,
                 datetime request_timestamp):
        """
        Initializes a new instance of the InstrumentRequest class.

        :param symbol: The request symbol.
        :param request_id: The request identifier.
        :param request_timestamp: The request timestamp.
        """
        super().__init__(request_id, request_timestamp)
        self.symbol = symbol


cdef class InstrumentsRequest(Request):
    """
    Represents a request for all instruments for a venue.
    """

    def __init__(self,
                 Venue venue,
                 GUID request_id,
                 datetime request_timestamp):
        """
        Initializes a new instance of the InstrumentsRequest class.

        :param venue: The request venue.
        :param request_id: The request identifier.
        :param request_timestamp: The request timestamp.
        """
        super().__init__(request_id, request_timestamp)
        self.venue = venue
