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

from libc.stdint cimport uint64_t

from nautilus_trader.core.data cimport Data
from nautilus_trader.model.enums_c cimport InstrumentCloseType
from nautilus_trader.model.enums_c cimport MarketStatus
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Price


cdef class VenueStatus(Data):
    cdef readonly Venue venue
    """The venue.\n\n:returns: `Venue`"""
    cdef readonly MarketStatus status
    """The venue market status.\n\n:returns: `MarketStatus`"""
    cdef readonly uint64_t ts_event
    """The UNIX timestamp (nanoseconds) when the data event occurred.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t ts_init
    """The UNIX timestamp (nanoseconds) when the object was initialized.\n\n:returns: `uint64_t`"""

    @staticmethod
    cdef VenueStatus from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(VenueStatus obj)


cdef class InstrumentStatus(Data):
    cdef readonly InstrumentId instrument_id
    """The instrument ID.\n\n:returns: `InstrumentId`"""
    cdef readonly MarketStatus status
    """The instrument market status.\n\n:returns: `MarketStatus`"""
    cdef readonly uint64_t ts_event
    """The UNIX timestamp (nanoseconds) when the data event occurred.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t ts_init
    """The UNIX timestamp (nanoseconds) when the object was initialized.\n\n:returns: `uint64_t`"""

    @staticmethod
    cdef InstrumentStatus from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(InstrumentStatus obj)


cdef class InstrumentClose(Data):
    cdef readonly InstrumentId instrument_id
    """The event instrument ID.\n\n:returns: `InstrumentId`"""
    cdef readonly Price close_price
    """The instrument close price.\n\n:returns: `Price`"""
    cdef readonly InstrumentCloseType close_type
    """The instrument close type.\n\n:returns: `InstrumentCloseType`"""
    cdef readonly uint64_t ts_event
    """The UNIX timestamp (nanoseconds) when the data event occurred.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t ts_init
    """The UNIX timestamp (nanoseconds) when the object was initialized.\n\n:returns: `uint64_t`"""

    @staticmethod
    cdef InstrumentClose from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(InstrumentClose obj)
