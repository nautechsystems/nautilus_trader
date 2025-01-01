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

from nautilus_trader.indicators.base.indicator cimport Indicator


cdef class BollingerBands(Indicator):
    cdef object _ma
    cdef object _prices

    cdef readonly int period
    """The period for the moving average.\n\n:returns: `int`"""
    cdef readonly double k
    """The standard deviation multiple.\n\n:returns: `double`"""
    cdef readonly double upper
    """The current value of the upper band.\n\n:returns: `double`"""
    cdef readonly double middle
    """The current value of the middle band.\n\n:returns: `double`"""
    cdef readonly double lower
    """The current value of the lower band.\n\n:returns: `double`"""

    cpdef void update_raw(self, double high, double low, double close)
