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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.aggressor_side cimport AggressorSide
from nautilus_trader.model.c_enums.aggressor_side cimport AggressorSideParser
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.price_type cimport PriceTypeParser
from nautilus_trader.model.data cimport Data
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport TradeMatchId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class Tick(Data):
    """
    The abstract base class for all ticks.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        int64_t ts_event_ns,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``Tick`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The ticks instrument identifier.
        ts_event_ns : int64
            The UNIX timestamp (nanos) when data event occurred.
        timestamp_ns : int64
            The UNIX timestamp (nanos) when received by the Nautilus system.

        """
        super().__init__(ts_event_ns, timestamp_ns)

        self.instrument_id = instrument_id

    def __eq__(self, Tick other) -> bool:
        return self.instrument_id == other.instrument_id and self.ts_recv_ns == other.ts_recv_ns

    def __ne__(self, Tick other) -> bool:
        return not self == other


cdef class QuoteTick(Tick):
    """
    Represents a single quote tick in a financial market.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Price bid not None,
        Price ask not None,
        Quantity bid_size not None,
        Quantity ask_size not None,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
    ):
        """
        Initialize a new instance of the ``QuoteTick`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier.
        bid : Price
            The best bid price.
        ask : Price
            The best ask price.
        bid_size : Quantity
            The size at the best bid.
        ask_size : Quantity
            The size at the best ask.
        ts_event_ns : int64
            The UNIX timestamp (nanos) when data event occurred.
        ts_recv_ns : int64
            The UNIX timestamp (nanos) when received by the Nautilus system.

        """
        super().__init__(instrument_id, ts_event_ns, ts_recv_ns)

        self.bid = bid
        self.ask = ask
        self.bid_size = bid_size
        self.ask_size = ask_size

    def __str__(self) -> str:
        return (f"{self.instrument_id},"
                f"{self.bid},"
                f"{self.ask},"
                f"{self.bid_size},"
                f"{self.ask_size},"
                f"{self.ts_event_ns}")

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    cpdef Price extract_price(self, PriceType price_type):
        """
        Extract the price for the given price type.

        Parameters
        ----------
        price_type : PriceType
            The price type to extraction.

        Returns
        -------
        Price

        """
        if price_type == PriceType.MID:
            return Price(((self.bid + self.ask) / 2), self.bid.precision + 1)
        elif price_type == PriceType.BID:
            return self.bid
        elif price_type == PriceType.ASK:
            return self.ask
        else:
            raise ValueError(f"Cannot extract with PriceType {PriceTypeParser.to_str(price_type)}")

    cpdef Quantity extract_volume(self, PriceType price_type):
        """
        Extract the volume for the given price type.

        Parameters
        ----------
        price_type : PriceType
            The price type for extraction.

        Returns
        -------
        Quantity

        """
        if price_type == PriceType.MID:
            return Quantity((self.bid_size + self.ask_size) / 2, self.bid_size.precision + 1)
        elif price_type == PriceType.BID:
            return self.bid_size
        elif price_type == PriceType.ASK:
            return self.ask_size
        else:
            raise ValueError(f"Cannot extract with PriceType {PriceTypeParser.to_str(price_type)}")

    @staticmethod
    cdef QuoteTick from_serializable_str_c(InstrumentId instrument_id, str values):
        Condition.not_none(instrument_id, "instrument_id")
        Condition.valid_string(values, "values")

        cdef list pieces = values.split(',', maxsplit=5)

        if len(pieces) != 6:
            raise ValueError(f"The QuoteTick string value was malformed, was {values}")

        return QuoteTick(
            instrument_id,
            Price.from_str_c(pieces[0]),
            Price.from_str_c(pieces[1]),
            Quantity.from_str_c(pieces[2]),
            Quantity.from_str_c(pieces[3]),
            int(pieces[4]),
            int(pieces[5]),
        )

    @staticmethod
    def from_serializable_str(InstrumentId instrument_id, str values):
        """
        Parse a tick from the given instrument identifier and values string.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument_id.
        values : str
            The tick values string.

        Returns
        -------
        Tick

        Raises
        ------
        ValueError
            If values is not a valid string.

        """
        return QuoteTick.from_serializable_str_c(instrument_id, values)

    cpdef str to_serializable_str(self):
        """
        Return a serializable string representation of this object.

        Returns
        -------
        str

        """
        return f"{self.bid},{self.ask},{self.bid_size},{self.ask_size},{self.ts_event_ns},{self.ts_recv_ns}"

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return {
            "type": type(self).__name__,
            "instrument_id": self.instrument_id.value,
            "bid": str(self.bid),
            "ask": str(self.ask),
            "bid_size": str(self.bid_size),
            "ask_size": str(self.ask_size),
            "ts_event_ns": self.ts_event_ns,
            "ts_recv_ns": self.ts_recv_ns,
        }


cdef class TradeTick(Tick):
    """
    Represents a single trade tick in a financial market.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Price price not None,
        Quantity size not None,
        AggressorSide aggressor_side,
        TradeMatchId match_id not None,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
    ):
        """
        Initialize a new instance of the ``TradeTick`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument identifier.
        price : Price
            The price of the trade.
        size : Quantity
            The size of the trade.
        aggressor_side : AggressorSide
            The aggressor side of the trade.
        match_id : TradeMatchId
            The trade match identifier.
        ts_event_ns : int64
            The UNIX timestamp (nanos) when data event occurred.
        ts_recv_ns : int64
            The UNIX timestamp (nanos) when received by the Nautilus system.

        """
        super().__init__(instrument_id, ts_event_ns, ts_recv_ns)

        self.price = price
        self.size = size
        self.aggressor_side = aggressor_side
        self.match_id = match_id

    def __str__(self) -> str:
        return (f"{self.instrument_id},"
                f"{self.price},"
                f"{self.size},"
                f"{AggressorSideParser.to_str(self.aggressor_side)},"
                f"{self.match_id},"
                f"{self.ts_event_ns}")

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return {
            "type": type(self).__name__,
            "instrument_id": self.instrument_id.value,
            "price": str(self.price),
            "size": str(self.size),
            "aggressor_side": AggressorSideParser.to_str(self.aggressor_side),
            "match_id": str(self.match_id),
            "ts_event_ns": self.ts_event_ns,
            "ts_recv_ns": self.ts_recv_ns,
        }

    @staticmethod
    cdef TradeTick from_serializable_str_c(InstrumentId instrument_id, str values):
        Condition.not_none(instrument_id, "instrument_id")
        Condition.valid_string(values, "values")

        cdef list pieces = values.split(',', maxsplit=5)

        if len(pieces) != 6:
            raise ValueError(f"The TradeTick string value was malformed, was {values}")

        return TradeTick(
            instrument_id,
            Price.from_str_c(pieces[0]),
            Quantity.from_str_c(pieces[1]),
            AggressorSideParser.from_str(pieces[2]),
            TradeMatchId(pieces[3]),
            int(pieces[4]),
            int(pieces[5]),
        )

    @staticmethod
    def from_serializable_str(InstrumentId instrument_id, str values):
        """
        Parse a tick from the given instrument identifier and values string.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument_id.
        values : str
            The tick values string.

        Returns
        -------
        TradeTick

        Raises
        ------
        ValueError
            If values is not a valid string.

        """
        return TradeTick.from_serializable_str_c(instrument_id, values)

    cpdef str to_serializable_str(self):
        """
        Return a serializable string representation of this object.

        Returns
        -------
        str

        """
        return (f"{self.price},"
                f"{self.size},"
                f"{AggressorSideParser.to_str(self.aggressor_side)},"
                f"{self.match_id},"
                f"{self.ts_event_ns},"
                f"{self.ts_recv_ns}")
