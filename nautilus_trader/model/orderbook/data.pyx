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

from libc.stdint cimport uint64_t

import uuid

import msgspec

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.model.c_enums.book_action cimport BookAction
from nautilus_trader.model.c_enums.book_action cimport BookActionParser
from nautilus_trader.model.c_enums.book_type cimport BookType
from nautilus_trader.model.c_enums.book_type cimport BookTypeParser
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser


cdef class OrderBookData(Data):
    """
    The abstract base class for all `OrderBook` data.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the book.
    book_type : BookType {``L1_TBBO``, ``L2_MBP``, ``L3_MBO``}
        The order book type.
    update_id : uint64, default 0
        The unique ID for the data.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the data object was initialized.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        BookType book_type,
        uint64_t update_id,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        super().__init__(ts_event, ts_init)

        self.instrument_id = instrument_id
        self.book_type = book_type
        self.update_id = update_id


cdef class OrderBookSnapshot(OrderBookData):
    """
    Represents a snapshot in time for an `OrderBook`.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the book.
    book_type : BookType {``L1_TBBO``, ``L2_MBP``, ``L3_MBO``}
        The order book type.
    bids : list
        The bids for the snapshot.
    asks : list
        The asks for the snapshot.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the data object was initialized.
    update_id : uint64, default 0
        The unique ID for the data. If zero then order book will maintain ID internally.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        BookType book_type,
        list bids not None,
        list asks not None,
        uint64_t ts_event,
        uint64_t ts_init,
        uint64_t update_id=0,
    ):
        super().__init__(instrument_id, book_type, update_id, ts_event, ts_init)

        self.bids = bids
        self.asks = asks

    def __eq__(self, OrderBookSnapshot other) -> bool:
        return OrderBookSnapshot.to_dict_c(self) == OrderBookSnapshot.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(OrderBookSnapshot.to_dict_c(self)))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"'{self.instrument_id}', "
            f"book_type={BookTypeParser.to_str(self.book_type)}, "
            f"bids={self.bids}, "
            f"asks={self.asks}, "
            f"update_id={self.update_id}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderBookSnapshot from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderBookSnapshot(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            book_type=BookTypeParser.from_str(values["book_type"]),
            bids=msgspec.json.decode(values["bids"]),
            asks=msgspec.json.decode(values["asks"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            update_id=values.get("update_id", 0),
        )

    @staticmethod
    cdef dict to_dict_c(OrderBookSnapshot obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderBookSnapshot",
            "instrument_id": obj.instrument_id.to_str(),
            "book_type": BookTypeParser.to_str(obj.book_type),
            "update_id": obj.update_id,
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


cdef class OrderBookDeltas(OrderBookData):
    """
    Represents bulk changes for an `OrderBook`.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the book.
    book_type : BookType {``L1_TBBO``, ``L2_MBP``, ``L3_MBO``}
        The order book type.
    deltas : list[OrderBookDelta]
        The list of order book changes.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the data object was initialized.
    update_id : uint64, default 0
        The unique ID for the data. If zero then order book will maintain ID internally.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        BookType book_type,
        list deltas not None,
        uint64_t ts_event,
        uint64_t ts_init,
        uint64_t update_id=0,
    ):
        super().__init__(instrument_id, book_type, update_id, ts_event, ts_init)

        self.deltas = deltas

    def __eq__(self, OrderBookDeltas other) -> bool:
        return OrderBookDeltas.to_dict_c(self) == OrderBookDeltas.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(OrderBookDeltas.to_dict_c(self)))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"'{self.instrument_id}', "
            f"book_type={BookTypeParser.to_str(self.book_type)}, "
            f"{self.deltas}, "
            f"update_id={self.update_id}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderBookDeltas from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderBookDeltas(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            book_type=BookTypeParser.from_str(values["book_type"]),
            deltas=[OrderBookDelta.from_dict_c(d) for d in msgspec.json.decode(values["deltas"])],
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            update_id=values.get("update_id", 0),
        )

    @staticmethod
    cdef dict to_dict_c(OrderBookDeltas obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderBookDeltas",
            "instrument_id": obj.instrument_id.to_str(),
            "book_type": BookTypeParser.to_str(obj.book_type),
            "deltas": msgspec.json.encode([OrderBookDelta.to_dict_c(d) for d in obj.deltas]),
            "update_id": obj.update_id,
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


cdef class OrderBookDelta(OrderBookData):
    """
    Represents a single difference on an `OrderBook`.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the book.
    book_type : BookType {``L1_TBBO``, ``L2_MBP``, ``L3_MBO``}
        The order book type.
    action : BookAction {``ADD``, ``UPDATED``, ``DELETE``, ``CLEAR``}
        The order book delta action.
    order : Order
        The order to apply.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the data object was initialized.
    update_id : uint64_t, default 0
        The unique ID for the data. If zero then order book will maintain ID internally.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        BookType book_type,
        BookAction action,
        Order order,
        uint64_t ts_event,
        uint64_t ts_init,
        uint64_t update_id=0,
    ):
        super().__init__(instrument_id, book_type, update_id, ts_event, ts_init)

        self.action = action
        self.order = order

    def __eq__(self, OrderBookDelta other) -> bool:
        return OrderBookDelta.to_dict_c(self) == OrderBookDelta.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(OrderBookDelta.to_dict_c(self)))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"'{self.instrument_id}', "
            f"book_type={BookTypeParser.to_str(self.book_type)}, "
            f"action={BookActionParser.to_str(self.action)}, "
            f"order={self.order}, "
            f"update_id={self.update_id}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderBookDelta from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef BookAction action = BookActionParser.from_str(values["action"])
        cdef Order order = Order.from_dict_c({
            "price": values["order_price"],
            "size": values["order_size"],
            "side": values["order_side"],
            "id": values["order_id"],
        }) if values['action'] != "CLEAR" else None
        return OrderBookDelta(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            book_type=BookTypeParser.from_str(values["book_type"]),
            action=action,
            order=order,
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            update_id=values.get("update_id", 0),
        )

    @staticmethod
    cdef dict to_dict_c(OrderBookDelta obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderBookDelta",
            "instrument_id": obj.instrument_id.to_str(),
            "book_type": BookTypeParser.to_str(obj.book_type),
            "action": BookActionParser.to_str(obj.action),
            "order_price": obj.order.price if obj.order else None,
            "order_size": obj.order.size if obj.order else None,
            "order_side": OrderSideParser.to_str(obj.order.side) if obj.order else None,
            "order_id": obj.order.id if obj.order else None,
            "update_id": obj.update_id,
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
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


cdef class Order:
    """
    Represents an order in a book.

    Parameters
    ----------
    price : double
        The order price.
    size : double
        The order size.
    side : OrderSide {``BUY``, ``SELL``}
        The order side.
    id : str
        The order ID.
    """

    def __init__(
        self,
        double price,
        double size,
        OrderSide side,
        str id = None,  # noqa (shadows built-in name)
    ):
        self.price = price
        self.size = size
        self.side = side
        self.id = id or str(uuid.uuid4())

    def __eq__(self, Order other) -> bool:
        return self.id == other.id

    def __hash__(self) -> int:
        return hash(frozenset(Order.to_dict_c(self)))

    def __repr__(self) -> str:
        return f"{Order.__name__}({self.price}, {self.size}, {OrderSideParser.to_str(self.side)}, {self.id})"

    cpdef void update_price(self, double price) except *:
        """
        Update the orders price.

        Parameters
        ----------
        price : double
            The updated price.

        """
        self.price = price

    cpdef void update_size(self, double size) except *:
        """
        Update the orders size.

        Parameters
        ----------
        size : double
            The updated size.

        """
        self.size = size

    cpdef void update_id(self, str value) except *:
        """
        Update the orders ID.

        Parameters
        ----------
        value : str
            The updated ID.

        """
        self.id = value

    cpdef double exposure(self):
        """
        Return the total exposure for this order (price * size).

        Returns
        -------
        double

        """
        return self.price * self.size

    cpdef double signed_size(self):
        """
        Return the signed size of the order (negative for ``SELL``).

        Returns
        -------
        double

        """
        if self.side == OrderSide.BUY:
            return self.size * 1.0
        else:
            return self.size * -1.0

    @staticmethod
    cdef Order from_dict_c(dict values):
        Condition.not_none(values, "values")
        return Order(
            price=values["price"],
            size=values["size"],
            side=OrderSideParser.from_str(values["side"]),
            id=values["id"],
        )

    @staticmethod
    cdef dict to_dict_c(Order obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "Order",
            "price": obj.price,
            "size": obj.size,
            "side": OrderSideParser.to_str(obj.side),
            "id": str(obj.id),
        }

    @staticmethod
    def from_dict(dict values) -> Order:
        """
        Return an order from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        Order

        """
        return Order.from_dict_c(values)

    @staticmethod
    def to_dict(Order obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return Order.to_dict_c(obj)
