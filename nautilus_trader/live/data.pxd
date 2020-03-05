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
from nautilus_trader.network.node_clients cimport MessageClient, MessageSubscriber
from nautilus_trader.serialization.constants cimport *
from nautilus_trader.serialization.base cimport DataSerializer, InstrumentSerializer


cdef class LiveDataClient(DataClient):
    cdef MessageClient _data_client
    cdef MessageSubscriber _data_subscriber
    cdef MessageSubscriber _tick_subscriber
    cdef DataSerializer _data_serializer
    cdef InstrumentSerializer _instrument_serializer
    cdef ObjectCache _cached_symbols
    cdef ObjectCache _cached_bar_types
    cdef dict _correlation_index

    cdef readonly TraderId trader_id
    cdef readonly ClientId client_id
    cdef readonly GUID last_request_id

    cpdef void _set_callback(self, GUID request_id, handler: callable) except *
    cpdef object _pop_callback(self, GUID correlation_id)
    cpdef void _handle_response(self, Response response) except *
    cpdef void _handle_data_response(self, DataResponse response) except *
    cpdef void _handle_instruments_py(self, list instruments) except *
    cpdef void _handle_tick_msg(self, str topic, bytes payload) except *
    cpdef void _handle_sub_msg(self, str topic, bytes payload) except *
