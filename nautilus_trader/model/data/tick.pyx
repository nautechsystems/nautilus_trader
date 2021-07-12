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
from nautilus_trader.model.data.base cimport Data
from nautilus_trader.model.identifiers cimport InstrumentId
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
        int64_t ts_recv_ns,
    ):
        """
        Initialize a new instance of the ``Tick`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The ticks instrument ID.
        ts_event_ns: int64
            The UNIX timestamp (nanoseconds) when data event occurred.
        ts_recv_ns : int64
            The UNIX timestamp (nanoseconds) when received by the Nautilus system.

        """
        super().__init__(ts_event_ns, ts_recv_ns)

        self.instrument_id = instrument_id


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
            The quotes instrument ID.
        bid : Price
            The best bid price.
        ask : Price
            The best ask price.
        bid_size : Quantity
            The size at the best bid.
        ask_size : Quantity
            The size at the best ask.
        ts_event_ns: int64
            The UNIX timestamp (nanoseconds) when data event occurred.
        ts_recv_ns: int64
            The UNIX timestamp (nanoseconds) when received by the Nautilus system.

        """
        super().__init__(instrument_id, ts_event_ns, ts_recv_ns)

        self.bid = bid
        self.ask = ask
        self.bid_size = bid_size
        self.ask_size = ask_size

    def __eq__(self, QuoteTick other) -> bool:
        return QuoteTick.to_dict_c(self) == QuoteTick.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(QuoteTick.to_dict_c(self)))

    def __str__(self) -> str:
        return (f"{self.instrument_id},"
                f"{self.bid},"
                f"{self.ask},"
                f"{self.bid_size},"
                f"{self.ask_size},"
                f"{self.ts_event_ns}")

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @staticmethod
    cdef QuoteTick from_dict_c(dict values):
        Condition.not_none(values, "values")
        return QuoteTick(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            bid=Price.from_str_c(values["bid"]),
            ask=Price.from_str_c(values["ask"]),
            bid_size=Quantity.from_str_c(values["bid_size"]),
            ask_size=Quantity.from_str_c(values["ask_size"]),
            ts_event_ns=values["ts_event_ns"],
            ts_recv_ns=values["ts_recv_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(QuoteTick obj):
        Condition.not_none(obj, "obj")
        return {
            "type": type(obj).__name__,
            "instrument_id": obj.instrument_id.value,
            "bid": str(obj.bid),
            "ask": str(obj.ask),
            "bid_size": str(obj.bid_size),
            "ask_size": str(obj.ask_size),
            "ts_event_ns": obj.ts_event_ns,
            "ts_recv_ns": obj.ts_recv_ns,
        }

    @staticmethod
    def from_dict(dict values) -> QuoteTick:
        """
        Return a quote tick parsed from the given values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        QuoteTick

        """
        return QuoteTick.from_dict_c(values)

    @staticmethod
    def to_dict(QuoteTick obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return QuoteTick.to_dict_c(obj)

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
        str match_id not None,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
    ):
        """
        Initialize a new instance of the ``TradeTick`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The trade instrument ID.
        price : Price
            The price of the trade.
        size : Quantity
            The size of the trade.
        aggressor_side : AggressorSide
            The aggressor side of the trade.
        match_id : str
            The trade match ID.
        ts_event_ns: int64
            The UNIX timestamp (nanoseconds) when data event occurred.
        ts_recv_ns: int64
            The UNIX timestamp (nanoseconds) when received by the Nautilus system.

        Raises
        ------
        ValueError
            If match_id is not a valid string.

        """
        Condition.valid_string(match_id, "match_id")
        super().__init__(instrument_id, ts_event_ns, ts_recv_ns)

        self.price = price
        self.size = size
        self.aggressor_side = aggressor_side
        self.match_id = match_id

    def __eq__(self, TradeTick other) -> bool:
        return TradeTick.to_dict_c(self) == TradeTick.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(TradeTick.to_dict_c(self)))

    def __str__(self) -> str:
        return (f"{self.instrument_id},"
                f"{self.price},"
                f"{self.size},"
                f"{AggressorSideParser.to_str(self.aggressor_side)},"
                f"{self.match_id},"
                f"{self.ts_event_ns}")

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @staticmethod
    cdef TradeTick from_dict_c(dict values):
        Condition.not_none(values, "values")
        return TradeTick(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            price=Price.from_str_c(values["price"]),
            size=Quantity.from_str_c(values["size"]),
            aggressor_side=AggressorSideParser.from_str(values["aggressor_side"]),
            match_id=values["match_id"],
            ts_event_ns=values["ts_event_ns"],
            ts_recv_ns=values["ts_recv_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(TradeTick obj):
        Condition.not_none(obj, "obj")
        return {
            "type": type(obj).__name__,
            "instrument_id": obj.instrument_id.value,
            "price": str(obj.price),
            "size": str(obj.size),
            "aggressor_side": AggressorSideParser.to_str(obj.aggressor_side),
            "match_id": obj.match_id,
            "ts_event_ns": obj.ts_event_ns,
            "ts_recv_ns": obj.ts_recv_ns,
        }

    @staticmethod
    def from_dict(dict values):
        """
        Return a trade tick from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        TradeTick

        """
        return TradeTick.from_dict_c(values)

    @staticmethod
    def to_dict(TradeTick obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return TradeTick.to_dict_c(obj)
