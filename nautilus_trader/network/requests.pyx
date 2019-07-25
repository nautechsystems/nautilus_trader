# -------------------------------------------------------------------------------------------------
# <copyright file="requests.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.message cimport Request
from nautilus_trader.model.c_enums.venue cimport Venue
from nautilus_trader.model.objects cimport Symbol, BarSpecification
from nautilus_trader.model.identifiers cimport GUID


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
