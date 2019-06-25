#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="responses.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime

from nautilus_trader.core.message cimport Response
from nautilus_trader.model.objects cimport Symbol, Tick, BarSpecification, Bar, Instrument
from nautilus_trader.model.identifiers cimport GUID


cdef class TickDataResponse(Response):
    """
    Represents a response of historical tick data.
    """

    def __init__(self,
                 Symbol symbol,
                 Tick[:] ticks,
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
                 Bar[:] bars,
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
                 Instrument[:] instruments,
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
