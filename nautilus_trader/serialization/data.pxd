# -------------------------------------------------------------------------------------------------
# <copyright file="data.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.objects cimport BarType, Instrument
from nautilus_trader.serialization.base cimport DataSerializer, InstrumentSerializer


cdef class DataMapper:
    """
    Provides a data mapper for data objects.
    """
    cpdef dict map_ticks(self, list ticks)
    cpdef dict map_bars(self, list bars, BarType bar_type)
    cpdef dict map_instruments(self, list instruments)


cdef class BsonInstrumentSerializer(InstrumentSerializer):
    """
    Provides an instrument serializer for the MessagePack specification.
    """
    cpdef bytes serialize(self, Instrument instrument)
    cpdef Instrument deserialize(self, bytes instrument_bytes)


cdef class BsonDataSerializer(DataSerializer):
    """
    Provides a serializer for data objects to BSON specification.
    """
    cpdef bytes serialize(self, dict data)
    cpdef dict deserialize(self, bytes data_bytes)
