# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.message cimport Event
from nautilus_trader.model.enums_c cimport TradingState
from nautilus_trader.model.identifiers cimport TraderId


cdef class RiskEvent(Event):
    cdef readonly TraderId trader_id
    """The trader ID associated with the event.\n\n:returns: `TraderId`"""


cdef class TradingStateChanged(RiskEvent):
    cdef readonly TradingState state
    """The trading state for the event.\n\n:returns: `TradingState`"""
    cdef readonly dict config
    """The risk engine configuration.\n\n:returns: `dict[str, Any]`"""

    @staticmethod
    cdef TradingStateChanged from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(TradingStateChanged obj)
