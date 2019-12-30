# -------------------------------------------------------------------------------------------------
# <copyright file="data.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.cache cimport ObjectCache
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.serialization.constants cimport *
from nautilus_trader.serialization.base cimport (
    DataSerializer, 
    InstrumentSerializer, 
    RequestSerializer, 
    ResponseSerializer)


cdef class LiveDataClient(DataClient):
    cdef object _zmq_context
    cdef object _tick_req_worker
    cdef object _tick_sub_worker
    cdef object _bar_req_worker
    cdef object _bar_sub_worker
    cdef object _inst_req_worker
    cdef object _inst_sub_worker
    cdef RequestSerializer _request_serializer
    cdef ResponseSerializer _response_serializer
    cdef DataSerializer _data_serializer
    cdef InstrumentSerializer _instrument_serializer
    cdef ObjectCache _cached_symbols
    cdef ObjectCache _cached_bar_types

    cpdef void _handle_instruments_py(self, list instruments)
    cpdef void _handle_tick_sub(self, str topic, bytes message)
    cpdef void _handle_bar_sub(self, str topic, bytes message)
    cpdef void _handle_inst_sub(self, str topic, bytes message)
