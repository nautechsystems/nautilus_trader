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

from libc.stdint cimport uint64_t

import json

from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.bracket cimport BracketOrder
from nautilus_trader.model.orders.unpacker cimport OrderUnpacker


cdef class TradingCommand(Command):
    """
    The abstract base class for all trading related commands.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        UUID command_id not None,
        uint64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``TradingCommand`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier for the command.
        strategy_id : StrategyId
            The strategy identifier for the command.
        instrument_id : InstrumentId
            The instrument identifier for the command.
        command_id : UUID
            The commands identifier.
        timestamp_ns : uint64
            The UNIX timestamp (nanoseconds) of the command.

        """
        super().__init__(command_id, timestamp_ns)

        self.trader_id = trader_id
        self.strategy_id = strategy_id
        self.instrument_id = instrument_id


cdef class SubmitOrder(TradingCommand):
    """
    Represents a command to submit the given order.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_D_68.html

    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        PositionId position_id not None,
        Order order not None,
        UUID command_id not None,
        uint64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``SubmitOrder`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier for the command.
        strategy_id : StrategyId
            The strategy identifier for the command.
        position_id : PositionId
            The position identifier for the command (can be NULL).
        order : Order
            The order to submit.
        command_id : UUID
            The commands identifier.
        timestamp_ns : uint64
            The UNIX timestamp (nanoseconds) of the command.

        """
        super().__init__(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=order.instrument_id,
            command_id=command_id,
            timestamp_ns=timestamp_ns,
        )

        self.position_id = position_id
        self.order = order

    def __repr__(self) -> str:
        cdef str position_id_str = '' if self.position_id.is_null() else f"position_id={self.position_id.value}, "
        return (f"{type(self).__name__}("
                f"{self.order.status_string_c()}, "
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.order.client_order_id.value}, "
                f"{position_id_str}"
                f"strategy_id={self.strategy_id.value}, "
                f"command_id={self.id})")

    @staticmethod
    cdef SubmitOrder from_dict_c(dict values):
        cdef str p = values["position_id"]
        cdef PositionId position_id = PositionId(p) if p is not None else None
        return SubmitOrder(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            position_id=position_id,
            order=OrderUnpacker.unpack_c(json.loads(values["order"])),
            command_id=UUID.from_str_c(values["command_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    def from_dict(dict values):
        """
        Return a submit order command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        SubmitOrder

        """
        return SubmitOrder.from_dict_c(values)

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return {
            "type": type(self).__name__,
            "trader_id": self.trader_id.value,
            "strategy_id": self.strategy_id.value,
            "position_id": self.position_id.value if self.position_id is not None else None,
            "order": json.dumps(self.order.init_event_c().to_dict()),
            "command_id": self.id.value,
            "timestamp_ns": self.timestamp_ns,
        }


cdef class SubmitBracketOrder(TradingCommand):
    """
    Represents a command to submit a bracket order consisting of parent and child orders.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_E_69.html

    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        BracketOrder bracket_order not None,
        UUID command_id not None,
        uint64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``SubmitBracketOrder`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier for the command.
        strategy_id : StrategyId
            The strategy identifier for the command.
        bracket_order : BracketOrder
            The bracket order to submit.
        command_id : UUID
            The command identifier.
        timestamp_ns : uint64
            The UNIX timestamp (nanoseconds) of the command.

        """
        super().__init__(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=bracket_order.instrument_id,
            command_id=command_id,
            timestamp_ns=timestamp_ns,
        )

        self.bracket_order = bracket_order

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_link_id={self.bracket_order.id.value}, "
                f"command_id={self.id})")

    @staticmethod
    cdef SubmitBracketOrder from_dict_c(dict values):
        cdef BracketOrder bracket_order = BracketOrder(
            entry=OrderUnpacker.unpack_c(json.loads(values["entry"])),
            stop_loss=OrderUnpacker.unpack_c(json.loads(values["stop_loss"])),
            take_profit=OrderUnpacker.unpack_c(json.loads(values["take_profit"])),
        )
        return SubmitBracketOrder(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            bracket_order=bracket_order,
            command_id=UUID.from_str_c(values["command_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    def from_dict(dict values):
        """
        Return a submit bracket order command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        SubmitBracketOrder

        """
        return SubmitOrder.from_dict_c(values)

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return {
            "type": type(self).__name__,
            "trader_id": self.trader_id.value,
            "strategy_id": self.strategy_id.value,
            "entry": json.dumps(self.bracket_order.entry.init_event_c().to_dict()),
            "stop_loss": json.dumps(self.bracket_order.stop_loss.init_event_c().to_dict()),
            "take_profit": json.dumps(self.bracket_order.take_profit.init_event_c().to_dict()),
            "command_id": self.id.value,
            "timestamp_ns": self.timestamp_ns,
        }


cdef class UpdateOrder(TradingCommand):
    """
    Represents a command to change to parameters of an existing order.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_G_71.html

    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        Quantity quantity,  # Can be None
        Price price,  # Can be None
        Price trigger,  # Can be None
        UUID command_id not None,
        uint64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``UpdateOrder`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier for the command.
        strategy_id : StrategyId
            The strategy identifier for the command.
        instrument_id : InstrumentId
            The instrument identifier for the command.
        client_order_id : VenueOrderId
            The client order identifier to update.
        venue_order_id : VenueOrderId
            The venue order identifier to update.
        quantity : Quantity, optional
            The quantity for the order update.
        price : Price, optional
            The price for the order update.
        trigger : Price, optional
            The trigger price for the order update.
        command_id : UUID
            The command identifier.
        timestamp_ns : uint64
            The UNIX timestamp (nanoseconds) of the command.

        """
        super().__init__(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            command_id=command_id,
            timestamp_ns=timestamp_ns,
        )

        self.client_order_id = client_order_id
        self.venue_order_id = venue_order_id
        self.quantity = quantity
        self.price = price
        self.trigger = trigger

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"quantity={self.quantity.to_str()}, "
                f"price={self.price}, "
                f"trigger={self.trigger}, "
                f"command_id={self.id})")

    @staticmethod
    cdef UpdateOrder from_dict_c(dict values):
        cdef str q = values["quantity"]
        cdef str p = values["price"]
        cdef str t = values["trigger"]
        return UpdateOrder(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            quantity=Quantity.from_str_c(q) if q is not None else None,
            price=Price.from_str_c(p) if p is not None else None,
            trigger=Price.from_str_c(t) if t is not None else None,
            command_id=UUID.from_str_c(values["command_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    def from_dict(dict values):
        """
        Return an update order command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        UpdateOrder

        """
        return SubmitOrder.from_dict_c(values)

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return {
            "type": type(self).__name__,
            "trader_id": self.trader_id.value,
            "strategy_id": self.strategy_id.value,
            "instrument_id": self.instrument_id.value,
            "client_order_id": self.client_order_id.value,
            "venue_order_id": self.venue_order_id.value,
            "quantity": str(self.quantity) if self.quantity is not None else None,
            "price": str(self.price) if self.price is not None else None,
            "trigger": str(self.trigger) if self.trigger is not None else None,
            "command_id": self.id.value,
            "timestamp_ns": self.timestamp_ns,
        }


cdef class CancelOrder(TradingCommand):
    """
    Represents a command to cancel an order.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_F_70.html

    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        UUID command_id not None,
        uint64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``CancelOrder`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier for the command.
        strategy_id : StrategyId
            The strategy identifier for the command.
        instrument_id : InstrumentId
            The instrument identifier for the command.
        client_order_id : ClientOrderId
            The client order identifier to cancel.
        venue_order_id : VenueOrderId
            The venue order identifier to cancel.
        command_id : UUID
            The command identifier.
        timestamp_ns : uint64
            The UNIX timestamp (nanoseconds) of the command.

        """
        super().__init__(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            command_id=command_id,
            timestamp_ns=timestamp_ns,
        )

        self.client_order_id = client_order_id
        self.venue_order_id = venue_order_id

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"command_id={self.id})")

    @staticmethod
    cdef CancelOrder from_dict_c(dict values):
        return CancelOrder(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            command_id=UUID.from_str_c(values["command_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    def from_dict(dict values):
        """
        Return a cancel order command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        CancelOrder

        """
        return CancelOrder.from_dict_c(values)

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return {
            "type": type(self).__name__,
            "trader_id": self.trader_id.value,
            "strategy_id": self.strategy_id.value,
            "instrument_id": self.instrument_id.value,
            "client_order_id": self.client_order_id.value,
            "venue_order_id": self.venue_order_id.value,
            "command_id": self.id.value,
            "timestamp_ns": self.timestamp_ns,
        }
