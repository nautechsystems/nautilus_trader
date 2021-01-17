# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport TradeMatchId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class Tick:
    cdef readonly Symbol symbol
    """The ticks symbol.\n\n:returns: `Symbol`"""
    cdef readonly datetime timestamp
    """The ticks timestamp.\n\n:returns: `datetime`"""


cdef class QuoteTick(Tick):
    cdef readonly Price bid
    """The ticks best quoted bid price.\n\n:returns: `Price`"""
    cdef readonly Price ask
    """The ticks best quoted ask price.\n\n:returns: `Price`"""
    cdef readonly Quantity bid_size
    """The ticks quoted bid size.\n\n:returns: `Quantity`"""
    cdef readonly Quantity ask_size
    """The ticks quoted ask size.\n\n:returns: `Quantity`"""

    cpdef Price extract_price(self, PriceType price_type)
    cpdef Quantity extract_volume(self, PriceType price_type)

    @staticmethod
    cdef QuoteTick from_serializable_string_c(Symbol symbol, str values)
    cpdef str to_serializable_string(self)


cdef class TradeTick(Tick):
    cdef readonly Price price
    """The ticks traded price.\n\n:returns: `Price`"""
    cdef readonly Quantity size
    """The ticks traded size.\n\n:returns: `Quantity`"""
    cdef readonly OrderSide side
    """ The ticks traded side.\n\n:returns: `OrderSide` (Enum)"""
    cdef readonly TradeMatchId match_id
    """The ticks trade match identifier.\n\n:returns: `TradeMatchId`"""

    @staticmethod
    cdef TradeTick from_serializable_string_c(Symbol symbol, str values)
    cpdef str to_serializable_string(self)
