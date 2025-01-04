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

from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.indicators.base.indicator cimport Indicator


cdef class ArcherMovingAveragesTrends(Indicator):
    cdef MovingAverage _fast_ma
    cdef MovingAverage _slow_ma
    cdef object _fast_ma_price
    cdef object _slow_ma_price

    cdef readonly int fast_period
    """The fast moving average window period.\n\n:returns: `int`"""
    cdef readonly int slow_period
    """The slow moving average window period.\n\n:returns: `int`"""
    cdef readonly int signal_period
    """The period for lookback price array.\n\n:returns: `int`"""
    cdef readonly int long_run
    """The current long run value.\n\n:returns: `int`"""
    cdef readonly int short_run
    """The current short run value.\n\n:returns: `int`"""

    cpdef void update_raw(self, double close)
