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

from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

import uuid

import msgspec

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.model cimport book_order_debug_to_cstr
from nautilus_trader.core.rust.model cimport book_order_eq
from nautilus_trader.core.rust.model cimport book_order_exposure
from nautilus_trader.core.rust.model cimport book_order_from_raw
from nautilus_trader.core.rust.model cimport book_order_hash
from nautilus_trader.core.rust.model cimport book_order_signed_size
from nautilus_trader.core.rust.model cimport instrument_id_clone
from nautilus_trader.core.rust.model cimport orderbook_delta_clone
from nautilus_trader.core.rust.model cimport orderbook_delta_drop
from nautilus_trader.core.rust.model cimport orderbook_delta_eq
from nautilus_trader.core.rust.model cimport orderbook_delta_hash
from nautilus_trader.core.rust.model cimport orderbook_delta_new
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.model.enums_c cimport BookAction
from nautilus_trader.model.enums_c cimport BookType
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.enums_c cimport book_action_from_str
from nautilus_trader.model.enums_c cimport book_action_to_str
from nautilus_trader.model.enums_c cimport book_type_from_str
from nautilus_trader.model.enums_c cimport book_type_to_str
from nautilus_trader.model.enums_c cimport order_side_from_str
from nautilus_trader.model.enums_c cimport order_side_to_str
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


ORDER_BOOK_DATA = (OrderBookSnapshot, OrderBookDeltas, OrderBookDelta)


