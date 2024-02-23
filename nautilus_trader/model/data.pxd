# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.mem cimport PyMem_Free
from cpython.pycapsule cimport PyCapsule_GetPointer
from libc.stdint cimport int64_t
from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.rust.model cimport AggressorSide
from nautilus_trader.core.rust.model cimport Bar_t
from nautilus_trader.core.rust.model cimport BarSpecification_t
from nautilus_trader.core.rust.model cimport BarType_t
from nautilus_trader.core.rust.model cimport BookAction
from nautilus_trader.core.rust.model cimport BookOrder_t
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.core.rust.model cimport HaltReason
from nautilus_trader.core.rust.model cimport InstrumentCloseType
from nautilus_trader.core.rust.model cimport MarketStatus
from nautilus_trader.core.rust.model cimport OrderBookDelta_t
from nautilus_trader.core.rust.model cimport OrderBookDeltas_API
from nautilus_trader.core.rust.model cimport OrderBookDepth10_t
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.core.rust.model cimport QuoteTick_t
from nautilus_trader.core.rust.model cimport TradeTick_t
from nautilus_trader.model.data cimport BarAggregation
from nautilus_trader.model.data cimport OrderBookDelta
from nautilus_trader.model.data cimport OrderBookDeltas
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cpdef list capsule_to_list(capsule)
cpdef Data capsule_to_data(capsule)

cdef inline void capsule_destructor(object capsule):
    cdef CVec *cvec = <CVec *>PyCapsule_GetPointer(capsule, NULL)
    PyMem_Free(cvec[0].ptr) # de-allocate buffer
    PyMem_Free(cvec) # de-allocate cvec


cdef inline void capsule_destructor_deltas(object capsule):
    cdef OrderBookDeltas_API *data = <OrderBookDeltas_API *>PyCapsule_GetPointer(capsule, NULL)
    PyMem_Free(data)


cdef class DataType:
    cdef frozenset _key
    cdef int _hash
    cdef str _metadata_str

    cdef readonly type type
    """The `Data` type of the data.\n\n:returns: `type`"""
    cdef readonly dict metadata
    """The data types metadata.\n\n:returns: `dict[str, object]`"""
    cdef readonly str topic
    """The data types topic string.\n\n:returns: `str`"""


cdef class CustomData(Data):
    cdef readonly DataType data_type
    """The data type.\n\n:returns: `DataType`"""
    cdef readonly Data data
    """The data.\n\n:returns: `Data`"""


cpdef enum BarAggregation:
    TICK = 1
    TICK_IMBALANCE = 2
    TICK_RUNS = 3
    VOLUME = 4
    VOLUME_IMBALANCE = 5
    VOLUME_RUNS = 6
    VALUE = 7
    VALUE_IMBALANCE = 8
    VALUE_RUNS = 9
    MILLISECOND = 10
    SECOND = 11
    MINUTE = 12
    HOUR = 13
    DAY = 14
    WEEK = 15
    MONTH = 16


cdef class BarSpecification:
    cdef BarSpecification_t _mem

    cdef str to_str(self)
    cdef str aggregation_string_c(self)

    @staticmethod
    cdef BarSpecification from_mem_c(BarSpecification_t raw)

    @staticmethod
    cdef BarSpecification from_str_c(str value)

    @staticmethod
    cdef bint check_time_aggregated_c(BarAggregation aggregation)

    @staticmethod
    cdef bint check_threshold_aggregated_c(BarAggregation aggregation)

    @staticmethod
    cdef bint check_information_aggregated_c(BarAggregation aggregation)

    cpdef bint is_time_aggregated(self)
    cpdef bint is_threshold_aggregated(self)
    cpdef bint is_information_aggregated(self)

    @staticmethod
    cdef BarSpecification from_mem_c(BarSpecification_t raw)


cdef class BarType:
    cdef BarType_t _mem

    cdef str to_str(self)

    @staticmethod
    cdef BarType from_mem_c(BarType_t raw)

    @staticmethod
    cdef BarType from_str_c(str value)

    cpdef bint is_externally_aggregated(self)
    cpdef bint is_internally_aggregated(self)


cdef class Bar(Data):
    cdef Bar_t _mem

    cdef readonly bint is_revision
    """If this bar is a revision for a previous bar with the same `ts_event`.\n\n:returns: `bool`"""

    cdef str to_str(self)

    @staticmethod
    cdef Bar from_mem_c(Bar_t mem)

    @staticmethod
    cdef Bar from_pyo3_c(pyo3_bar)

    @staticmethod
    cdef Bar from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(Bar obj)

    cpdef bint is_single_price(self)


cdef class BookOrder:
    cdef BookOrder_t _mem

    cpdef double exposure(self)
    cpdef double signed_size(self)

    @staticmethod
    cdef BookOrder from_raw_c(
        OrderSide side,
        int64_t price_raw,
        uint8_t price_prec,
        uint64_t size_raw,
        uint8_t size_prec,
        uint64_t order_id,
    )

    @staticmethod
    cdef BookOrder from_mem_c(BookOrder_t mem)

    @staticmethod
    cdef BookOrder from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(BookOrder obj)


