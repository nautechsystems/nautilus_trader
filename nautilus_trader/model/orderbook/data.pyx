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

import orjson

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.uuid cimport uuid4
from nautilus_trader.model.c_enums.book_level cimport BookLevel
from nautilus_trader.model.c_enums.book_level cimport BookLevelParser
from nautilus_trader.model.c_enums.delta_type cimport DeltaType
from nautilus_trader.model.c_enums.delta_type cimport DeltaTypeParser
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.data.base cimport Data


cdef class OrderBookData(Data):
    """
    The abstract base class for all `OrderBook` data.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        BookLevel level,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
    ):
        """
        Initialize a new instance of the ``OrderBookData`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the book.
        level : BookLevel
            The order book level (L1, L2, L3).
        ts_event_ns: int64
            The UNIX timestamp (nanoseconds) when data event occurred.
        ts_recv_ns: int64
            The UNIX timestamp (nanoseconds) when received by the Nautilus system.

        """
        super().__init__(ts_event_ns, ts_recv_ns)

        self.instrument_id = instrument_id
        self.level = level


cdef class OrderBookSnapshot(OrderBookData):
    """
    Represents a snapshot in time for an `OrderBook`.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        BookLevel level,
        list bids not None,
        list asks not None,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
    ):
        """
        Initialize a new instance of the ``OrderBookSnapshot`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the book.
        level : BookLevel
            The order book level (L1, L2, L3).
        bids : list
            The bids for the snapshot.
        asks : list
            The asks for the snapshot.
        ts_event_ns: int64
            The UNIX timestamp (nanoseconds) when data event occurred.
        ts_recv_ns: int64
            The UNIX timestamp (nanoseconds) when received by the Nautilus system.

        """
        super().__init__(instrument_id, level, ts_event_ns, ts_recv_ns)

        self.bids = bids
        self.asks = asks

    def __eq__(self, OrderBookSnapshot other) -> bool:
        return OrderBookSnapshot.to_dict_c(self) == OrderBookSnapshot.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(OrderBookSnapshot.to_dict_c(self)))

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"'{self.instrument_id}', "
                f"level={BookLevelParser.to_str(self.level)}, "
                f"bids={self.bids}, "
                f"asks={self.asks}, "
                f"ts_recv_ns={self.ts_recv_ns})")

    @staticmethod
    cdef OrderBookSnapshot from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderBookSnapshot(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            level=BookLevelParser.from_str(values["level"]),
            bids=orjson.loads(values["bids"]),
            asks=orjson.loads(values["asks"]),
            ts_event_ns=values["ts_event_ns"],
            ts_recv_ns=values["ts_recv_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderBookSnapshot obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderBookSnapshot",
            "instrument_id": obj.instrument_id.value,
            "level": BookLevelParser.to_str(obj.level),
            "bids": orjson.dumps(obj.bids),
            "asks": orjson.dumps(obj.asks),
            "ts_event_ns": obj.ts_event_ns,
            "ts_recv_ns": obj.ts_recv_ns,
        }

    @staticmethod
    def from_dict(dict values):
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
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        BookLevel level,
        list deltas not None,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
    ):
        """
        Initialize a new instance of the ``OrderBookDeltas`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the book.
        level : BookLevel
            The order book level (L1, L2, L3).
        deltas : list[OrderBookDelta]
            The list of order book changes.
        ts_event_ns: int64
            The UNIX timestamp (nanoseconds) when data event occurred.
        ts_recv_ns: int64
            The UNIX timestamp (nanoseconds) when received by the Nautilus system.

        """
        super().__init__(instrument_id, level, ts_event_ns, ts_recv_ns)

        self.deltas = deltas

    def __eq__(self, OrderBookDeltas other) -> bool:
        return OrderBookDeltas.to_dict_c(self) == OrderBookDeltas.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(OrderBookDeltas.to_dict_c(self)))

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"'{self.instrument_id}', "
                f"level={BookLevelParser.to_str(self.level)}, "
                f"{self.deltas}, "
                f"ts_recv_ns={self.ts_recv_ns})")

    @staticmethod
    cdef OrderBookDeltas from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderBookDeltas(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            level=BookLevelParser.from_str(values["level"]),
            deltas=[OrderBookDelta.from_dict_c(d) for d in orjson.loads(values["deltas"])],
            ts_event_ns=values["ts_event_ns"],
            ts_recv_ns=values["ts_recv_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderBookDeltas obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderBookDeltas",
            "instrument_id": obj.instrument_id.value,
            "level": BookLevelParser.to_str(obj.level),
            "deltas": orjson.dumps([OrderBookDelta.to_dict_c(d) for d in obj.deltas]),
            "ts_event_ns": obj.ts_event_ns,
            "ts_recv_ns": obj.ts_recv_ns,
        }

    @staticmethod
    def from_dict(dict values):
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
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        BookLevel level,
        DeltaType delta_type,
        Order order,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
    ):
        """
        Initialize a new instance of the ``OrderBookDelta`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID.
        level : BookLevel
            The book level for the delta.
        delta_type : DeltaType
            The type of change (ADD, UPDATED, DELETE, CLEAR).
        order : Order
            The order to apply.
        ts_event_ns: int64
            The UNIX timestamp (nanoseconds) when data event occurred.
        ts_recv_ns: int64
            The UNIX timestamp (nanoseconds) when received by the Nautilus system.

        """
        super().__init__(instrument_id, level, ts_event_ns, ts_recv_ns)

        self.type = delta_type
        self.order = order

    def __eq__(self, OrderBookDelta other) -> bool:
        return OrderBookDelta.to_dict_c(self) == OrderBookDelta.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(OrderBookDelta.to_dict_c(self)))

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"'{self.instrument_id}', "
                f"level={BookLevelParser.to_str(self.level)}, "
                f"delta_type={DeltaTypeParser.to_str(self.type)}, "
                f"order={self.order}, "
                f"ts_recv_ns={self.ts_recv_ns})")

    @staticmethod
    cdef OrderBookDelta from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef DeltaType delta_type = DeltaTypeParser.from_str(values["delta_type"])
        cdef Order order = Order.from_dict_c({
            "price": values["order_price"],
            "size": values["order_size"],
            "side": values["order_side"],
            "id": values["order_id"],
        }) if values['delta_type'] != "CLEAR" else None
        return OrderBookDelta(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            level=BookLevelParser.from_str(values["level"]),
            delta_type=DeltaTypeParser.from_str(values["delta_type"]),
            order=order,
            ts_event_ns=values["ts_event_ns"],
            ts_recv_ns=values["ts_recv_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderBookDelta obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderBookDelta",
            "instrument_id": obj.instrument_id.value,
            "level": BookLevelParser.to_str(obj.level),
            "delta_type": DeltaTypeParser.to_str(obj.type),
            "order_price": obj.order.price if obj.order else None,
            "order_size": obj.order.size if obj.order else None,
            "order_side": OrderSideParser.to_str(obj.order.side) if obj.order else None,
            "order_id": obj.order.id if obj.order else None,
            "ts_event_ns": obj.ts_event_ns,
            "ts_recv_ns": obj.ts_recv_ns,
        }

    @staticmethod
    def from_dict(dict values):
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
    """

    def __init__(
        self,
        double price,
        double size,
        OrderSide side,
        str id=None,  # noqa (shadows built-in name)
    ):
        """
        Initialize a new instance of the ``Order`` class.

        Parameters
        ----------
        price : double
            The order price.
        size : double
            The order size.
        side : OrderSide
            The order side.
        id : str
            The order ID.

        """
        self.price = price
        self.size = size
        self.side = side
        self.id = id or str(uuid4())

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
        Return the signed size of the order (negative for SELL).

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
            "id": obj.id,
        }

    @staticmethod
    def from_dict(dict values):
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
