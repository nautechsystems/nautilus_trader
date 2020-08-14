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

from cpython.datetime cimport datetime

from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.security_type cimport SecurityType
from nautilus_trader.model.c_enums.bar_structure cimport BarStructure
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.identifiers cimport Symbol, InstrumentId


cdef class Quantity(Decimal):

    @staticmethod
    cdef Quantity zero()

    @staticmethod
    cdef Quantity from_string(str value)
    cpdef Quantity add(self, Quantity other)
    cpdef Quantity sub(self, Quantity other)
    cpdef str to_string_formatted(self)


cdef class Price(Decimal):
    @staticmethod
    cdef Price from_string(str value)
    cpdef Price add(self, Decimal other)
    cpdef Price sub(self, Decimal other)


cdef class Volume(Decimal):
    @staticmethod
    cdef Volume zero()

    @staticmethod
    cdef Volume one()

    @staticmethod
    cdef Volume from_string(str value)
    cpdef Volume add(self, Volume other)
    cpdef Volume sub(self, Volume other)


cdef class Money(Decimal):
    cdef readonly Currency currency

    @staticmethod
    cdef Money from_string(str value, Currency currency)
    cpdef Money add(self, Money other)
    cpdef Money sub(self, Money other)
    cpdef str to_string_formatted(self)


cdef class Tick:
    cdef readonly Symbol symbol
    cdef readonly Price bid
    cdef readonly Price ask
    cdef readonly Volume bid_size
    cdef readonly Volume ask_size
    cdef readonly datetime timestamp

    @staticmethod
    cdef Tick from_serializable_string_with_symbol(Symbol symbol, str values)

    @staticmethod
    cdef Tick from_serializable_string(str value)

    @staticmethod
    cdef Tick _parse(Symbol symbol, list splits)

    cpdef bint equals(self, Tick other)
    cpdef str to_string(self)
    cpdef str to_serializable_string(self)


cdef class BarSpecification:
    cdef readonly int step
    cdef readonly BarStructure structure
    cdef readonly PriceType price_type

    @staticmethod
    cdef BarSpecification from_string(str value)
    cdef str structure_string(self)
    cdef str price_type_string(self)
    cpdef bint equals(self, BarSpecification other)
    cpdef str to_string(self)


cdef class BarType:
    cdef readonly Symbol symbol
    cdef readonly BarSpecification specification

    @staticmethod
    cdef BarType from_string(str value)
    cdef str structure_string(self)
    cdef str price_type_string(self)
    cpdef bint equals(self, BarType other)
    cpdef str to_string(self)


cdef class Bar:
    cdef readonly Price open
    cdef readonly Price high
    cdef readonly Price low
    cdef readonly Price close
    cdef readonly Volume volume
    cdef readonly datetime timestamp
    cdef readonly bint checked

    @staticmethod
    cdef Bar from_serializable_string(str value)
    cpdef bint equals(self, Bar other)
    cpdef str to_string(self)
    cpdef str to_serializable_string(self)


cdef class Instrument:
    cdef readonly InstrumentId id
    cdef readonly Symbol symbol
    cdef readonly Currency quote_currency
    cdef readonly SecurityType security_type
    cdef readonly int price_precision
    cdef readonly int size_precision
    cdef readonly int min_stop_distance_entry
    cdef readonly int min_stop_distance
    cdef readonly int min_limit_distance_entry
    cdef readonly int min_limit_distance
    cdef readonly Price tick_size
    cdef readonly Quantity round_lot_size
    cdef readonly Quantity min_trade_size
    cdef readonly Quantity max_trade_size
    cdef readonly Decimal rollover_interest_buy
    cdef readonly Decimal rollover_interest_sell
    cdef readonly datetime timestamp


cdef class ForexInstrument(Instrument):
    cdef readonly Currency base_currency
