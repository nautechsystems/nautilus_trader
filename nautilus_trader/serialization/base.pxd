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

from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.order cimport Order


cdef class Serializer:
    cdef object _re_camel_to_snake

    cdef inline str convert_camel_to_snake(self, str value)
    cdef inline str convert_snake_to_camel(self, str value)

    cpdef str py_convert_camel_to_snake(self, str value)
    cpdef str py_convert_snake_to_camel(self, str value)


cdef class DictionarySerializer(Serializer):
    cpdef bytes serialize(self, dict dictionary)
    cpdef dict deserialize(self, bytes dictionary_bytes)


cdef class InstrumentSerializer(Serializer):
    cpdef bytes serialize(self, Instrument instrument)
    cpdef Instrument deserialize(self, bytes instrument_bytes)


cdef class OrderSerializer(Serializer):
    cpdef bytes serialize(self, Order order)
    cpdef Order deserialize(self, bytes order_bytes)


cdef class CommandSerializer(Serializer):
    cpdef bytes serialize(self, Command command)
    cpdef Command deserialize(self, bytes command_bytes)


cdef class EventSerializer(Serializer):
    cpdef bytes serialize(self, Event event)
    cpdef Event deserialize(self, bytes event_bytes)
