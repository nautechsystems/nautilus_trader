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

from nautilus_trader.common.c_enums.component_state cimport ComponentState
from nautilus_trader.core.message cimport Event
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport TraderId


cdef class ComponentStateChanged(Event):
    cdef readonly TraderId trader_id
    """The trader ID associated with the event.\n\n:returns: `TraderId`"""
    cdef readonly ComponentId component_id
    """The component ID associated with the event.\n\n:returns: `ComponentId`"""
    cdef readonly str component_type
    """The component type associated with the event.\n\n:returns: `str`"""
    cdef readonly ComponentState state
    """The component state.\n\n:returns: `ComponentState`"""
    cdef readonly dict config
    """The component configuration.\n\n:returns: `dict[str, Any]`"""

    @staticmethod
    cdef ComponentStateChanged from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(ComponentStateChanged obj)
