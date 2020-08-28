# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.serialization.base cimport DataSerializer
from nautilus_trader.serialization.base cimport InstrumentSerializer


cdef class Utf8QuoteTickSerializer:
    @staticmethod
    cdef bytes serialize(QuoteTick tick)

    @staticmethod
    cdef list serialize_ticks_list(list ticks)

    @staticmethod
    cdef QuoteTick deserialize(Symbol symbol, bytes tick_bytes)

    @staticmethod
    cdef list deserialize_bytes_list(Symbol symbol, list tick_values)


cdef class Utf8TradeTickSerializer:
    @staticmethod
    cdef bytes serialize(TradeTick tick)

    @staticmethod
    cdef list serialize_ticks_list(list ticks)

    @staticmethod
    cdef TradeTick deserialize(Symbol symbol, bytes tick_bytes)

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

    cpdef dict map_quote_ticks(self, list ticks)
    cpdef dict map_trade_ticks(self, list ticks)
    cpdef dict map_bars(self, list bars, BarType bar_type)
    cpdef dict map_instruments(self, list instruments)


cdef class BsonInstrumentSerializer(InstrumentSerializer):
    pass
