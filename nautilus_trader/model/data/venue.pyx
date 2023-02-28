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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.model.enums_c cimport InstrumentCloseType
from nautilus_trader.model.enums_c cimport MarketStatus
from nautilus_trader.model.enums_c cimport instrument_close_type_from_str
from nautilus_trader.model.enums_c cimport instrument_close_type_to_str
from nautilus_trader.model.enums_c cimport market_status_from_str
from nautilus_trader.model.enums_c cimport market_status_to_str
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Price


cdef class StatusUpdate(Data):
    """
    The base class for all status updates.

    Parameters
    ----------
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the status update event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        super().__init__(ts_event, ts_init)


cdef class VenueStatusUpdate(StatusUpdate):
    """
    Represents an update that indicates a change in a Venue status.

    Parameters
    ----------
    venue : Venue
        The venue ID.
    status : MarketStatus
        The venue market status.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the status update event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        Venue venue,
        MarketStatus status,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        super().__init__(ts_event, ts_init)
        self.venue = venue
        self.status = status

    def __eq__(self, VenueStatusUpdate other) -> bool:
        return VenueStatusUpdate.to_dict_c(self) == VenueStatusUpdate.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(VenueStatusUpdate.to_dict_c(self)))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"venue={self.venue}, "
            f"status={market_status_to_str(self.status)})"
        )

    @staticmethod
    cdef VenueStatusUpdate from_dict_c(dict values):
        Condition.not_none(values, "values")
        return VenueStatusUpdate(
            venue=Venue(values["venue"]),
            status=market_status_from_str(values["status"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(VenueStatusUpdate obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "VenueStatusUpdate",
            "venue": obj.venue.to_str(),
            "status": market_status_to_str(obj.status),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> VenueStatusUpdate:
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

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID.
    status : MarketStatus
        The instrument market status.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the status update event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        InstrumentId instrument_id,
        MarketStatus status,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        super().__init__(ts_event, ts_init,)
        self.instrument_id = instrument_id
        self.status = status

    def __eq__(self, InstrumentStatusUpdate other) -> bool:
        return InstrumentStatusUpdate.to_dict_c(self) == InstrumentStatusUpdate.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(InstrumentStatusUpdate.to_dict_c(self)))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"status={market_status_to_str(self.status)})"
        )

    @staticmethod
    cdef InstrumentStatusUpdate from_dict_c(dict values):
        Condition.not_none(values, "values")
        return InstrumentStatusUpdate(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            status=market_status_from_str(values["status"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(InstrumentStatusUpdate obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "InstrumentStatusUpdate",
            "instrument_id": obj.instrument_id.to_str(),
            "status": market_status_to_str(obj.status),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> InstrumentStatusUpdate:
        """
        Return an instrument status update from the given dict values.

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


cdef class InstrumentClose(Data):
    """
    Represents an instrument close at a venue.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID.
    close_price : Price
        The closing price for the instrument.
    close_type : InstrumentCloseType
        The type of closing price.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the close price event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Price close_price not None,
        InstrumentCloseType close_type,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        super().__init__(ts_event, ts_init,)
        self.instrument_id = instrument_id
        self.close_price = close_price
        self.close_type = close_type

    def __eq__(self, InstrumentClose other) -> bool:
        return InstrumentClose.to_dict_c(self) == InstrumentClose.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(InstrumentClose.to_dict_c(self)))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"close_price={self.close_price}, "
            f"close_type={instrument_close_type_to_str(self.close_type)})"
        )

    @staticmethod
    cdef InstrumentClose from_dict_c(dict values):
        Condition.not_none(values, "values")
        return InstrumentClose(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            close_price=Price.from_str_c(values["close_price"]),
            close_type=instrument_close_type_from_str(values["close_type"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(InstrumentClose obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "InstrumentClose",
            "instrument_id": obj.instrument_id.to_str(),
            "close_price": str(obj.close_price),
            "close_type": instrument_close_type_to_str(obj.close_type),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> InstrumentClose:
        """
        Return an instrument close price event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        InstrumentClose

        """
        return InstrumentClose.from_dict_c(values)

    @staticmethod
    def to_dict(InstrumentClose obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return InstrumentClose.to_dict_c(obj)
