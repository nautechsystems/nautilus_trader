# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from libc.stdint cimport uint64_t

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Logger
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Price


cdef class GreeksCalculator:
    cdef Clock _clock
    cdef Logger _log
    cdef CacheFacade _cache
    cdef dict _cached_futures_spreads
    cdef Price _get_underlying_price(self, InstrumentId underlying_instrument_id)
    cdef object _calculate_non_option_greeks(self, object instrument, InstrumentId instrument_id, double spot_shock, uint64_t ts_event, object position, bint percent_greeks, object index_instrument_id, object beta_weights)
    cdef object _calculate_option_greeks(self, object instrument, InstrumentId instrument_id, InstrumentId underlying_instrument_id, double flat_interest_rate, object flat_dividend_yield, bint use_cached_greeks, bint update_vol, bint cache_greeks, uint64_t ts_event, bint percent_greeks, object index_instrument_id, object beta_weights, object vega_time_weight_base)
    cdef object _apply_option_greeks_shocks(self, object greeks_data, InstrumentId underlying_instrument_id, double spot_shock, double vol_shock, double time_to_expiry_shock, bint percent_greeks, object index_instrument_id, object beta_weights, object vega_time_weight_base)
    cpdef object get_cached_futures_spread_price(self, InstrumentId underlying_instrument_id)
    cdef double _calculate_implied_future_price(self, object call_instrument, Price call_price, Price put_price)
    cdef Price _get_price(self, InstrumentId instrument_id)
    cpdef object cache_futures_spread(self, InstrumentId call_instrument_id, InstrumentId put_instrument_id, InstrumentId futures_instrument_id)
