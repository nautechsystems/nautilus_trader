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

import orjson
from libc.stdint cimport int64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.data.base cimport Data
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class Ticker(Data):
    """
    The base class for all tickers.

    Represents a market ticker for the previous 24hr period.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        int64_t ts_event,
        int64_t ts_init,
        Price open=None,  # noqa (shadows built-in name open)
        Price high=None,
        Price low=None,
        Price close=None,
        Quantity volume_quote=None,
        Quantity volume_base=None,  # Can be None
        Price bid=None,
        Price ask=None,
        Quantity bid_size=None,
        Quantity ask_size=None,
        Price last_px=None,
        Quantity last_qty=None,
        dict info=None,
    ):
        """
        Initialize a new instance of the ``Ticker`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID.
        open : Price
            The open price for the previous 24hr period.
        high : Price
            The high price for the previous 24hr period.
        low : Price
            The low price for the previous 24hr period.
        close : Price
            The close price for the previous 24hr period.
        volume_quote : Quantity
            The traded quote asset volume for the previous 24hr period.
        volume_base : Quantity
            The traded base asset volume for the previous 24hr period.
        bid : Price
            The top of book bid price.
        ask : Price
            The top of book ask price.
        bid_size : Quantity
            The top of book bid size.
        ask_size : Quantity
            The top of book ask size.
        last_px : Price
            The last traded price.
        last_qty : Quantity
            The last traded quantity.
        ts_event : int64
            The UNIX timestamp (nanoseconds) when the ticker event occurred.
        ts_init : int64
            The UNIX timestamp (nanoseconds) when the data object was initialized.
        info : dict[str, object]
            The additional ticker information.

        """
        super().__init__(ts_event, ts_init)

        self.instrument_id = instrument_id
        self.open = open
        self.high = high
        self.low = low
        self.close = close
        self.volume_quote = volume_quote
        self.volume_base = volume_base
        self.bid = bid
        self.ask = ask
        self.bid_size = bid_size
        self.ask_size = ask_size
        self.last_px = last_px
        self.last_qty = last_qty
        self.info = info

    def __eq__(self, Ticker other) -> bool:
        return self.instrument_id.value == other.instrument_id.value

    def __hash__(self) -> int:
        return hash(self.instrument_id.value)

    def __repr__(self) -> str:
        return (f"{type(self).__name__}"
                f"(instrument_id={self.instrument_id.value}, "
                f"open={self.open}, "
                f"high={self.high}, "
                f"low={self.low}, "
                f"close={self.close}, "
                f"volume_quote={self.volume_quote}, "
                f"volume_base={self.volume_base}, "
                f"bid={self.bid}, "
                f"ask={self.ask}, "
                f"bid_size={self.bid_size}, "
                f"ask_size={self.ask_size}, "
                f"last_px={self.last_px}, "
                f"last_qty={self.last_qty}, "
                f"ts_event={self.ts_event}, "
                f"info={self.info})")

    @staticmethod
    cdef Ticker from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str vol_b = values["volume_base"]
        cdef bytes info = values["info"]
        return Ticker(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            open=Price.from_str_c(values["open"]),
            high=Price.from_str_c(values["high"]),
            low=Price.from_str_c(values["low"]),
            close=Price.from_str_c(values["close"]),
            volume_quote=Quantity.from_str_c(values["volume_quote"]),
            volume_base=Quantity.from_str_c(vol_b) if vol_b is not None else None,
            bid=Price.from_str_c(values["bid"]),
            ask=Price.from_str_c(values["ask"]),
            bid_size=Quantity.from_str_c(values["bid_size"]),
            ask_size=Quantity.from_str_c(values["ask_size"]),
            last_px=Price.from_str_c(values["last_px"]),
            last_qty=Quantity.from_str_c(values["last_qty"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            info=orjson.loads(info) if info is not None else None,
        )

    @staticmethod
    cdef dict to_dict_c(Ticker obj):
        Condition.not_none(obj, "obj")
        return {
            "type": type(obj).__name__,
            "instrument_id": obj.instrument_id.value,
            "open": str(obj.open),
            "high": str(obj.high),
            "low": str(obj.low),
            "close": str(obj.close),
            "volume_quote": str(obj.volume_quote),
            "volume_base": str(obj.volume_base),
            "bid": str(obj.bid),
            "ask": str(obj.ask),
            "bid_size": str(obj.bid_size),
            "ask_size": str(obj.ask_size),
            "last_px": str(obj.last_px),
            "last_qty": str(obj.last_qty),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "info": orjson.dumps(obj.info) if obj.info is not None else None,
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
