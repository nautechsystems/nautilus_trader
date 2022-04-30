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

from libc.stdint cimport int64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.model cimport instrument_id_from_buffers
from nautilus_trader.core.rust.model cimport quote_tick_free
from nautilus_trader.core.rust.model cimport quote_tick_from_raw
from nautilus_trader.core.rust.model cimport trade_id_from_buffer
from nautilus_trader.core.rust.model cimport trade_tick_free
from nautilus_trader.core.rust.model cimport trade_tick_from_raw
from nautilus_trader.core.string cimport pystr_to_buffer16
from nautilus_trader.core.string cimport pystr_to_buffer64
from nautilus_trader.core.string cimport pystr_to_buffer128
from nautilus_trader.model.c_enums.aggressor_side cimport AggressorSide
from nautilus_trader.model.c_enums.aggressor_side cimport AggressorSideParser
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.price_type cimport PriceTypeParser
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class QuoteTick(Data):
    """
    Represents a single quote tick in a financial market.

    Contains information about the best top of book bid and ask.

    Parameters
    ----------
    instrument_id : InstrumentId
        The quotes instrument ID.
    bid : Price
        The top of book bid price.
    ask : Price
        The top of book ask price.
    bid_size : Quantity
        The top of book bid size.
    ask_size : Quantity
        The top of book ask size.
    ts_event: int64
        The UNIX timestamp (nanoseconds) when the tick event occurred.
    ts_init: int64
        The UNIX timestamp (nanoseconds) when the data object was initialized.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Price bid not None,
        Price ask not None,
        Quantity bid_size not None,
        Quantity ask_size not None,
        int64_t ts_event,
        int64_t ts_init,
    ):
        super().__init__(ts_event, ts_init)

        # Temporary until identifiers moved to Rust
        self.instrument_id = instrument_id

        self._mem = quote_tick_from_raw(
            instrument_id_from_buffers(
                pystr_to_buffer128(instrument_id.symbol.value),
                pystr_to_buffer16(instrument_id.venue.value),
            ),
            bid.raw_int64_c(),
            ask.raw_int64_c(),
            bid._mem.precision,
            bid_size.raw_uint64_c(),
            ask_size.raw_uint64_c(),
            bid_size._mem.precision,
            ts_event,
            ts_init,
        )

    def __eq__(self, QuoteTick other) -> bool:
        return QuoteTick.to_dict_c(self) == QuoteTick.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(QuoteTick.to_dict_c(self)))

    def __str__(self) -> str:
        return (
            f"{self.instrument_id},"
            f"{self.bid},"
            f"{self.ask},"
            f"{self.bid_size},"
            f"{self.ask_size},"
            f"{self.ts_event}"
        )

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    def __del__(self) -> None:
        # https://cython.readthedocs.io/en/latest/src/userguide/special_methods.html#finalization-methods-dealloc-and-del
        quote_tick_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    @property
    def bid(self) -> Price:
        """
        The top of book bid price.

        Returns
        -------
        Price

        """
        return Price.from_raw_c(self._mem.bid.raw, self._mem.bid.precision)

    @property
    def ask(self) -> Price:
        """
        The top of book ask price.

        Returns
        -------
        Price

        """
        return Price.from_raw_c(self._mem.ask.raw, self._mem.ask.precision)

    @property
    def bid_size(self) -> Quantity:
        """
        The top of book bid size.

        Returns
        -------
        Quantity

        """
        return Quantity.from_raw_c(self._mem.bid_size.raw, self._mem.bid_size.precision)

    @property
    def ask_size(self) -> Quantity:
        """
        The top of book ask size.

        Returns
        -------
        Quantity

        """
        return Quantity.from_raw_c(self._mem.ask_size.raw, self._mem.ask_size.precision)

    @staticmethod
    cdef QuoteTick from_dict_c(dict values):
        Condition.not_none(values, "values")
        return QuoteTick(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            bid=Price.from_str_c(values["bid"]),
            ask=Price.from_str_c(values["ask"]),
            bid_size=Quantity.from_str_c(values["bid_size"]),
            ask_size=Quantity.from_str_c(values["ask_size"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
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
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
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
            The price type to extract.

        Returns
        -------
        Price

        """
        if price_type == PriceType.MID:
            return Price.from_raw_c(((self._mem.bid.raw + self._mem.ask.raw) / 2), self._mem.bid.precision + 1)
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
            The price type to extract.

        Returns
        -------
        Quantity

        """
        if price_type == PriceType.MID:
            return Quantity.from_raw_c((self._mem.bid_size.raw + self._mem.ask_size.raw) / 2, self._mem.bid_size.precision + 1)
        elif price_type == PriceType.BID:
            return self.bid_size
        elif price_type == PriceType.ASK:
            return self.ask_size
        else:
            raise ValueError(f"Cannot extract with PriceType {PriceTypeParser.to_str(price_type)}")


cdef class TradeTick(Data):
    """
    Represents a single trade tick in a financial market.

    Contains information about a single unique trade which matched buyer and
    seller counterparties.

    Parameters
    ----------
    instrument_id : InstrumentId
        The trade instrument ID.
    price : Price
        The traded price.
    size : Quantity
        The traded size.
    aggressor_side : AggressorSide
        The trade aggressor side.
    trade_id : TradeId
        The trade match ID (assigned by the venue).
    ts_event: int64
        The UNIX timestamp (nanoseconds) when the tick event occurred.
    ts_init: int64
        The UNIX timestamp (nanoseconds) when the data object was initialized.

    Raises
    ------
    ValueError
        If `trade_id` is not a valid string.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Price price not None,
        Quantity size not None,
        AggressorSide aggressor_side,
        TradeId trade_id not None,
        int64_t ts_event,
        int64_t ts_init,
    ):
        super().__init__(ts_event, ts_init)

        # Temporary until identifiers moved to Rust
        self.instrument_id = instrument_id
        self.trade_id = trade_id

        self._mem = trade_tick_from_raw(
            instrument_id_from_buffers(
                pystr_to_buffer128(instrument_id.symbol.value),
                pystr_to_buffer16(instrument_id.venue.value),
            ),
            price.raw_int64_c(),
            price._mem.precision,
            size.raw_uint64_c(),
            size._mem.precision,
            <OrderSide>aggressor_side,
            trade_id_from_buffer(pystr_to_buffer64(trade_id.value)),
            ts_event,
            ts_init,
        )

    def __eq__(self, TradeTick other) -> bool:
        return TradeTick.to_dict_c(self) == TradeTick.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(TradeTick.to_dict_c(self)))

    def __str__(self) -> str:
        return (
            f"{self.instrument_id.value},"
            f"{self.price},"
            f"{self.size},"
            f"{AggressorSideParser.to_str(self._mem.aggressor_side)},"
            f"{self.trade_id.value},"
            f"{self.ts_event}"
        )

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    def __del__(self) -> None:
        # https://cython.readthedocs.io/en/latest/src/userguide/special_methods.html#finalization-methods-dealloc-and-del
        trade_tick_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    @property
    def price(self) -> Price:
        """
        The ticks price.

        Returns
        -------
        Price

        """
        return Price.from_raw_c(self._mem.price.raw, self._mem.price.precision)

    @property
    def size(self) -> Price:
        """
        The ticks size.

        Returns
        -------
        Quantity

        """
        return Quantity.from_raw_c(self._mem.size.raw, self._mem.size.precision)

    @property
    def aggressor_side(self) -> AggressorSide:
        """
        The ticks aggressor side.

        Returns
        -------
        AggressorSide

        """
        return <AggressorSide>self._mem.aggressor_side

    @staticmethod
    cdef TradeTick from_dict_c(dict values):
        Condition.not_none(values, "values")
        return TradeTick(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            price=Price.from_str_c(values["price"]),
            size=Quantity.from_str_c(values["size"]),
            aggressor_side=AggressorSideParser.from_str(values["aggressor_side"]),
            trade_id=TradeId(values["trade_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(TradeTick obj):
        Condition.not_none(obj, "obj")
        return {
            "type": type(obj).__name__,
            "instrument_id": obj.instrument_id.value,
            "price": str(obj.price),
            "size": str(obj.size),
            "aggressor_side": AggressorSideParser.to_str(obj._mem.aggressor_side),
            "trade_id": obj.trade_id.value,
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> TradeTick:
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
