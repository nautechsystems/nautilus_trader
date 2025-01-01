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

from cpython.datetime cimport datetime

from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data cimport Bar


cdef class Swings(Indicator):
    cdef object _high_inputs
    cdef object _low_inputs

    cdef readonly int period
    """The window period.\n\n:returns: `int`"""
    cdef readonly int direction
    """The current swing direction.\n\n:returns: `int`"""
    cdef readonly bint changed
    """If the swing direction changed at the last bar.\n\n:returns: `bool`"""
    cdef readonly datetime high_datetime
    """The last swing high time.\n\n:returns: `datetime`"""
    cdef readonly datetime low_datetime
    """The last swing low time.\n\n:returns: `datetime`"""
    cdef readonly double high_price
    """The last swing high price.\n\n:returns: `double`"""
    cdef readonly double low_price
    """The last swing low price.\n\n:returns: `double`"""
    cdef readonly double length
    """The length of the current swing.\n\n:returns: `double`"""
    cdef readonly int duration
    """The current swing duration.\n\n:returns: `int`"""
    cdef readonly int since_high
    """The bars since the last swing high.\n\n:returns: `int`"""
    cdef readonly int since_low
    """The bars since the last swing low.\n\n:returns: `int`"""

    cpdef void handle_bar(self, Bar bar)
    cpdef void update_raw(self, double high, double low, datetime timestamp)
