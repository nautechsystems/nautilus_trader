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
from nautilus_trader.model.enums_c cimport HaltReason
from nautilus_trader.model.enums_c cimport InstrumentCloseType
from nautilus_trader.model.enums_c cimport MarketStatus
from nautilus_trader.model.enums_c cimport halt_reason_from_str
from nautilus_trader.model.enums_c cimport halt_reason_to_str
from nautilus_trader.model.enums_c cimport instrument_close_type_from_str
from nautilus_trader.model.enums_c cimport instrument_close_type_to_str
from nautilus_trader.model.enums_c cimport market_status_from_str
from nautilus_trader.model.enums_c cimport market_status_to_str
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Price


cdef class VenueStatus(Data):
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
        self.venue = venue
        self.status = status
        self.ts_event = ts_event
        self.ts_init = ts_init

    def __eq__(self, VenueStatus other) -> bool:
        return VenueStatus.to_dict_c(self) == VenueStatus.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(VenueStatus.to_dict_c(self)))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"venue={self.venue}, "
            f"status={market_status_to_str(self.status)})"
        )

    @staticmethod
    cdef VenueStatus from_dict_c(dict values):
        Condition.not_none(values, "values")
        return VenueStatus(
            venue=Venue(values["venue"]),
            status=market_status_from_str(values["status"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(VenueStatus obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "VenueStatus",
            "venue": obj.venue.to_str(),
            "status": market_status_to_str(obj.status),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> VenueStatus:
        """
        Return a venue status update from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        VenueStatus

        """
        return VenueStatus.from_dict_c(values)

    @staticmethod
    def to_dict(VenueStatus obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return VenueStatus.to_dict_c(obj)


cdef class InstrumentStatus(Data):
    """
    Represents an event that indicates a change in an instrument market status.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID.
    status : MarketStatus
        The instrument market session status.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the status update event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    trading_session : str, default 'Regular'
        The name of the trading session.
    halt_reason : HaltReason, default ``NOT_HALTED``
        The halt reason (only applicable for ``HALT`` status).

    Raises
    ------
    ValueError
        If `status` is not equal to ``HALT`` and `halt_reason` is other than ``NOT_HALTED``.

    """

    def __init__(
        self,
        InstrumentId instrument_id,
        MarketStatus status,
        uint64_t ts_event,
        uint64_t ts_init,
        str trading_session = "Regular",
        HaltReason halt_reason = HaltReason.NOT_HALTED,
    ):
        if status != MarketStatus.HALT:
            Condition.equal(halt_reason, HaltReason.NOT_HALTED, "halt_reason", "NO_HALT")

        self.instrument_id = instrument_id
        self.trading_session = trading_session
        self.status = status
        self.halt_reason = halt_reason
        self.ts_event = ts_event
        self.ts_init = ts_init

    def __eq__(self, InstrumentStatus other) -> bool:
        return InstrumentStatus.to_dict_c(self) == InstrumentStatus.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(InstrumentStatus.to_dict_c(self)))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"trading_session={self.trading_session}, "
            f"status={market_status_to_str(self.status)}, "
            f"halt_reason={halt_reason_to_str(self.halt_reason)}, "
            f"ts_event={self.ts_event})"
        )

    @staticmethod
    cdef InstrumentStatus from_dict_c(dict values):
        Condition.not_none(values, "values")
        return InstrumentStatus(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            trading_session=values.get("trading_session", "Regular"),
            status=market_status_from_str(values["status"]),
            halt_reason=halt_reason_from_str(values.get("halt_reason", "NOT_HALTED")),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(InstrumentStatus obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "InstrumentStatus",
            "instrument_id": obj.instrument_id.to_str(),
            "trading_session": obj.trading_session,
            "status": market_status_to_str(obj.status),
            "halt_reason": halt_reason_to_str(obj.halt_reason),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> InstrumentStatus:
        """
        Return an instrument status update from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        InstrumentStatus

        """
        return InstrumentStatus.from_dict_c(values)

    @staticmethod
    def to_dict(InstrumentStatus obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return InstrumentStatus.to_dict_c(obj)


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
        self.instrument_id = instrument_id
        self.close_price = close_price
        self.close_type = close_type
        self.ts_event = ts_event
        self.ts_init = ts_init

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
