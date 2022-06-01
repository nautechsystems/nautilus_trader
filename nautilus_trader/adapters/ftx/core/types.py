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

from typing import Any, Dict

from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class FTXTicker(Ticker):
    """
    Represents an `FTX` 24hr market ticker.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID.
    bid : Price
        The top of book bid price.
    ask : Price
        The top of book ask price.
    bid_size : Quantity
        The top of book bid size.
    ask_size : Quantity
        The top of book ask size.
    last : Price
        The last price.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the ticker event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.

    References
    ----------
    https://docs.ftx.com/#ticker
    """

    def __init__(
        self,
        instrument_id: InstrumentId,
        bid: Price,
        ask: Price,
        bid_size: Quantity,
        ask_size: Quantity,
        last: Price,
        ts_event: int,
        ts_init: int,
    ):
        super().__init__(
            instrument_id=instrument_id,
            ts_event=ts_event,
            ts_init=ts_init,
        )

        self.bid = bid
        self.ask = ask
        self.bid_size = bid_size
        self.ask_size = ask_size
        self.last = last

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.value}, "
            f"bid={self.bid}, "
            f"ask={self.ask}, "
            f"bid_size={self.bid_size}, "
            f"ask_size={self.ask_size}, "
            f"last={self.last}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    def from_dict(values: Dict[str, Any]) -> "FTXTicker":
        """
        Return an `FTX` ticker parsed from the given values.

        Parameters
        ----------
        values : dict[str, Any]
            The values for initialization.

        Returns
        -------
        FTXTicker

        """
        return FTXTicker(
            instrument_id=InstrumentId.from_str(values["instrument_id"]),
            bid=Price.from_str(values["bid"]),
            ask=Price.from_str(values["ask"]),
            bid_size=Quantity.from_str(values["bid_size"]),
            ask_size=Quantity.from_str(values["ask_size"]),
            last=Price.from_str(values["last"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    def to_dict(obj: "FTXTicker") -> Dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, Any]

        """
        return {
            "type": type(obj).__name__,
            "instrument_id": obj.instrument_id.value,
            "bid": str(obj.bid),
            "ask": str(obj.ask),
            "bid_size": str(obj.bid_size),
            "ask_size": str(obj.ask_size),
            "last": str(obj.last),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }
