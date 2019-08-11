# -------------------------------------------------------------------------------------------------
# <copyright file="data.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.typed_collections cimport ObjectCache
from nautilus_trader.core.message cimport Response
from nautilus_trader.model.objects cimport Venue, Symbol, BarType, Instrument
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.guid cimport LiveGuidFactory
from nautilus_trader.common.logger cimport LiveLogger
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.network.workers import RequestWorker, SubscriberWorker
from nautilus_trader.serialization.base cimport DataSerializer, InstrumentSerializer, RequestSerializer, ResponseSerializer
from nautilus_trader.serialization.data cimport BsonDataSerializer, BsonInstrumentSerializer
from nautilus_trader.serialization.constants cimport *
from nautilus_trader.serialization.common cimport parse_symbol, parse_tick, parse_bar_type, parse_bar, convert_datetime_to_string
from nautilus_trader.serialization.serializers cimport MsgPackRequestSerializer, MsgPackResponseSerializer
from nautilus_trader.network.requests cimport DataRequest
from nautilus_trader.network.responses cimport MessageRejected, QueryFailure
from nautilus_trader.trade.strategy cimport TradingStrategy
from nautilus_trader.serialization.common import parse_symbol, parse_bar_type


cdef class LiveDataClient(DataClient):
    """
    Provides a data client for live trading.
    """
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
