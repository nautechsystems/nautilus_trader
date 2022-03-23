# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from numpy import ndarray
from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.indicators.base.indicator cimport Indicator
from talib import abstract

cdef class ta_lib(Indicator):
    cdef abstract indicator_type
    cdef list price_types
    cdef dict params


    cdef ndarray value
    """The current value.\n\n:returns: `double`"""

    cpdef void update_raw(self, double high, double low, double close)
    cdef void _check_initialized(self) except *
     cdef list _unpack_params(self, double high, double low, double close)