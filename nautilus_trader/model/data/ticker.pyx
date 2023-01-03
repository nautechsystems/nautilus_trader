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
from nautilus_trader.model.identifiers cimport InstrumentId


cdef class Ticker(Data):
    """
    The base class for all tickers.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the ticker event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        super().__init__(ts_event, ts_init)

        self.instrument_id = instrument_id

    def __eq__(self, Ticker other) -> bool:
        return self.instrument_id == other.instrument_id

    def __hash__(self) -> int:
        return hash(self.instrument_id)

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}"
            f"(instrument_id={self.instrument_id.to_str()}, "
            f"ts_event={self.ts_event})"
        )

    @staticmethod
    cdef Ticker from_dict_c(dict values):
        Condition.not_none(values, "values")
        return Ticker(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(Ticker obj):
        Condition.not_none(obj, "obj")
        return {
            "type": type(obj).__name__,
            "instrument_id": obj.instrument_id.to_str(),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> Ticker:
        """
        Return a ticker from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        Ticker

        """
        return Ticker.from_dict_c(values)

    @staticmethod
    def to_dict(Ticker obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return Ticker.to_dict_c(obj)
