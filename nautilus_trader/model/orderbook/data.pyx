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
from nautilus_trader.core.data cimport Data
from nautilus_trader.model.c_enums.book_action cimport BookAction
from nautilus_trader.model.c_enums.book_action cimport BookActionParser
from nautilus_trader.model.c_enums.book_level cimport BookLevel
from nautilus_trader.model.c_enums.book_level cimport BookLevelParser
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.orderbook.order cimport Order


cdef class OrderBookData(Data):
    """
    The abstract base class for all `OrderBook` data.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        BookLevel level,
        int64_t ts_event,
        int64_t ts_init,
    ):
        """
        Initialize a new instance of the ``OrderBookData`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the book.
        level : BookLevel {``L1``, ``L2``, ``L3``}
            The order book level.
        ts_event: int64
            The UNIX timestamp (nanoseconds) when the data event occurred.
        ts_init: int64
            The UNIX timestamp (nanoseconds) when the data object was initialized.

        """
        super().__init__(ts_event, ts_init)

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
        int64_t ts_event,
        int64_t ts_init,
    ):
        """
        Initialize a new instance of the ``OrderBookSnapshot`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the book.
        level : BookLevel {``L1``, ``L2``, ``L3``}
            The order book level.
        bids : list
            The bids for the snapshot.
        asks : list
            The asks for the snapshot.
        ts_event: int64
            The UNIX timestamp (nanoseconds) when the data event occurred.
        ts_init: int64
            The UNIX timestamp (nanoseconds) when the data object was initialized.

        """
        super().__init__(instrument_id, level, ts_event, ts_init)

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
                f"ts_init={self.ts_init})")

    @staticmethod
    cdef OrderBookSnapshot from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderBookSnapshot(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            level=BookLevelParser.from_str(values["level"]),
            bids=orjson.loads(values["bids"]),
            asks=orjson.loads(values["asks"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
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
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        BookLevel level,
        list deltas not None,
        int64_t ts_event,
        int64_t ts_init,
    ):
        """
        Initialize a new instance of the ``OrderBookDeltas`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the book.
        level : BookLevel {``L1``, ``L2``, ``L3``}
            The order book level.
        deltas : list[OrderBookDelta]
            The list of order book changes.
        ts_event: int64
            The UNIX timestamp (nanoseconds) when the data event occurred.
        ts_init: int64
            The UNIX timestamp (nanoseconds) when the data object was initialized.

        """
        super().__init__(instrument_id, level, ts_event, ts_init)

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
                f"ts_init={self.ts_init})")

    @staticmethod
    cdef OrderBookDeltas from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderBookDeltas(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            level=BookLevelParser.from_str(values["level"]),
            deltas=[OrderBookDelta.from_dict_c(d) for d in orjson.loads(values["deltas"])],
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderBookDeltas obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderBookDeltas",
            "instrument_id": obj.instrument_id.value,
            "level": BookLevelParser.to_str(obj.level),
            "deltas": orjson.dumps([OrderBookDelta.to_dict_c(d) for d in obj.deltas]),
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
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        BookLevel level,
        BookAction action,
        Order order,
        int64_t ts_event,
        int64_t ts_init,
    ):
        """
        Initialize a new instance of the ``OrderBookDelta`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID.
        level : BookLevel {``L1``, ``L2``, ``L3``}
            The order book level.
        action : BookAction {``ADD``, ``UPDATED``, ``DELETE``, ``CLEAR``}
            The order book delta action.
        order : Order
            The order to apply.
        ts_event: int64
            The UNIX timestamp (nanoseconds) when the data event occurred.
        ts_init: int64
            The UNIX timestamp (nanoseconds) when the data object was initialized.

        """
        super().__init__(instrument_id, level, ts_event, ts_init)

        self.action = action
        self.order = order

    def __eq__(self, OrderBookDelta other) -> bool:
        return OrderBookDelta.to_dict_c(self) == OrderBookDelta.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(OrderBookDelta.to_dict_c(self)))

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"'{self.instrument_id}', "
                f"level={BookLevelParser.to_str(self.level)}, "
                f"action={BookActionParser.to_str(self.action)}, "
                f"order={self.order}, "
                f"ts_init={self.ts_init})")

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
            level=BookLevelParser.from_str(values["level"]),
            action=BookActionParser.from_str(values["action"]),
            order=order,
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderBookDelta obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderBookDelta",
            "instrument_id": obj.instrument_id.value,
            "level": BookLevelParser.to_str(obj.level),
            "action": BookActionParser.to_str(obj.action),
            "order_price": obj.order.price if obj.order else None,
            "order_size": obj.order.size if obj.order else None,
            "order_side": OrderSideParser.to_str(obj.order.side) if obj.order else None,
            "order_id": obj.order.id if obj.order else None,
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