cdef class OrderBookDelta(Data):
    cdef OrderBookDelta_t _mem

    @staticmethod
    cdef OrderBookDelta from_raw_c(
        InstrumentId instrument_id,
        BookAction action,
        OrderSide side,
        int64_t price_raw,
        uint8_t price_prec,
        uint64_t size_raw,
        uint8_t size_prec,
        uint64_t order_id,
        uint8_t flags,
        uint64_t sequence,
        uint64_t ts_event,
        uint64_t ts_init,
    )

    @staticmethod
    cdef OrderBookDelta from_mem_c(OrderBookDelta_t mem)

    @staticmethod
    cdef OrderBookDelta from_pyo3_c(pyo3_delta)

    @staticmethod
    cdef OrderBookDelta from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderBookDelta obj)

    @staticmethod
    cdef OrderBookDelta clear_c(
        InstrumentId instrument_id,
        uint64_t ts_event,
        uint64_t ts_init,
        uint64_t sequence=*,
    )

    @staticmethod
    cdef list capsule_to_list_c(capsule)

    @staticmethod
    cdef object list_to_capsule_c(list items)


cdef class OrderBookDeltas(Data):
    cdef OrderBookDeltas_API _mem

    @staticmethod
    cdef OrderBookDeltas from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderBookDeltas obj)

    cpdef to_capsule(self)
    cpdef to_pyo3(self)


cdef class OrderBookDepth10(Data):
    cdef OrderBookDepth10_t _mem

    @staticmethod
    cdef OrderBookDepth10 from_mem_c(OrderBookDepth10_t mem)

    @staticmethod
    cdef OrderBookDepth10 from_pyo3_c(pyo3_depth10)

    @staticmethod
    cdef OrderBookDepth10 from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderBookDepth10 obj)

    @staticmethod
    cdef list capsule_to_list_c(capsule)

    @staticmethod
    cdef object list_to_capsule_c(list items)


cdef class VenueStatus(Data):
    cdef readonly Venue venue
    """The venue.\n\n:returns: `Venue`"""
    cdef readonly MarketStatus status
    """The venue market status.\n\n:returns: `MarketStatus`"""
    cdef readonly uint64_t ts_event
    """The UNIX timestamp (nanoseconds) when the data event occurred.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t ts_init
    """The UNIX timestamp (nanoseconds) when the object was initialized.\n\n:returns: `uint64_t`"""

    @staticmethod
    cdef VenueStatus from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(VenueStatus obj)


cdef class InstrumentStatus(Data):
    cdef readonly InstrumentId instrument_id
    """The instrument ID.\n\n:returns: `InstrumentId`"""
    cdef readonly str trading_session
    """The trading session name.\n\n:returns: `str`"""
    cdef readonly MarketStatus status
    """The instrument market status.\n\n:returns: `MarketStatus`"""
    cdef readonly HaltReason halt_reason
    """The halt reason.\n\n:returns: `HaltReason`"""
    cdef readonly uint64_t ts_event
    """The UNIX timestamp (nanoseconds) when the data event occurred.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t ts_init
    """The UNIX timestamp (nanoseconds) when the object was initialized.\n\n:returns: `uint64_t`"""

    @staticmethod
    cdef InstrumentStatus from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(InstrumentStatus obj)


cdef class InstrumentClose(Data):
    cdef readonly InstrumentId instrument_id
    """The event instrument ID.\n\n:returns: `InstrumentId`"""
    cdef readonly Price close_price
    """The instrument close price.\n\n:returns: `Price`"""
    cdef readonly InstrumentCloseType close_type
    """The instrument close type.\n\n:returns: `InstrumentCloseType`"""
    cdef readonly uint64_t ts_event
    """The UNIX timestamp (nanoseconds) when the data event occurred.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t ts_init
    """The UNIX timestamp (nanoseconds) when the object was initialized.\n\n:returns: `uint64_t`"""

    @staticmethod
    cdef InstrumentClose from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(InstrumentClose obj)


cdef class QuoteTick(Data):
    cdef QuoteTick_t _mem

    cdef str to_str(self)

    @staticmethod
    cdef QuoteTick from_raw_c(
        InstrumentId instrument_id,
        int64_t bid_price_raw,
        int64_t ask_price_raw,
        uint8_t bid_price_prec,
        uint8_t ask_price_prec,
        uint64_t bid_size_raw,
        uint64_t ask_size_raw,
        uint8_t bid_size_prec,
        uint8_t ask_size_prec,
        uint64_t ts_event,
        uint64_t ts_init,
    )

    @staticmethod
    cdef QuoteTick from_mem_c(QuoteTick_t mem)

    @staticmethod
    cdef QuoteTick from_pyo3_c(pyo3_quote)

    @staticmethod
    cdef list capsule_to_list_c(capsule)

    @staticmethod
    cdef object list_to_capsule_c(list items)

    @staticmethod
    cdef QuoteTick from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(QuoteTick obj)

    cpdef Price extract_price(self, PriceType price_type)
    cpdef Quantity extract_volume(self, PriceType price_type)


cdef class TradeTick(Data):
    cdef TradeTick_t _mem

    cdef str to_str(self)

    @staticmethod
    cdef TradeTick from_raw_c(
        InstrumentId instrument_id,
        int64_t price_raw,
        uint8_t price_prec,
        uint64_t size_raw,
        uint8_t size_prec,
        AggressorSide aggressor_side,
        TradeId trade_id,
        uint64_t ts_event,
        uint64_t ts_init,
    )

    @staticmethod
    cdef TradeTick from_mem_c(TradeTick_t mem)

    @staticmethod
    cdef TradeTick from_pyo3_c(pyo3_trade)

    @staticmethod
    cdef list capsule_to_list_c(capsule)

    @staticmethod
    cdef object list_to_capsule_c(list items)

    @staticmethod
    cdef TradeTick from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(TradeTick obj)

    @staticmethod
    cdef TradeTick from_mem_c(TradeTick_t mem)
