# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.indicators.atr cimport AverageTrueRange


cdef class Pressure(Indicator):
    cdef AverageTrueRange _atr
    cdef MovingAverage _average_volume

    cdef readonly int period
    cdef readonly double value
    cdef readonly double value_cumulative

    cpdef void update(self, double high, double low, double close, double volume)
    cpdef void reset(self)
