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
from nautilus_trader.core.rust.enums cimport InstrumentCloseType
from nautilus_trader.core.rust.enums cimport MarketStatus
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Price


cdef class StatusUpdate(Data):
    pass


cdef class VenueStatusUpdate(StatusUpdate):
    cdef readonly Venue venue
    """The event venue.\n\n:returns: `Venue`"""
    cdef readonly MarketStatus status
    """The events venue status.\n\n:returns: `MarketStatus`"""

    @staticmethod
    cdef VenueStatusUpdate from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(VenueStatusUpdate obj)


cdef class InstrumentStatusUpdate(StatusUpdate):
    cdef readonly InstrumentId instrument_id
    """The event instrument ID.\n\n:returns: `InstrumentId`"""
    cdef readonly MarketStatus status
    """The events instrument status.\n\n:returns: `MarketStatus`"""

    @staticmethod
    cdef InstrumentStatusUpdate from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(InstrumentStatusUpdate obj)


cdef class InstrumentClosePrice(Data):
    cdef readonly InstrumentId instrument_id
    """The event instrument ID.\n\n:returns: `InstrumentId`"""
    cdef readonly Price close_price
    """The events close price.\n\n:returns: `Price`"""
    cdef readonly InstrumentCloseType close_type
    """The events close type.\n\n:returns: `InstrumentCloseType`"""

    @staticmethod
    cdef InstrumentClosePrice from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(InstrumentClosePrice obj)
