# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
from libc.stdint cimport int64_t

from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.model.data.base cimport Data
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick


cdef class DataProducerFacade:
    cdef readonly list execution_resolutions
    cdef readonly datetime min_timestamp
    cdef readonly datetime max_timestamp
    cdef readonly int64_t min_timestamp_ns
    cdef readonly int64_t max_timestamp_ns
    cdef readonly bint has_data

    cpdef list instruments(self)
    cpdef void reset(self) except *
    cpdef Data next(self)


cdef class BacktestDataProducer(DataProducerFacade):
    cdef LoggerAdapter _log

    cdef list _instruments
    cdef object _quote_tick_data
    cdef object _trade_tick_data
    cdef dict _instrument_index
    cdef bint _is_connected

    cdef list _stream
    cdef int _stream_index
    cdef int _stream_index_last
    cdef Data _next_data

    cdef unsigned short[:] _quote_instruments
    cdef str[:] _quote_bids
    cdef str[:] _quote_asks
    cdef str[:] _quote_bid_sizes
    cdef str[:] _quote_ask_sizes
    cdef int64_t[:] _quote_timestamps
    cdef int _quote_index
    cdef int _quote_index_last
    cdef QuoteTick _next_quote_tick

    cdef unsigned short[:] _trade_instruments
    cdef str[:] _trade_prices
    cdef str[:] _trade_sizes
    cdef str[:] _trade_match_ids
    cdef str[:] _trade_sides
    cdef int64_t[:] _trade_timestamps
    cdef int _trade_index
    cdef int _trade_index_last
    cdef TradeTick _next_trade_tick

    cpdef LoggerAdapter get_logger(self)
    cpdef void reset(self) except *
    cpdef void clear(self) except *
    cpdef Data next(self)

    cdef void _iterate_stream(self) except *
    cdef void _iterate_quote_ticks(self) except *
    cdef void _iterate_trade_ticks(self) except *
    cdef QuoteTick _generate_quote_tick(self, int index)
    cdef TradeTick _generate_trade_tick(self, int index)


cdef class CachedProducer(DataProducerFacade):
    cdef BacktestDataProducer _producer
    cdef LoggerAdapter _log
    cdef list _timestamp_cache
    cdef list _data_cache
    cdef int _data_index
    cdef int _data_index_last
    cdef int _init_start_data_index
    cdef int _init_stop_data_index

    cpdef void reset(self) except *
    cpdef Data next(self)
    cdef void _create_data_cache(self) except *
