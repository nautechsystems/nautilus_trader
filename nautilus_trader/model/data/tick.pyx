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

from nautilus_trader.core.nautilus_pyo3 import QuoteTick as RustQuoteTick
from nautilus_trader.core.nautilus_pyo3 import TradeTick as RustTradeTick

from cpython.mem cimport PyMem_Free
from cpython.mem cimport PyMem_Malloc
from cpython.pycapsule cimport PyCapsule_Destructor
from cpython.pycapsule cimport PyCapsule_GetPointer
from cpython.pycapsule cimport PyCapsule_New
from libc.stdint cimport int64_t
from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.rust.model cimport instrument_id_from_cstr
from nautilus_trader.core.rust.model cimport quote_tick_eq
from nautilus_trader.core.rust.model cimport quote_tick_hash
from nautilus_trader.core.rust.model cimport quote_tick_new
from nautilus_trader.core.rust.model cimport quote_tick_to_cstr
from nautilus_trader.core.rust.model cimport symbol_new
from nautilus_trader.core.rust.model cimport trade_id_new
from nautilus_trader.core.rust.model cimport trade_tick_eq
from nautilus_trader.core.rust.model cimport trade_tick_hash
from nautilus_trader.core.rust.model cimport trade_tick_new
from nautilus_trader.core.rust.model cimport trade_tick_to_cstr
from nautilus_trader.core.rust.model cimport venue_new
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pystr_to_cstr
from nautilus_trader.core.string cimport ustr_to_pystr
from nautilus_trader.model.data.base cimport capsule_destructor
from nautilus_trader.model.enums_c cimport AggressorSide
from nautilus_trader.model.enums_c cimport PriceType
from nautilus_trader.model.enums_c cimport aggressor_side_from_str
from nautilus_trader.model.enums_c cimport aggressor_side_to_str
from nautilus_trader.model.enums_c cimport price_type_to_str
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
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
    bid_price : Price
        The top of book bid price.
    ask_price : Price
        The top of book ask price.
    bid_size : Quantity
        The top of book bid size.
    ask_size : Quantity
        The top of book ask size.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the tick event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the data object was initialized.

    Raises
    ------
    ValueError
        If `bid.precision` != `ask.precision`.
    ValueError
        If `bid_size.precision` != `ask_size.precision`.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Price bid_price not None,
        Price ask_price not None,
        Quantity bid_size not None,
        Quantity ask_size not None,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        Condition.equal(bid_price._mem.precision, ask_price._mem.precision, "bid_price.precision", "ask_price.precision")
        Condition.equal(bid_size._mem.precision, ask_size._mem.precision, "bid_size.precision", "ask_size.precision")

        self._mem = quote_tick_new(
            instrument_id._mem,
            bid_price._mem.raw,
            ask_price._mem.raw,
            bid_price._mem.precision,
            ask_price._mem.precision,
            bid_size._mem.raw,
            ask_size._mem.raw,
            bid_size._mem.precision,
            ask_size._mem.precision,
            ts_event,
            ts_init,
        )

    def __getstate__(self):
        return (
            self.instrument_id.value,
            self._mem.bid_price.raw,
            self._mem.ask_price.raw,
            self._mem.bid_price.precision,
            self._mem.ask_price.precision,
            self._mem.bid_size.raw,
            self._mem.ask_size.raw,
            self._mem.bid_size.precision,
            self._mem.ask_size.precision,
            self.ts_event,
            self.ts_init,
        )

    def __setstate__(self, state):
        cdef InstrumentId instrument_id = InstrumentId.from_str_c(state[0])
        self._mem = quote_tick_new(
            instrument_id._mem,
            state[1],
            state[2],
            state[3],
            state[4],
            state[5],
            state[6],
            state[7],
            state[8],
            state[9],
            state[10],
        )

    def __eq__(self, QuoteTick other) -> bool:
        return quote_tick_eq(&self._mem, &other._mem)

    def __hash__(self) -> int:
        return quote_tick_hash(&self._mem)

    def __str__(self) -> str:
        return self.to_str()

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    cdef str to_str(self):
        return cstr_to_pystr(quote_tick_to_cstr(&self._mem))

    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the tick instrument ID.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId.from_mem_c(self._mem.instrument_id)

    @property
    def bid_price(self) -> Price:
        """
        Return the top of book bid price.

        Returns
        -------
        Price

        """
        return Price.from_raw_c(self._mem.bid_price.raw, self._mem.bid_price.precision)

    @property
    def ask_price(self) -> Price:
        """
        Return the top of book ask price.

        Returns
        -------
        Price

        """
        return Price.from_raw_c(self._mem.ask_price.raw, self._mem.ask_price.precision)

    @property
    def bid_size(self) -> Quantity:
        """
        Return the top of book bid size.

        Returns
        -------
        Quantity

        """
        return Quantity.from_raw_c(self._mem.bid_size.raw, self._mem.bid_size.precision)

    @property
    def ask_size(self) -> Quantity:
        """
        Return the top of book ask size.

        Returns
        -------
        Quantity

        """
        return Quantity.from_raw_c(self._mem.ask_size.raw, self._mem.ask_size.precision)

    @property
    def ts_event(self) -> int:
        """
        The UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return self._mem.ts_event

    @property
    def ts_init(self) -> int:
        """
        The UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self._mem.ts_init

    @staticmethod
    cdef QuoteTick from_dict_c(dict values):
        Condition.not_none(values, "values")
        return QuoteTick(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            bid_price=Price.from_str_c(values["bid_price"]),
            ask_price=Price.from_str_c(values["ask_price"]),
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
            "instrument_id": str(obj.instrument_id),
            "bid_price": str(obj.bid_price),
            "ask_price": str(obj.ask_price),
            "bid_size": str(obj.bid_size),
            "ask_size": str(obj.ask_size),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    cdef QuoteTick from_raw_c(
        InstrumentId instrument_id,
        int64_t bid_price_raw,
        int64_t ask_price_raw,
        uint8_t bid_price_prec,
        uint8_t ask_price_prec,
        uint64_t bid_size_raw,
        uint64_t ask_size_raw,
        uint8_t bid_size_prec,
        uint8_t ask_size_prec,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        cdef QuoteTick tick = QuoteTick.__new__(QuoteTick)
        tick._mem = quote_tick_new(
            instrument_id._mem,
            bid_price_raw,
            ask_price_raw,
            bid_price_prec,
            ask_price_prec,
            bid_size_raw,
            ask_size_raw,
            bid_size_prec,
            ask_size_prec,
            ts_event,
            ts_init,
        )
        return tick

    @staticmethod
    cdef QuoteTick from_mem_c(QuoteTick_t mem):
        cdef QuoteTick quote_tick = QuoteTick.__new__(QuoteTick)
        quote_tick._mem = mem
        return quote_tick

    # SAFETY: Do NOT deallocate the capsule here
    # It is supposed to be deallocated by the creator
    @staticmethod
    cdef inline list capsule_to_list_c(object capsule):
        cdef CVec* data = <CVec*>PyCapsule_GetPointer(capsule, NULL)
        cdef QuoteTick_t* ptr = <QuoteTick_t*>data.ptr
        cdef list ticks = []

        cdef uint64_t i
        for i in range(0, data.len):
            ticks.append(QuoteTick.from_mem_c(ptr[i]))

        return ticks

    @staticmethod
    cdef inline list_to_capsule_c(list items):
        # Create a C struct buffer
        cdef uint64_t len_ = len(items)
        cdef QuoteTick_t * data = <QuoteTick_t *> PyMem_Malloc(len_ * sizeof(QuoteTick_t))
        cdef uint64_t i
        for i in range(len_):
            data[i] = (<QuoteTick> items[i])._mem
        if not data:
            raise MemoryError()

        # Create CVec
        cdef CVec * cvec = <CVec *> PyMem_Malloc(1 * sizeof(CVec))
        cvec.ptr = data
        cvec.len = len_
        cvec.cap = len_

        # Create PyCapsule
        return PyCapsule_New(cvec, NULL, <PyCapsule_Destructor>capsule_destructor)

    @staticmethod
    def list_from_capsule(capsule) -> list[QuoteTick]:
        return QuoteTick.capsule_to_list_c(capsule)

    @staticmethod
    def capsule_from_list(list items):
        return QuoteTick.list_to_capsule_c(items)

    @staticmethod
    def from_raw(
        InstrumentId instrument_id,
        int64_t bid_price_raw,
        int64_t ask_price_raw,
        uint8_t bid_price_prec,
        uint8_t ask_price_prec,
        uint64_t bid_size_raw ,
        uint64_t ask_size_raw,
        uint8_t bid_size_prec,
        uint8_t ask_size_prec,
        uint64_t ts_event,
        uint64_t ts_init,
    ) -> QuoteTick:
        """
        Return a quote tick from the given raw values.

        Parameters
        ----------
        instrument_id : InstrumentId
            The quotes instrument ID.
        bid_price_raw : int64_t
            The raw top of book bid price (as a scaled fixed precision integer).
        ask_price_raw : int64_t
            The raw top of book ask price (as a scaled fixed precision integer).
        bid_price_prec : uint8_t
            The bid price precision.
        ask_price_prec : uint8_t
            The ask price precision.
        bid_size_raw : uint64_t
            The raw top of book bid size (as a scaled fixed precision integer).
        ask_size_raw : uint64_t
            The raw top of book ask size (as a scaled fixed precision integer).
        bid_size_prec : uint8_t
            The bid size precision.
        ask_size_prec : uint8_t
            The ask size precision.
        ts_event : uint64_t
            The UNIX timestamp (nanoseconds) when the tick event occurred.
        ts_init : uint64_t
            The UNIX timestamp (nanoseconds) when the data object was initialized.

        Returns
        -------
        QuoteTick

        Raises
        ------
        ValueError
            If `bid_price_prec` != `ask_price_prec`.
        ValueError
            If `bid_size_prec` != `ask_size_prec`.

        """
        Condition.equal(bid_price_prec, ask_price_prec, "bid_price_prec", "ask_price_prec")
        Condition.equal(bid_size_prec, ask_size_prec, "bid_size_prec", "ask_size_prec")

        return QuoteTick.from_raw_c(
            instrument_id,
            bid_price_raw,
            ask_price_raw,
            bid_price_prec,
            ask_price_prec,
            bid_size_raw,
            ask_size_raw,
            bid_size_prec,
            ask_size_prec,
            ts_event,
            ts_init,
        )

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

    @staticmethod
    def from_pyo3(list pyo3_ticks) -> list[QuoteTick]:
        """
        Return quote ticks converted from the given pyo3 provided objects.

        Parameters
        ----------
        pyo3_ticks : list[RustQuoteTick]
            The Rust pyo3 quote ticks to convert from.

        Returns
        -------
        list[QuoteTick]

        """
        cdef list output = []

        cdef InstrumentId instrument_id = None
        cdef uint8_t bid_prec = 0
        cdef uint8_t ask_prec = 0
        cdef uint8_t bid_size_prec = 0
        cdef uint8_t ask_size_prec = 0

        cdef:
            QuoteTick tick
        for pyo3_tick in pyo3_ticks:
            if instrument_id is None:
                instrument_id = InstrumentId.from_str_c(pyo3_tick.instrument_id.value)
                bid_prec = pyo3_tick.bid_price.precision
                ask_prec = pyo3_tick.ask_price.precision
                bid_size_prec = pyo3_tick.bid_size.precision
                ask_size_prec = pyo3_tick.ask_size.precision

            tick = QuoteTick.from_raw_c(
                instrument_id,
                pyo3_tick.bid_price.raw,
                pyo3_tick.ask_price.raw,
                bid_prec,
                ask_prec,
                pyo3_tick.bid_size.raw,
                pyo3_tick.ask_size.raw,
                bid_size_prec,
                ask_size_prec,
                pyo3_tick.ts_event,
                pyo3_tick.ts_init,
            )
            output.append(tick)

        return output

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
            return Price.from_raw_c(((self._mem.bid_price.raw + self._mem.ask_price.raw) / 2), self._mem.bid_price.precision + 1)
        elif price_type == PriceType.BID:
            return self.bid_price
        elif price_type == PriceType.ASK:
            return self.ask_price
        else:
            raise ValueError(f"Cannot extract with PriceType {price_type_to_str(price_type)}")

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
            raise ValueError(f"Cannot extract with PriceType {price_type_to_str(price_type)}")


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
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the tick event occurred.
    ts_init : uint64_t
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
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        self._mem = trade_tick_new(
            instrument_id._mem,
            price._mem.raw,
            price._mem.precision,
            size._mem.raw,
            size._mem.precision,
            aggressor_side,
            trade_id._mem,
            ts_event,
            ts_init,
        )

    def __getstate__(self):
        return (
            self.instrument_id.value,
            self._mem.price.raw,
            self._mem.price.precision,
            self._mem.size.raw,
            self._mem.size.precision,
            self._mem.aggressor_side,
            self.trade_id.value,
            self.ts_event,
            self.ts_init,
        )

    def __setstate__(self, state):
        cdef InstrumentId instrument_id = InstrumentId.from_str_c(state[0])
        self._mem = trade_tick_new(
            instrument_id._mem,
            state[1],
            state[2],
            state[3],
            state[4],
            state[5],
            TradeId(state[6])._mem,
            state[7],
            state[8],
        )

    def __eq__(self, TradeTick other) -> bool:
        return trade_tick_eq(&self._mem, &other._mem)

    def __hash__(self) -> int:
        return trade_tick_hash(&self._mem)

    def __str__(self) -> str:
        return self.to_str()

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.to_str()})"

    cdef str to_str(self):
        return cstr_to_pystr(trade_tick_to_cstr(&self._mem))

    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the ticks instrument ID.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId.from_mem_c(self._mem.instrument_id)

    @property
    def trade_id(self) -> InstrumentId:
        """
        Return the ticks trade match ID.

        Returns
        -------
        Price

        """
        return TradeId.from_mem_c(self._mem.trade_id)

    @property
    def price(self) -> Price:
        """
        Return the ticks price.

        Returns
        -------
        Price

        """
        return Price.from_raw_c(self._mem.price.raw, self._mem.price.precision)

    @property
    def size(self) -> Price:
        """
        Return the ticks size.

        Returns
        -------
        Quantity

        """
        return Quantity.from_raw_c(self._mem.size.raw, self._mem.size.precision)

    @property
    def aggressor_side(self) -> AggressorSide:
        """
        Return the ticks aggressor side.

        Returns
        -------
        AggressorSide

        """
        return <AggressorSide>self._mem.aggressor_side

    @property
    def ts_event(self) -> int:
        """
        The UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return self._mem.ts_event

    @property
    def ts_init(self) -> int:
        """
        The UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self._mem.ts_init

    @staticmethod
    cdef TradeTick from_raw_c(
        InstrumentId instrument_id,
        int64_t price_raw,
        uint8_t price_prec,
        uint64_t size_raw,
        uint8_t size_prec,
        AggressorSide aggressor_side,
        TradeId trade_id,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        cdef TradeTick tick = TradeTick.__new__(TradeTick)
        tick._mem = trade_tick_new(
            instrument_id._mem,
            price_raw,
            price_prec,
            size_raw,
            size_prec,
            aggressor_side,
            trade_id._mem,
            ts_event,
            ts_init,
        )
        return tick

    @staticmethod
    cdef TradeTick from_mem_c(TradeTick_t mem):
        cdef TradeTick trade_tick = TradeTick.__new__(TradeTick)
        trade_tick._mem = mem
        return trade_tick

    # SAFETY: Do NOT deallocate the capsule here
    # It is supposed to be deallocated by the creator
    @staticmethod
    cdef inline list capsule_to_list_c(capsule):
        cdef CVec* data = <CVec *>PyCapsule_GetPointer(capsule, NULL)
        cdef TradeTick_t* ptr = <TradeTick_t *>data.ptr
        cdef list ticks = []

        cdef uint64_t i
        for i in range(0, data.len):
            ticks.append(TradeTick.from_mem_c(ptr[i]))

        return ticks

    @staticmethod
    cdef inline list_to_capsule_c(list items):

        # Create a C struct buffer
        cdef uint64_t len_ = len(items)
        cdef TradeTick_t * data = <TradeTick_t *> PyMem_Malloc(len_ * sizeof(TradeTick_t))
        cdef uint64_t i
        for i in range(len_):
            data[i] = (<TradeTick> items[i])._mem
        if not data:
            raise MemoryError()

        # Create CVec
        cdef CVec* cvec = <CVec *> PyMem_Malloc(1 * sizeof(CVec))
        cvec.ptr = data
        cvec.len = len_
        cvec.cap = len_

        # Create PyCapsule
        return PyCapsule_New(cvec, NULL, <PyCapsule_Destructor>capsule_destructor)

    @staticmethod
    def list_from_capsule(capsule) -> list[TradeTick]:
        return TradeTick.capsule_to_list_c(capsule)

    @staticmethod
    def capsule_from_list(items):
        return TradeTick.list_to_capsule_c(items)

    @staticmethod
    cdef TradeTick from_dict_c(dict values):
        Condition.not_none(values, "values")
        return TradeTick(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            price=Price.from_str_c(values["price"]),
            size=Quantity.from_str_c(values["size"]),
            aggressor_side=aggressor_side_from_str(values["aggressor_side"]),
            trade_id=TradeId(values["trade_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(TradeTick obj):
        Condition.not_none(obj, "obj")
        return {
            "type": type(obj).__name__,
            "instrument_id": str(obj.instrument_id),
            "price": str(obj.price),
            "size": str(obj.size),
            "aggressor_side": aggressor_side_to_str(obj._mem.aggressor_side),
            "trade_id": str(obj.trade_id),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_raw(
        InstrumentId instrument_id,
        int64_t price_raw,
        uint8_t price_prec,
        uint64_t size_raw,
        uint8_t size_prec,
        AggressorSide aggressor_side,
        TradeId trade_id,
        uint64_t ts_event,
        uint64_t ts_init,
    ) -> TradeTick:
        """
        Return a trade tick from the given raw values.

        Parameters
        ----------
        instrument_id : InstrumentId
            The trade instrument ID.
        price_raw : int64_t
            The traded raw price (as a scaled fixed precision integer).
        price_prec : uint8_t
            The traded price precision.
        size_raw : uint64_t
            The traded raw size (as a scaled fixed precision integer).
        size_prec : uint8_t
            The traded size precision.
        aggressor_side : AggressorSide
            The trade aggressor side.
        trade_id : TradeId
            The trade match ID (assigned by the venue).
        ts_event : uint64_t
            The UNIX timestamp (nanoseconds) when the tick event occurred.
        ts_init : uint64_t
            The UNIX timestamp (nanoseconds) when the data object was initialized.

        Returns
        -------
        TradeTick

        """
        return TradeTick.from_raw_c(
            instrument_id,
            price_raw,
            price_prec,
            size_raw,
            size_prec,
            aggressor_side,
            trade_id,
            ts_event,
            ts_init,
        )

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

    @staticmethod
    def from_pyo3(list pyo3_ticks) -> list[TradeTick]:
        """
        Return trade ticks converted from the given pyo3 provided objects.

        Parameters
        ----------
        pyo3_ticks : list[RustTradeTick]
            The Rust pyo3 trade ticks to convert from.

        Returns
        -------
        list[TradeTick]

        """
        cdef list output = []

        cdef InstrumentId instrument_id = None
        cdef uint8_t price_prec = 0
        cdef uint8_t size_prec = 0

        cdef:
            TradeTick tick
        for pyo3_tick in pyo3_ticks:
            if instrument_id is None:
                instrument_id = InstrumentId.from_str_c(pyo3_tick.instrument_id.value)
                price_prec = pyo3_tick.price.precision
                size_prec = pyo3_tick.price.precision

            tick = TradeTick.from_raw_c(
                instrument_id,
                pyo3_tick.price.raw,
                price_prec,
                pyo3_tick.size.raw,
                size_prec,
                pyo3_tick.aggressor_side.value,
                TradeId(pyo3_tick.trade_id.value),
                pyo3_tick.ts_event,
                pyo3_tick.ts_init,
            )
            output.append(tick)

        return output
