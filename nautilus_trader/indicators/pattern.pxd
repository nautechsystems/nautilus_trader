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

from nautilus_trader.indicators.base.indicator cimport Indicator


cdef class Pattern(Indicator):
    cdef object _open_inputs
    cdef object _high_inputs
    cdef object _low_inputs
    cdef object _close_inputs

    cdef readonly int period
    """The window period.\n\n:returns: `int`"""
    cdef readonly list pattern_names
    """The input pattern_names.\n\n:returns: `list`"""
    cdef readonly list value
    """The current value.\n\n:returns: `list`"""

    cpdef void update_raw(
        self,
        double open,
        double high,
        double low,
        double close,
    ) except *
