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


cdef class AroonOscillator(Indicator):
    cdef object _high_inputs
    cdef object _low_inputs

    cdef readonly int period
    """The window period.\n\n:returns: `int`"""
    cdef readonly double aroon_up
    """The current aroon up value.\n\n:returns: `double`"""
    cdef readonly double aroon_down
    """The current aroon down value.\n\n:returns: `double`"""
    cdef readonly double value
    """The current value.\n\n:returns: `double`"""

    cpdef void update_raw(self, double high, double low)
    cdef void _check_initialized(self)
