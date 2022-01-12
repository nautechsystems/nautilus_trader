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

from nautilus_trader.core.data cimport Data
from nautilus_trader.model.c_enums.aggressor_side cimport AggressorSide
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class Tick(Data):
    cdef readonly InstrumentId instrument_id
    """The tick instrument ID.\n\n:returns: `InstrumentId`"""


cdef class QuoteTick(Tick):
    cdef readonly Price bid
    """The top of book bid price.\n\n:returns: `Price`"""
    cdef readonly Price ask
    """The top of book ask price.\n\n:returns: `Price`"""
    cdef readonly Quantity bid_size
    """The top of book bid size.\n\n:returns: `Quantity`"""
    cdef readonly Quantity ask_size
    """The top of book ask size.\n\n:returns: `Quantity`"""

    @staticmethod
    cdef QuoteTick from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(QuoteTick obj)
    cpdef Price extract_price(self, PriceType price_type)
    cpdef Quantity extract_volume(self, PriceType price_type)


cdef class TradeTick(Tick):
    cdef readonly Price price
    """The traded price.\n\n:returns: `Price`"""
    cdef readonly Quantity size
    """The traded size.\n\n:returns: `Quantity`"""
    cdef readonly AggressorSide aggressor_side
    """The trade aggressor side.\n\n:returns: `AggressorSide`"""
    cdef readonly str trade_id
    """The trade match ID.\n\n:returns: `str`"""

    @staticmethod
    cdef TradeTick from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(TradeTick obj)
