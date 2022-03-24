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
cimport numpy
import talib
from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.indicators.base.indicator cimport Indicator


cdef class TaLib(Indicator):
    cdef str indicator_type
    cdef list price_types
    cdef dict indicator_params
    cdef talib._ta_lib.Function indicator_function


    cdef object value
    """The current value.\n\n:returns: `double`"""

    cpdef void update_raw(self, double high, double low, double close)
    cdef void _check_initialized(self) except *
    cdef dict _unpack_params(self, double high, double low, double close)