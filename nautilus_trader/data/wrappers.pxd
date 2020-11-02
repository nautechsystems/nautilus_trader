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
from nautilus_trader.model.bar cimport BarType


cdef class BarData:
    cdef readonly BarType bar_type
    """The type of the bar data.\n\n:returns: `BarType`"""
    cdef readonly Bar bar
    """The bar data.\n\n:returns: `Bar`"""


cdef class BarDataBlock:
    cdef readonly BarType bar_type
    """The type of the bar data.\n\n:returns: `BarType`"""
    cdef readonly list bars
    """The bars data.\n\n:returns: `list[BarType]`"""


cdef class QuoteTickDataBlock:
    cdef readonly list ticks
    """The ticks data.\n\n:returns: `list[QuoteTick]`"""


cdef class TradeTickDataBlock:
    cdef readonly list ticks
    """The ticks data.\n\n:returns: `list[TradeTick]`"""


cdef class InstrumentDataBlock:
    cdef readonly list instruments
    """The instruments data.\n\n:returns: `list[Instrument]`"""
