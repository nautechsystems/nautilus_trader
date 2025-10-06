# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue


cdef class TopicCache:
    cdef dict _topic_cache_instruments
    cdef dict _topic_cache_instruments_pattern
    cdef dict _topic_cache_deltas
    cdef dict _topic_cache_depth
    cdef dict _topic_cache_quotes
    cdef dict _topic_cache_trades
    cdef dict _topic_cache_status
    cdef dict _topic_cache_mark_prices
    cdef dict _topic_cache_index_prices
    cdef dict _topic_cache_funding_rates
    cdef dict _topic_cache_close_prices
    cdef dict _topic_cache_snapshots
    cdef dict _topic_cache_custom
    cdef dict _topic_cache_custom_simple
    cdef dict _topic_cache_bars
    cdef dict _topic_cache_signal

    # Topic generation methods
    cdef str get_instruments_topic(self, InstrumentId instrument_id, bint historical = *)
    cdef str get_instruments_topic_pattern(self, Venue venue)
    cdef str get_book_topic(self, type book_data_type, InstrumentId instrument_id, bint historical = *)
    cdef str get_deltas_topic(self, InstrumentId instrument_id, bint historical = *)
    cdef str get_depth_topic(self, InstrumentId instrument_id, bint historical = *)
    cdef str get_quotes_topic(self, InstrumentId instrument_id, bint historical = *)
    cdef str get_trades_topic(self, InstrumentId instrument_id, bint historical = *)
    cdef str get_status_topic(self, InstrumentId instrument_id, bint historical = *)
    cdef str get_mark_prices_topic(self, InstrumentId instrument_id, bint historical = *)
    cdef str get_index_prices_topic(self, InstrumentId instrument_id, bint historical = *)
    cdef str get_funding_rates_topic(self, InstrumentId instrument_id, bint historical = *)
    cdef str get_close_prices_topic(self, InstrumentId instrument_id, bint historical = *)
    cdef str get_snapshots_topic(self, InstrumentId instrument_id, int interval_ms, bint historical = *)
    cdef str get_custom_data_topic(self, DataType data_type, InstrumentId instrument_id = *, bint historical = *)
    cdef str get_bars_topic(self, BarType bar_type, bint historical = *)
    cdef str get_signal_topic(self, str name)

    # Cache management methods
    cpdef void clear_cache(self)
