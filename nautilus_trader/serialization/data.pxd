# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Tick, Bar, BarType
from nautilus_trader.serialization.base cimport DataSerializer, InstrumentSerializer


cdef class Utf8TickSerializer:
    @staticmethod
    cdef bytes serialize(Tick tick)
    @staticmethod
    cdef list serialize_ticks_list(list ticks)
    @staticmethod
    cdef Tick deserialize(Symbol symbol, bytes tick_bytes)
    @staticmethod
    cdef list deserialize_bytes_list(Symbol symbol, list tick_values)


cdef class Utf8BarSerializer:
    @staticmethod
    cdef bytes serialize(Bar bar)
    @staticmethod
    cdef list serialize_bars_list(list bars)
    @staticmethod
    cdef Bar deserialize(bytes bar_bytes)
    @staticmethod
    cdef list deserialize_bytes_list(list bar_values)


cdef class BsonDataSerializer(DataSerializer):
    pass


cdef class DataMapper:
    cdef InstrumentSerializer instrument_serializer

    cpdef dict map_ticks(self, list ticks)
    cpdef dict map_bars(self, list bars, BarType bar_type)
    cpdef dict map_instruments(self, list instruments)


cdef class BsonInstrumentSerializer(InstrumentSerializer):
    pass
