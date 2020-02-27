# -------------------------------------------------------------------------------------------------
# <copyright file="data.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.cache cimport ObjectCache
from nautilus_trader.core.types cimport GUID
from nautilus_trader.core.message cimport Response
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.network.identifiers cimport ClientId
from nautilus_trader.network.messages cimport DataResponse
from nautilus_trader.serialization.constants cimport *
from nautilus_trader.serialization.base cimport DataSerializer, InstrumentSerializer


cdef class LiveDataClient(DataClient):
    cdef object _zmq_context
    cdef object _tick_req_worker
    cdef object _tick_sub_worker
    cdef object _bar_req_worker
    cdef object _bar_sub_worker
    cdef object _inst_req_worker
    cdef object _inst_sub_worker
    cdef DataSerializer _data_serializer
    cdef InstrumentSerializer _instrument_serializer
    cdef ObjectCache _cached_symbols
    cdef ObjectCache _cached_bar_types
    cdef object _response_queue
    cdef object _response_thread
    cdef dict _correlation_index

    cdef readonly TraderId trader_id
    cdef readonly ClientId client_id

    cpdef void _set_callback(self, GUID request_id, handler: callable) except *
    cpdef void _pop_callback(self, GUID correlation_id, list data) except *
    cpdef void _handle_response(self, Response response) except *
    cpdef void _handle_data_response(self, DataResponse response) except *
    cpdef void _put_response(self, Response response) except *
    cpdef void _pop_response(self) except *
    cpdef void _handle_instruments_py(self, list instruments) except *
    cpdef void _handle_tick_sub(self, str topic, bytes payload) except *
    cpdef void _handle_bar_sub(self, str topic, bytes payload) except *
    cpdef void _handle_inst_sub(self, str topic, bytes payload) except *
