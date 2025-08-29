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

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.component cimport TimeEvent
from nautilus_trader.model.greeks cimport GreeksCalculator
from nautilus_trader.model.identifiers cimport InstrumentId


cdef class SpreadQuoteAggregator(Component):
    cdef readonly InstrumentId _spread_instrument_id
    cdef readonly object _handler
    cdef readonly CacheFacade _cache
    cdef readonly list _components
    cdef readonly GreeksCalculator _greeks_calculator
    cdef readonly double _vega_multiplier
    cdef readonly int _update_interval_seconds
    cdef readonly str _timer_name
    cdef readonly list _component_ids
    cdef readonly object _ratios
    cdef readonly object _mid_prices
    cdef readonly object _vegas
    cdef readonly object _deltas
    cdef readonly object _bid_ask_spreads
    cdef readonly object _bid_sizes
    cdef readonly object _ask_sizes

    cdef void _set_build_timer(self)
    cdef void _build_quote(self, TimeEvent event)