cdef class BookOrder:
    """
    Represents an order in a book.

    Parameters
    ----------
    side : OrderSide {``BUY``, ``SELL``}
        The order side.
    price : Price
        The order price.
    size : Quantity
        The order size.
    order_id : uint64_t
        The order ID.
    """

    def __init__(
        self,
        OrderSide side,
        Price price,
        Quantity size,
        uint64_t order_id,
    ):
        self._mem = book_order_from_raw(
            side,
            price._mem.raw,
            price._mem.precision,
            size._mem.raw,
            size._mem.precision,
            order_id,
        )

    def __eq__(self, BookOrder other) -> bool:
        return book_order_eq(&self._mem, &other._mem)

    def __hash__(self) -> int:
        return book_order_hash(&self._mem)

    def __repr__(self) -> str:
        return cstr_to_pystr(book_order_debug_to_cstr(&self._mem))

    @staticmethod
    cdef BookOrder from_mem_c(BookOrder_t mem):
        cdef BookOrder order = BookOrder.__new__(BookOrder)
        order._mem = mem
        return order

    @property
    def price(self) -> Price:
        """
        Return the book orders price.

        Returns
        -------
        Price

        """
        return Price.from_raw_c(self._mem.price.raw, self._mem.price.precision)

    @property
    def size(self) -> Price:
        """
        Return the book orders size.

        Returns
        -------
        Quantity

        """
        return Quantity.from_raw_c(self._mem.size.raw, self._mem.size.precision)

    @property
    def side(self) -> OrderSide:
        """
        Return the book orders side.

        Returns
        -------
        OrderSide

        """
        return <OrderSide>self._mem.side

    @property
    def order_id(self) -> uint64_t:
        """
        Return the book orders side.

        Returns
        -------
        uint64_t

        """
        return self._mem.order_id

    cpdef double exposure(self):
        """
        Return the total exposure for this order (price * size).

        Returns
        -------
        double

        """
        return book_order_exposure(&self._mem)

    cpdef double signed_size(self):
        """
        Return the signed size of the order (negative for ``SELL``).

        Returns
        -------
        double

        """
        return book_order_signed_size(&self._mem)

    @staticmethod
    cdef BookOrder from_dict_c(dict values):
        Condition.not_none(values, "values")
        return BookOrder(
            side=order_side_from_str(values["side"]),
            price=Price.from_str(values["price"]),
            size=Quantity.from_str(values["size"]),
            order_id=values["order_id"],
        )

    @staticmethod
    cdef dict to_dict_c(BookOrder obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "BookOrder",
            "side": order_side_to_str(obj.side),
            "price": str(obj.price),
            "size": str(obj.size),
            "order_id": obj.order_id,
        }

    @staticmethod
    def from_dict(dict values) -> BookOrder:
        """
        Return an order from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        BookOrder

        """
        return BookOrder.from_dict_c(values)

    @staticmethod
    def to_dict(BookOrder obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return BookOrder.to_dict_c(obj)


cdef class OrderBookDelta(Data):
    """
    Represents a single difference on an `OrderBook`.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the book.
    action : BookAction {``ADD``, ``UPDATE``, ``DELETE``, ``CLEAR``}
        The order book delta action.
    order : Order
        The order to apply.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the data object was initialized.
    flags : uint8_t, default 0
        The unique sequence number for the update. If default 0 then will increment the `sequence`.
    sequence : uint64_t, default 0
        The unique sequence number for the update. If default 0 then will increment the `sequence`.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        BookAction action,
        BookOrder order,
        uint64_t ts_event,
        uint64_t ts_init,
        uint8_t flags=0,
        uint64_t sequence=0,
    ):
        # Placeholder for now
        cdef BookOrder_t book_order = order._mem if order is not None else book_order_from_raw(
            OrderSide.NO_ORDER_SIDE,
            0,
            0,
            0,
            0,
            0,
        )
        self._mem = orderbook_delta_new(
            instrument_id_clone(&instrument_id._mem),
            action,
            book_order,
            flags,
            sequence,
            ts_event,
            ts_init,
        )

    def __del__(self) -> None:
        if self._mem.instrument_id.symbol.value != NULL:
            orderbook_delta_drop(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __eq__(self, OrderBookDelta other) -> bool:
        return orderbook_delta_eq(&self._mem, &other._mem)

    def __hash__(self) -> int:
        return orderbook_delta_hash(&self._mem)

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"action={book_action_to_str(self.action)}, "
            f"order={self.order}, "
            f"flags={self.flags}, "
            f"sequence={self.sequence}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the deltas book instrument ID.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId.from_mem_c(self._mem.instrument_id)

    @property
    def action(self) -> BookAction:
        """
        Return the deltas book action {``ADD``, ``UPDATE``, ``DELETE``, ``CLEAR``}

        Returns
        -------
        BookAction

        """
        return <BookAction>self._mem.action

    @property
    def order(self) -> BookOrder:
        """
        Return the deltas book order for the action.

        Returns
        -------
        BookOrder

        """
        return BookOrder.from_mem_c(self._mem.order)

    @property
    def flags(self) -> uint8_t:
        """
        Return the flags for the delta.

        Returns
        -------
        uint8_t

        """
        return self._mem.flags

    @property
    def sequence(self) -> uint64_t:
        """
        Return the sequence number for the delta.

        Returns
        -------
        uint64_t

        """
        return self._mem.sequence

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
    cdef OrderBookDelta from_mem_c(OrderBookDelta_t mem):
        cdef OrderBookDelta delta = OrderBookDelta.__new__(OrderBookDelta)
        delta._mem = orderbook_delta_clone(&mem)
        return delta

    @staticmethod
    cdef OrderBookDelta from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef BookAction action = book_action_from_str(values["action"])
        cdef BookOrder order = BookOrder.from_dict_c({
            "side": values["side"],
            "price": values["price"],
            "size": values["size"],
            "order_id": values["order_id"],
        }) if values["action"] != "CLEAR" else None
        return OrderBookDelta(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            action=action,
            order=order,
            flags=values["flags"],
            sequence=values["sequence"],
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderBookDelta obj):
        Condition.not_none(obj, "obj")
        cdef BookOrder order = obj.order
        return {
            "type": "OrderBookDelta",
            "instrument_id": obj.instrument_id.value,
            "action": book_action_to_str(obj._mem.action),
            "side": order_side_to_str(order.side) if order else None,
            "price": str(obj.order.price) if order else None,
            "size": str(obj.order.size) if order else None,
            "order_id": order._mem.order_id if order else None,
            "flags": obj._mem.flags,
            "sequence": obj._mem.sequence,
            "ts_event": obj._mem.ts_event,
            "ts_init": obj._mem.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> OrderBookDelta:
        """
        Return an order book delta from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderBookDelta

        """
        return OrderBookDelta.from_dict_c(values)

    @staticmethod
    def to_dict(OrderBookDelta obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderBookDelta.to_dict_c(obj)


cdef class OrderBookDeltas(Data):
    """
    Represents bulk `OrderBookDelta` updates for an `OrderBook`.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the book.
    deltas : list[OrderBookDelta]
        The list of order book changes.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the data object was initialized.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        list deltas not None,
        uint64_t ts_event,
        uint64_t ts_init,
        uint64_t sequence=0,
    ):
        self.instrument_id = instrument_id
        self.deltas = deltas
        self.sequence = sequence
        self.ts_event = ts_event
        self.ts_init = ts_init

    def __eq__(self, OrderBookDeltas other) -> bool:
        return OrderBookDeltas.to_dict_c(self) == OrderBookDeltas.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(OrderBookDeltas.to_dict_c(self)))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"{self.deltas}, "
            f"sequence={self.sequence}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderBookDeltas from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderBookDeltas(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            deltas=[OrderBookDelta.from_dict_c(d) for d in msgspec.json.decode(values["deltas"])],
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            sequence=values.get("update_id", 0),
        )

    @staticmethod
    cdef dict to_dict_c(OrderBookDeltas obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderBookDeltas",
            "instrument_id": obj.instrument_id.to_str(),
            "deltas": msgspec.json.encode([OrderBookDelta.to_dict_c(d) for d in obj.deltas]),
            "sequence": obj.sequence,
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> OrderBookDeltas:
        """
        Return order book deltas from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderBookDeltas

        """
        return OrderBookDeltas.from_dict_c(values)

    @staticmethod
    def to_dict(OrderBookDeltas obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderBookDeltas.to_dict_c(obj)


cdef class OrderBookSnapshot(Data):
    """
    Represents a snapshot in time for an `OrderBook`.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the book.
    bids : list
        The bids for the snapshot.
    asks : list
        The asks for the snapshot.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the data object was initialized.
    sequence : uint64, default 0
        The unique sequence number for the update. If default 0 then will increment the `sequence`.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        list bids not None,
        list asks not None,
        uint64_t ts_event,
        uint64_t ts_init,
        uint64_t sequence=0,
    ):
        self.instrument_id = instrument_id
        self.bids = bids
        self.asks = asks
        self.sequence = sequence
        self.ts_event = ts_event
        self.ts_init = ts_init

    def __eq__(self, OrderBookSnapshot other) -> bool:
        return OrderBookSnapshot.to_dict_c(self) == OrderBookSnapshot.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(OrderBookSnapshot.to_dict_c(self)))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"bids={self.bids}, "
            f"asks={self.asks}, "
            f"sequence={self.sequence}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderBookSnapshot from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderBookSnapshot(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            bids=msgspec.json.decode(values["bids"]),
            asks=msgspec.json.decode(values["asks"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            sequence=values.get("sequence", 0),
        )

    @staticmethod
    cdef dict to_dict_c(OrderBookSnapshot obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderBookSnapshot",
            "instrument_id": obj.instrument_id.to_str(),
            "sequence": obj.sequence,
            "bids": msgspec.json.encode(obj.bids),
            "asks": msgspec.json.encode(obj.asks),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> OrderBookSnapshot:
        """
        Return an order book snapshot from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderBookSnapshot

        """
        return OrderBookSnapshot.from_dict_c(values)

    @staticmethod
    def to_dict(OrderBookSnapshot obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderBookSnapshot.to_dict_c(obj)
