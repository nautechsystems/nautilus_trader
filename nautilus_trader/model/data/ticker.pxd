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

from nautilus_trader.model.data.base cimport Data
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class Ticker(Data):
    cdef readonly InstrumentId instrument_id
    """The ticker instrument ID.\n\n:returns: `InstrumentId`"""
    cdef readonly Price open
    """The open price for the previous 24hr period.\n\n:returns: `Price`"""
    cdef readonly Price high
    """The high price for the previous 24hr period.\n\n:returns: `Price`"""
    cdef readonly Price low
    """The low price for the previous 24hr period.\n\n:returns: `Price`"""
    cdef readonly Price close
    """The close price for the previous 24hr period.\n\n:returns: `Price`"""
    cdef readonly Quantity volume_quote
    """The traded quote asset volume for the previous 24hr period.\n\n:returns: `Quantity`"""
    cdef readonly Quantity volume_base
    """The traded base asset volume for the previous 24hr period.\n\n:returns: `Quantity` or None"""
    cdef readonly Price bid
    """The top of book bid price.\n\n:returns: `Price`"""
    cdef readonly Price ask
    """The top of book ask price.\n\n:returns: `Price`"""
    cdef readonly Quantity bid_size
    """The top of book bid size.\n\n:returns: `Quantity`"""
    cdef readonly Quantity ask_size
    """The top of book ask size.\n\n:returns: `Quantity`"""
    cdef readonly Price last_px
    """The last traded price.\n\n:returns: `Price`"""
    cdef readonly Quantity last_qty
    """The last traded quantity.\n\n:returns: `Quantity`"""
    cdef readonly dict info
    """The additional ticker information.\n\n:returns: `dict[str, object]`"""

    @staticmethod
    cdef Ticker from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(Ticker obj)
