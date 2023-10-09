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

from typing import Optional

import msgspec

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
from nautilus_trader.core.rust.model cimport book_order_debug_to_cstr
from nautilus_trader.core.rust.model cimport book_order_eq
from nautilus_trader.core.rust.model cimport book_order_exposure
from nautilus_trader.core.rust.model cimport book_order_from_raw
from nautilus_trader.core.rust.model cimport book_order_hash
from nautilus_trader.core.rust.model cimport book_order_signed_size
from nautilus_trader.core.rust.model cimport orderbook_delta_eq
from nautilus_trader.core.rust.model cimport orderbook_delta_hash
from nautilus_trader.core.rust.model cimport orderbook_delta_new
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.model.data.base cimport capsule_destructor
from nautilus_trader.model.enums_c cimport BookAction
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.enums_c cimport book_action_from_str
from nautilus_trader.model.enums_c cimport book_action_to_str
from nautilus_trader.model.enums_c cimport order_side_from_str
from nautilus_trader.model.enums_c cimport order_side_to_str
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


# Represents a 'NULL' order (used for 'CLEAR' actions by OrderBookDelta)
NULL_ORDER = BookOrder(
    side=OrderSide.NO_ORDER_SIDE,
    price=Price(0, 0),
    size=Quantity(0, 0),
    order_id=0,
)


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
        Price price not None,
        Quantity size not None,
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

    def __getstate__(self):
        return (
            self._mem.side,
            self._mem.price.raw,
            self._mem.price.precision,
            self._mem.size.raw ,
            self._mem.size.precision,
            self._mem.order_id,
        )

    def __setstate__(self, state):
        self._mem = book_order_from_raw(
            state[0],
            state[1],
            state[2],
            state[3],
            state[4],
            state[5],
        )

    def __eq__(self, BookOrder other) -> bool:
        return book_order_eq(&self._mem, &other._mem)

    def __hash__(self) -> int:
        return book_order_hash(&self._mem)

    def __repr__(self) -> str:
        return cstr_to_pystr(book_order_debug_to_cstr(&self._mem))

    @staticmethod
    cdef BookOrder from_raw_c(
        OrderSide side,
        int64_t price_raw,
        uint8_t price_prec,
        uint64_t size_raw,
        uint8_t size_prec,
        uint64_t order_id,
    ):
        cdef BookOrder order = BookOrder.__new__(BookOrder)
        order._mem = book_order_from_raw(
            side,
            price_raw,
            price_prec,
            size_raw,
            size_prec,
            order_id,
        )
        return order

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
    def from_raw(
        OrderSide side,
        int64_t price_raw,
        uint8_t price_prec,
        uint64_t size_raw,
        uint8_t size_prec,
        uint64_t order_id,
    ) -> BookOrder:
        """
        Return an book order from the given raw values.

        Parameters
        ----------
        side : OrderSide {``BUY``, ``SELL``}
            The order side.
        price_raw : int64_t
            The order raw price (as a scaled fixed precision integer).
        price_prec : uint8_t
            The order price precision.
        size_raw : uint64_t
            The order raw size (as a scaled fixed precision integer).
        size_prec : uint8_t
            The order size precision.
        order_id : uint64_t
            The order ID.

        Returns
        -------
        BookOrder

        """
        return BookOrder.from_raw_c(
            side,
            price_raw,
            price_prec,
            size_raw,
            size_prec,
            order_id,
        )

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
    Represents a single update/difference on an `OrderBook`.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the book.
    action : BookAction {``ADD``, ``UPDATE``, ``DELETE``, ``CLEAR``}
        The order book delta action.
    order : BookOrder
        The book order for the delta.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the data object was initialized.
    flags : uint8_t, default 0 (no flags)
        A combination of packet end with matching engine status.
    sequence : uint64_t, default 0
        The unique sequence number for the update.
        If default 0 then will increment the `sequence`.
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
            instrument_id._mem,
            action,
            book_order,
            flags,
            sequence,
            ts_event,
            ts_init,
        )

    def __getstate__(self):
        return (
            self.instrument_id.value,
            self._mem.action,
            self._mem.order.side,
            self._mem.order.price.raw,
            self._mem.order.price.precision,
            self._mem.order.size.raw ,
            self._mem.order.size.precision,
            self._mem.order.order_id,
            self._mem.flags,
            self._mem.sequence,
            self._mem.ts_event,
            self._mem.ts_init,
        )

    def __setstate__(self, state):
        cdef InstrumentId instrument_id = InstrumentId.from_str_c(state[0])
        cdef BookOrder_t book_order = book_order_from_raw(
            state[2],
            state[3],
            state[4],
            state[5],
            state[6],
            state[7],
        )
        self._mem = orderbook_delta_new(
            instrument_id._mem,
            state[1],
            book_order,
            state[8],
            state[9],
            state[10],
            state[11],
        )

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
            f"flags={self._mem.flags}, "
            f"sequence={self._mem.sequence}, "
            f"ts_event={self._mem.ts_event}, "
            f"ts_init={self._mem.ts_init})"
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
    def is_add(self) -> BookAction:
        """
        If the deltas book action is an ``ADD``.

        Returns
        -------
        bool

        """
        return <BookAction>self._mem.action == BookAction.ADD

    @property
    def is_update(self) -> BookAction:
        """
        If the deltas book action is an ``UPDATE``.

        Returns
        -------
        bool

        """
        return <BookAction>self._mem.action == BookAction.UPDATE

    @property
    def is_delete(self) -> BookAction:
        """
        If the deltas book action is a ``DELETE``.

        Returns
        -------
        bool

        """
        return <BookAction>self._mem.action == BookAction.DELETE

    @property
    def is_clear(self) -> BookAction:
        """
        If the deltas book action is a ``CLEAR``.

        Returns
        -------
        bool

        """
        return <BookAction>self._mem.action == BookAction.CLEAR

    @property
    def order(self) -> Optional[BookOrder]:
        """
        Return the deltas book order for the action.

        Returns
        -------
        BookOrder

        """
        order = self._mem.order
        if order is None:
            return None
        return BookOrder.from_mem_c(order)

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
    cdef OrderBookDelta from_raw_c(
        InstrumentId instrument_id,
        BookAction action,
        OrderSide side,
        int64_t price_raw,
        uint8_t price_prec,
        uint64_t size_raw,
        uint8_t size_prec,
        uint64_t order_id,
        uint8_t flags,
        uint64_t sequence,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        cdef BookOrder_t order_mem = book_order_from_raw(
            side,
            price_raw,
            price_prec,
            size_raw,
            size_prec,
            order_id,
        )
        cdef OrderBookDelta delta = OrderBookDelta.__new__(OrderBookDelta)
        delta._mem = orderbook_delta_new(
            instrument_id._mem,
            action,
            order_mem,
            flags,
            sequence,
            ts_event,
            ts_init,
        )
        return delta

    @staticmethod
    cdef OrderBookDelta from_mem_c(OrderBookDelta_t mem):
        cdef OrderBookDelta delta = OrderBookDelta.__new__(OrderBookDelta)
        delta._mem = mem
        return delta

    @staticmethod
    cdef OrderBookDelta from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef BookAction action = book_action_from_str(values["action"])
        cdef BookOrder order = BookOrder.from_dict_c(values["order"])
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
            "order": {
                "side": order_side_to_str(order._mem.side),
                "price": str(order.price),
                "size": str(order.size),
                "order_id": order._mem.order_id,
            },
            "flags": obj._mem.flags,
            "sequence": obj._mem.sequence,
            "ts_event": obj._mem.ts_event,
            "ts_init": obj._mem.ts_init,
        }

    @staticmethod
    cdef OrderBookDelta clear_c(
        InstrumentId instrument_id,
        uint64_t ts_event,
        uint64_t ts_init,
        uint64_t sequence=0,
    ):
        return OrderBookDelta(
            instrument_id=instrument_id,
            action=BookAction.CLEAR,
            order=None,
            ts_event=ts_event,
            ts_init=ts_init,
            sequence=sequence,
        )

    # SAFETY: Do NOT deallocate the capsule here
    # It is supposed to be deallocated by the creator
    @staticmethod
    cdef inline list capsule_to_list_c(object capsule):
        cdef CVec* data = <CVec*>PyCapsule_GetPointer(capsule, NULL)
        cdef OrderBookDelta_t* ptr = <OrderBookDelta_t*>data.ptr
        cdef list deltas = []

        cdef uint64_t i
        for i in range(0, data.len):
            deltas.append(OrderBookDelta.from_mem_c(ptr[i]))

        return deltas

    @staticmethod
    cdef inline list_to_capsule_c(list items):
        # Create a C struct buffer
        cdef uint64_t len_ = len(items)
        cdef OrderBookDelta_t * data = <OrderBookDelta_t *> PyMem_Malloc(len_ * sizeof(OrderBookDelta_t))
        cdef uint64_t i
        for i in range(len_):
            data[i] = (<OrderBookDelta> items[i])._mem
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
        return OrderBookDelta.capsule_to_list_c(capsule)

    @staticmethod
    def capsule_from_list(list items):
        return OrderBookDelta.list_to_capsule_c(items)

    @staticmethod
    def from_raw(
        InstrumentId instrument_id,
        BookAction action,
        OrderSide side,
        int64_t price_raw,
        uint8_t price_prec,
        uint64_t size_raw,
        uint8_t size_prec,
        uint64_t order_id,
        uint8_t flags,
        uint64_t sequence,
        uint64_t ts_event,
        uint64_t ts_init,
    ) -> OrderBookDelta:
        """
        Return an order book delta from the given raw values.

        Parameters
        ----------
        instrument_id : InstrumentId
            The trade instrument ID.
        action : BookAction {``ADD``, ``UPDATE``, ``DELETE``, ``CLEAR``}
            The order book delta action.
        side : OrderSide {``BUY``, ``SELL``}
            The order side.
        price_raw : int64_t
            The order raw price (as a scaled fixed precision integer).
        price_prec : uint8_t
            The order price precision.
        size_raw : uint64_t
            The order raw size (as a scaled fixed precision integer).
        size_prec : uint8_t
            The order size precision.
        order_id : uint64_t
            The order ID.
        flags : uint8_t
            A combination of packet end with matching engine status.
        sequence : uint64_t
            The unique sequence number for the update.
        ts_event : uint64_t
            The UNIX timestamp (nanoseconds) when the tick event occurred.
        ts_init : uint64_t
            The UNIX timestamp (nanoseconds) when the data object was initialized.

        Returns
        -------
        OrderBookDelta

        """
        return OrderBookDelta.from_raw_c(
            instrument_id,
            action,
            side,
            price_raw,
            price_prec,
            size_raw,
            size_prec,
            order_id,
            flags,
            sequence,
            ts_event,
            ts_init,
        )

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

    @staticmethod
    def clear(InstrumentId instrument_id, uint64_t ts_event, uint64_t ts_init, uint64_t sequence=0):
        """
        Return an order book delta which acts as an initial ``CLEAR``.

        Returns
        -------
        OrderBookDelta

        """
        return OrderBookDelta.clear_c(instrument_id, ts_event, ts_init, sequence)

    @staticmethod
    def from_pyo3(list pyo3_deltas) -> list[OrderBookDelta]:
        """
        Return order book deltas converted from the given pyo3 provided objects.

        Parameters
        ----------
        pyo3_deltas : list[RustOrderBookDelta]
            The Rust pyo3 order book deltas to convert from.

        Returns
        -------
        list[OrderBookDelta]

        """
        cdef list output = []

        cdef InstrumentId instrument_id = None
        cdef uint8_t price_prec = 0
        cdef uint8_t size_prec = 0

        cdef:
            OrderBookDelta delta
            BookOrder book_order
        for pyo3_delta in pyo3_deltas:
            if instrument_id is None:
                instrument_id = InstrumentId.from_str_c(pyo3_delta.instrument_id.value)
                price_prec = pyo3_delta.order.price.precision
                size_prec = pyo3_delta.order.size.precision

            book_order = BookOrder(
               pyo3_delta.order.side.value,
               Price.from_raw_c(pyo3_delta.order.price.raw, price_prec),
               Quantity.from_raw_c(pyo3_delta.order.size.raw, size_prec),
               pyo3_delta.order.order_id,
            )

            delta = OrderBookDelta(
                instrument_id,
                pyo3_delta.action.value,
                book_order,
                pyo3_delta.ts_event,
                pyo3_delta.ts_init,
                pyo3_delta.flags,
                pyo3_delta.sequence,
            )
            output.append(delta)

        return output


cdef class OrderBookDeltas(Data):
    """
    Represents bulk `OrderBookDelta` updates for an `OrderBook`.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the book.
    deltas : list[OrderBookDelta]
        The list of order book changes.

    Raises
    ------
    ValueError
        If `deltas` is an empty list.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        list deltas not None,
    ):
        Condition.not_empty(deltas, "deltas")

        self.instrument_id = instrument_id
        self.deltas = deltas
        self.is_snapshot = deltas[0].is_clear
        self.sequence = deltas[-1].sequence
        self.ts_event = deltas[-1].ts_event
        self.ts_init = deltas[-1].ts_init

    def __eq__(self, OrderBookDeltas other) -> bool:
        return OrderBookDeltas.to_dict_c(self) == OrderBookDeltas.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(OrderBookDeltas.to_dict_c(self)))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"{self.deltas}, "
            f"is_snapshot={self.is_snapshot}, "
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
        )

    @staticmethod
    cdef dict to_dict_c(OrderBookDeltas obj):
        Condition.not_none(obj, "obj")
        return {
            "type": obj.__class__.__name__,
            "instrument_id": obj.instrument_id.to_str(),
            "deltas": msgspec.json.encode([OrderBookDelta.to_dict_c(d) for d in obj.deltas]),
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
