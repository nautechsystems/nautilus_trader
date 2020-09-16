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
from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.indicators.efficiency_ratio cimport EfficiencyRatio


cdef class AdaptiveMovingAverage(MovingAverage):
    cdef int _period_er
    cdef int _period_alpha_fast
    cdef int _period_alpha_slow
    cdef double _alpha_fast
    cdef double _alpha_slow
    cdef double _alpha_diff
    cdef EfficiencyRatio _efficiency_ratio
    cdef double _prior_value

    cpdef void update(self, Bar bar) except *
    cpdef void update_raw(self, double value) except *
    cpdef void reset(self) except *
