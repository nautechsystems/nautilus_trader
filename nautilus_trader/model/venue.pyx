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

from libc.stdint cimport int64_t

from nautilus_trader.model.c_enums.instrument_close_type cimport InstrumentCloseType
from nautilus_trader.model.c_enums.instrument_close_type cimport InstrumentCloseTypeParser
from nautilus_trader.model.c_enums.instrument_status cimport InstrumentStatus
from nautilus_trader.model.c_enums.instrument_status cimport InstrumentStatusParser
from nautilus_trader.model.c_enums.venue_status cimport VenueStatus
from nautilus_trader.model.c_enums.venue_status cimport VenueStatusParser
from nautilus_trader.model.data cimport Data
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Price


cdef class StatusUpdate(Data):
    """
    The abstract base class for all status updates.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
    ):
        """
        Initialize a new instance of the ``StatusUpdate`` base class.

        Parameters
        ----------
        ts_event_ns : int64
            The UNIX timestamp (nanoseconds) when data event occurred.
        ts_recv_ns : int64
            The UNIX timestamp (nanoseconds) when received by the Nautilus system.

        """
        super().__init__(ts_event_ns, ts_recv_ns)


cdef class VenueStatusUpdate(StatusUpdate):
    """
    Represents an update that indicates a change in a Venue status.
    """

    def __init__(
        self,
        Venue venue,
        VenueStatus status,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
    ):
        """
        Initialize a new instance of the ``VenueStatusUpdate`` class.

        Parameters
        ----------
        status : VenueStatus
            The venue status.
        ts_event_ns : int64
            The UNIX timestamp (nanoseconds) when data event occurred.
        ts_recv_ns : int64
            The UNIX timestamp (nanoseconds) when received by the Nautilus system.

        """
        super().__init__(ts_event_ns, ts_recv_ns)
        self.venue = venue
        self.status = status

    def __eq__(self, VenueStatusUpdate other) -> bool:
        return VenueStatusUpdate.to_dict_c(self) == VenueStatusUpdate.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(VenueStatusUpdate.to_dict_c(self)))

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"venue={self.venue}, "
                f"status={VenueStatusParser.to_str(self.status)})")

    @staticmethod
    cdef VenueStatusUpdate from_dict_c(dict values):
        return VenueStatusUpdate(
            venue=Venue(values["venue"]),
            status=VenueStatusParser.from_str(values["status"]),
            ts_event_ns=values["ts_event_ns"],
            ts_recv_ns=values["ts_recv_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(VenueStatusUpdate obj):
        return {
            "type": "VenueStatusUpdate",
            "venue": obj.venue.value,
            "status": VenueStatusParser.to_str(obj.status),
            "ts_event_ns": obj.ts_event_ns,
            "ts_recv_ns": obj.ts_recv_ns,
        }

    @staticmethod
    def from_dict(dict values):
        """
        Return a venue status update from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        VenueStatusUpdate

        """
        return VenueStatusUpdate.from_dict_c(values)

    @staticmethod
    def to_dict(VenueStatusUpdate obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return VenueStatusUpdate.to_dict_c(obj)


cdef class InstrumentStatusUpdate(StatusUpdate):
    """
    Represents an event that indicates a change in an instrument status.
    """

    def __init__(
        self,
        InstrumentId instrument_id,
        InstrumentStatus status,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
    ):
        """
        Initialize a new instance of the ``InstrumentStatusUpdate`` class.

        Parameters
        ----------
        status : InstrumentStatus
            The instrument status.
        ts_event_ns : int64
            The UNIX timestamp (nanoseconds) when data event occurred.
        ts_recv_ns : int64
            The UNIX timestamp (nanoseconds) when received by the Nautilus system.

        """
        super().__init__(ts_event_ns, ts_recv_ns,)
        self.instrument_id = instrument_id
        self.status = status

    def __eq__(self, InstrumentStatusUpdate other) -> bool:
        return InstrumentStatusUpdate.to_dict_c(self) == InstrumentStatusUpdate.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(InstrumentStatusUpdate.to_dict_c(self)))

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"instrument_id={self.instrument_id}, "
                f"status={InstrumentStatusParser.to_str(self.status)})")

    @staticmethod
    cdef InstrumentStatusUpdate from_dict_c(dict values):
        return InstrumentStatusUpdate(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            status=InstrumentStatusParser.from_str(values["status"]),
            ts_event_ns=values["ts_event_ns"],
            ts_recv_ns=values["ts_recv_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(InstrumentStatusUpdate obj):
        return {
            "type": "InstrumentStatusUpdate",
            "instrument_id": obj.instrument_id.value,
            "status": InstrumentStatusParser.to_str(obj.status),
            "ts_event_ns": obj.ts_event_ns,
            "ts_recv_ns": obj.ts_recv_ns,
        }

    @staticmethod
    def from_dict(dict values):
        """
        Return an instrument status event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        InstrumentStatusUpdate

        """
        return InstrumentStatusUpdate.from_dict_c(values)

    @staticmethod
    def to_dict(InstrumentStatusUpdate obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return InstrumentStatusUpdate.to_dict_c(obj)


cdef class InstrumentClosePrice(Data):
    """
    Represents an instruments closing price at a venue.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Price close_price not None,
        InstrumentCloseType close_type,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
    ):
        """
        Initialize a new instance of the ``InstrumentClosePrice`` class.

        Parameters
        ----------
        close_price : Price
            The closing price for the instrument.
        close_type : InstrumentCloseType
            The type of closing price.
        ts_event_ns : int64
            The UNIX timestamp (nanoseconds) when data event occurred.
        ts_recv_ns : int64
            The UNIX timestamp (nanoseconds) when received by the Nautilus system.

        """
        super().__init__(ts_event_ns, ts_recv_ns,)
        self.instrument_id = instrument_id
        self.close_price = close_price
        self.close_type = close_type

    def __eq__(self, InstrumentClosePrice other) -> bool:
        return InstrumentClosePrice.to_dict_c(self) == InstrumentClosePrice.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(InstrumentClosePrice.to_dict_c(self)))

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"instrument_id={self.instrument_id}, "
                f"close_price={self.close_price}, "
                f"close_type={InstrumentCloseTypeParser.to_str(self.close_type)})")

    @staticmethod
    cdef InstrumentClosePrice from_dict_c(dict values):
        return InstrumentClosePrice(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            close_price=Price.from_str_c(values["close_price"]),
            close_type=InstrumentCloseTypeParser.from_str(values["close_type"]),
            ts_event_ns=values["ts_event_ns"],
            ts_recv_ns=values["ts_recv_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(InstrumentClosePrice obj):
        return {
            "type": "InstrumentClosePrice",
            "instrument_id": obj.instrument_id.value,
            "close_price": str(obj.close_price),
            "close_type": InstrumentCloseTypeParser.to_str(obj.close_type),
            "ts_event_ns": obj.ts_event_ns,
            "ts_recv_ns": obj.ts_recv_ns,
        }

    @staticmethod
    def from_dict(dict values):
        """
        Return an instrument close price event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        InstrumentClosePrice

        """
        return InstrumentClosePrice.from_dict_c(values)

    @staticmethod
    def to_dict(InstrumentClosePrice obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return InstrumentClosePrice.to_dict_c(obj)
