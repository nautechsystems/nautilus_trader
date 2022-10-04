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

from typing import Optional

import msgspec

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.trigger_type cimport TriggerType
from nautilus_trader.model.c_enums.trigger_type cimport TriggerTypeParser
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.unpacker cimport OrderUnpacker


cdef class TradingCommand(Command):
    """
    The abstract base class for all trading related commands.

    Parameters
    ----------
    client_id : ClientId, optional with no default so ``None`` must be passed explicitly
        The execution client ID for the command.
    trader_id : TraderId
        The trader ID for the command.
    strategy_id : StrategyId
        The strategy ID for the command.
    instrument_id : InstrumentId
        The instrument ID for the command.
    command_id : UUID4
        The commands ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        ClientId client_id: Optional[ClientId],
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        UUID4 command_id not None,
        uint64_t ts_init,
    ):
        super().__init__(command_id, ts_init)

        self.client_id = client_id
        self.trader_id = trader_id
        self.strategy_id = strategy_id
        self.instrument_id = instrument_id


cdef class SubmitOrder(TradingCommand):
    """
    Represents a command to submit the given order.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the command.
    strategy_id : StrategyId
        The strategy ID for the command.
    order : Order
        The order to submit.
    command_id : UUID4
        The commands ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    position_id : PositionId, optional
        The position ID for the command.
    emulation_trigger : TriggerType, default ``NONE``
        The trigger type for order emulation (if ``NONE`` then no emulation).
    execution_algorithm : str, optional
        The name of the execution algorithm for the order.
    client_id : ClientId, optional
        The execution client ID for the command.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_D_68.html
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        Order order not None,
        UUID4 command_id not None,
        uint64_t ts_init,
        PositionId position_id: Optional[PositionId] = None,
        TriggerType emulation_trigger = TriggerType.NONE,
        str execution_algorithm = None,
        ClientId client_id = None,
    ):
        super().__init__(
            client_id=client_id,
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=order.instrument_id,
            command_id=command_id,
            ts_init=ts_init,
        )

        self.order = order
        self.position_id = position_id
        self.emulation_trigger = emulation_trigger
        self.execution_algorithm = execution_algorithm

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.order.client_order_id.to_str()}, "
            f"order={self.order.info()}, "
            f"position_id={self.position_id}, "
            f"emulation_trigger={TriggerTypeParser.to_str(self.emulation_trigger)}, "
            f"execution_algorithm={self.execution_algorithm})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "  # Can be None
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.order.client_order_id.to_str()}, "
            f"order={self.order.info()}, "
            f"position_id={self.position_id}, "
            f"emulation_trigger={TriggerTypeParser.to_str(self.emulation_trigger)}, "
            f"execution_algorithm={self.execution_algorithm}, "
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef SubmitOrder from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str c = values["client_id"]
        cdef str p = values["position_id"]
        return SubmitOrder(
            client_id=ClientId(c) if c is not None else None,
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            order=OrderUnpacker.unpack_c(msgspec.json.decode(values["order"])),
            position_id=PositionId(p) if p is not None else None,
            emulation_trigger=TriggerTypeParser.from_str(values["emulation_trigger"]),
            execution_algorithm=values["execution_algorithm"],
            command_id=UUID4(values["command_id"]),
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(SubmitOrder obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "SubmitOrder",
            "client_id": obj.client_id.to_str() if obj.client_id is not None else None,
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "order": msgspec.json.encode(OrderInitialized.to_dict_c(obj.order.init_event_c())),
            "position_id": obj.position_id.to_str() if obj.position_id is not None else None,
            "emulation_trigger": TriggerTypeParser.to_str(obj.emulation_trigger),
            "execution_algorithm": obj.execution_algorithm,
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> SubmitOrder:
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

    @staticmethod
    def to_dict(SubmitOrder obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return SubmitOrder.to_dict_c(obj)


cdef class SubmitOrderList(TradingCommand):
    """
    Represents a command to submit an order list consisting of bulk or related
    parent-child contingent orders.

    This command can correspond to a `NewOrderList <E> message` for the FIX
    protocol.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the command.
    strategy_id : StrategyId
        The strategy ID for the command.
    order_list : OrderList
        The order list to submit.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    client_id : ClientId, optional
        The execution client ID for the command.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_E_69.html
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        OrderList order_list not None,
        UUID4 command_id not None,
        uint64_t ts_init,
        ClientId client_id = None,
    ):
        super().__init__(
            client_id=client_id,
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=order_list.instrument_id,
            command_id=command_id,
            ts_init=ts_init,
        )

        self.list = order_list

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"order_list={self.list})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "  # Can be None
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"order_list={self.list}, "
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef SubmitOrderList from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str c = values["client_id"]
        cdef dict o_dict
        cdef OrderList order_list = OrderList(
            list_id=OrderListId(values["order_list_id"]),
            orders=[OrderUnpacker.unpack_c(o_dict) for o_dict in msgspec.json.decode(values["orders"])],
        )
        return SubmitOrderList(
            client_id=ClientId(c) if c is not None else None,
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            order_list=order_list,
            command_id=UUID4(values["command_id"]),
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(SubmitOrderList obj):
        Condition.not_none(obj, "obj")
        cdef Order o
        return {
            "type": "SubmitOrderList",
            "client_id": obj.client_id.to_str() if obj.client_id is not None else None,
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "order_list_id": str(obj.list.id),
            "orders": msgspec.json.encode([OrderInitialized.to_dict_c(o.init_event_c()) for o in obj.list.orders]),
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> SubmitOrderList:
        """
        Return a submit order list command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        SubmitOrderList

        """
        return SubmitOrderList.from_dict_c(values)

    @staticmethod
    def to_dict(SubmitOrderList obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return SubmitOrderList.to_dict_c(obj)


cdef class ModifyOrder(TradingCommand):
    """
    Represents a command to modify the properties of an existing order.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the command.
    strategy_id : StrategyId
        The strategy ID for the command.
    instrument_id : InstrumentId
        The instrument ID for the command.
    client_order_id : VenueOrderId
        The client order ID to update.
    venue_order_id : VenueOrderId, optional with no default so ``None`` must be passed explicitly
        The venue order ID (assigned by the venue) to update.
    quantity : Quantity, optional with no default so ``None`` must be passed explicitly
        The quantity for the order update.
    price : Price, optional with no default so ``None`` must be passed explicitly
        The price for the order update.
    trigger_price : Price, optional with no default so ``None`` must be passed explicitly
        The trigger price for the order update.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    client_id : ClientId, optional
        The execution client ID for the command.

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
        VenueOrderId venue_order_id: Optional[VenueOrderId],
        Quantity quantity: Optional[Quantity],
        Price price: Optional[Price],
        Price trigger_price: Optional[Price],
        UUID4 command_id not None,
        uint64_t ts_init,
        ClientId client_id = None,
    ):
        super().__init__(
            client_id=client_id,
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            command_id=command_id,
            ts_init=ts_init,
        )

        self.client_order_id = client_order_id
        self.venue_order_id = venue_order_id
        self.quantity = quantity
        self.price = price
        self.trigger_price = trigger_price

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"quantity={self.quantity.to_str()}, "
            f"price={self.price}, "
            f"trigger_price={self.trigger_price})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "  # Can be None
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"quantity={self.quantity.to_str()}, "
            f"price={self.price}, "
            f"trigger_price={self.trigger_price}, "
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef ModifyOrder from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str c = values["client_id"]
        cdef str v = values["venue_order_id"]
        cdef str q = values["quantity"]
        cdef str p = values["price"]
        cdef str t = values["trigger_price"]
        return ModifyOrder(
            client_id=ClientId(c) if c is not None else None,
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(v) if v is not None else None,
            quantity=Quantity.from_str_c(q) if q is not None else None,
            price=Price.from_str_c(p) if p is not None else None,
            trigger_price=Price.from_str_c(t) if t is not None else None,
            command_id=UUID4(values["command_id"]),
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(ModifyOrder obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "ModifyOrder",
            "client_id": obj.client_id.to_str() if obj.client_id is not None else None,
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "venue_order_id": obj.venue_order_id.to_str() if obj.venue_order_id is not None else None,
            "quantity": str(obj.quantity) if obj.quantity is not None else None,
            "price": str(obj.price) if obj.price is not None else None,
            "trigger_price": str(obj.trigger_price) if obj.trigger_price is not None else None,
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> ModifyOrder:
        """
        Return a modify order command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        ModifyOrder

        """
        return ModifyOrder.from_dict_c(values)

    @staticmethod
    def to_dict(ModifyOrder obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return ModifyOrder.to_dict_c(obj)


cdef class CancelOrder(TradingCommand):
    """
    Represents a command to cancel an order.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the command.
    strategy_id : StrategyId
        The strategy ID for the command.
    instrument_id : InstrumentId
        The instrument ID for the command.
    client_order_id : ClientOrderId
        The client order ID to cancel.
    venue_order_id : VenueOrderId, optional with no default so ``None`` must be passed explicitly
        The venue order ID (assigned by the venue) to cancel.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    client_id : ClientId, optional
        The execution client ID for the command.

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
        VenueOrderId venue_order_id: Optional[VenueOrderId],
        UUID4 command_id not None,
        uint64_t ts_init,
        ClientId client_id = None,
    ):
        if client_id is None:
            client_id = ClientId(instrument_id.venue.to_str())
        super().__init__(
            client_id=client_id,
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            command_id=command_id,
            ts_init=ts_init,
        )

        self.client_order_id = client_order_id
        self.venue_order_id = venue_order_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id})"  # Can be None
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "  # Can be None
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef CancelOrder from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str c = values["client_id"]
        cdef str v = values["venue_order_id"]
        return CancelOrder(
            client_id=ClientId(c) if c is not None else None,
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(v) if v is not None else None,
            command_id=UUID4(values["command_id"]),
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(CancelOrder obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "CancelOrder",
            "client_id": obj.client_id.to_str() if obj.client_id is not None else None,
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "venue_order_id": obj.venue_order_id.to_str() if obj.venue_order_id is not None else None,
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> CancelOrder:
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

    @staticmethod
    def to_dict(CancelOrder obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return CancelOrder.to_dict_c(obj)


cdef class CancelAllOrders(TradingCommand):
    """
    Represents a command to cancel all orders for an instrument.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the command.
    strategy_id : StrategyId
        The strategy ID for the command.
    instrument_id : InstrumentId
        The instrument ID for the command.
    order_side : OrderSide
        The order side for the command.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    client_id : ClientId, optional
        The execution client ID for the command.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        OrderSide order_side,
        UUID4 command_id not None,
        uint64_t ts_init,
        ClientId client_id = None,
    ):
        super().__init__(
            client_id=client_id,
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            command_id=command_id,
            ts_init=ts_init,
        )

        self.order_side = order_side

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"order_side={OrderSideParser.to_str(self.order_side)})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "  # Can be None
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"order_side={OrderSideParser.to_str(self.order_side)}, "
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef CancelAllOrders from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str c = values["client_id"]
        return CancelAllOrders(
            client_id=ClientId(c) if c is not None else None,
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            order_side=OrderSideParser.from_str(values["order_side"]),
            command_id=UUID4(values["command_id"]),
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(CancelAllOrders obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "CancelAllOrders",
            "client_id": obj.client_id.to_str() if obj.client_id is not None else None,
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "order_side": OrderSideParser.to_str(obj.order_side),
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> CancelAllOrders:
        """
        Return a cancel order command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        CancelAllOrders

        """
        return CancelAllOrders.from_dict_c(values)

    @staticmethod
    def to_dict(CancelAllOrders obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return CancelAllOrders.to_dict_c(obj)


cdef class QueryOrder(TradingCommand):
    """
    Represents a command to query an order.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the command.
    strategy_id : StrategyId
        The strategy ID for the command.
    instrument_id : InstrumentId
        The instrument ID for the command.
    client_order_id : ClientOrderId
        The client order ID to cancel.
    venue_order_id : VenueOrderId, optional with no default so ``None`` must be passed explicitly
        The venue order ID (assigned by the venue) to cancel.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    client_id : ClientId, optional
        The execution client ID for the command.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id: Optional[VenueOrderId],
        UUID4 command_id not None,
        uint64_t ts_init,
        ClientId client_id = None,
    ):
        if client_id is None:
            client_id = ClientId(instrument_id.venue.to_str())
        super().__init__(
            client_id=client_id,
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            command_id=command_id,
            ts_init=ts_init,
        )

        self.client_order_id = client_order_id
        self.venue_order_id = venue_order_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id})"  # Can be None
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef QueryOrder from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str c = values["client_id"]
        cdef str v = values["venue_order_id"]
        return QueryOrder(
            client_id=ClientId(c) if c is not None else None,
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(v) if v is not None else None,
            command_id=UUID4(values["command_id"]),
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(QueryOrder obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "QueryOrder",
            "client_id": obj.client_id.to_str() if obj.client_id is not None else None,
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "venue_order_id": obj.venue_order_id.to_str() if obj.venue_order_id is not None else None,
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> QueryOrder:
        """
        Return a query order command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        QueryOrder

        """
        return QueryOrder.from_dict_c(values)

    @staticmethod
    def to_dict(QueryOrder obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return QueryOrder.to_dict_c(obj)
