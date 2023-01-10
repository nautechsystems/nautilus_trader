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

import json
from typing import Optional

import msgspec

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.enums_c cimport ContingencyType
from nautilus_trader.model.enums_c cimport LiquiditySide
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.enums_c cimport OrderType
from nautilus_trader.model.enums_c cimport TimeInForce
from nautilus_trader.model.enums_c cimport TriggerType
from nautilus_trader.model.enums_c cimport contingency_type_from_str
from nautilus_trader.model.enums_c cimport contingency_type_to_str
from nautilus_trader.model.enums_c cimport liquidity_side_from_str
from nautilus_trader.model.enums_c cimport liquidity_side_to_str
from nautilus_trader.model.enums_c cimport order_side_from_str
from nautilus_trader.model.enums_c cimport order_side_to_str
from nautilus_trader.model.enums_c cimport order_type_from_str
from nautilus_trader.model.enums_c cimport order_type_to_str
from nautilus_trader.model.enums_c cimport time_in_force_from_str
from nautilus_trader.model.enums_c cimport time_in_force_to_str
from nautilus_trader.model.enums_c cimport trigger_type_from_str
from nautilus_trader.model.enums_c cimport trigger_type_to_str
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class OrderEvent(Event):
    """
    The base class for all order events.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId, optional with no default so ``None`` must be passed explicitly
        The venue order ID (assigned by the venue).
    account_id : AccountId, optional with no default so ``None`` must be passed explicitly
        The account ID (with the venue).
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the order event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool
        If the event was generated during reconciliation.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id: Optional[VenueOrderId],
        AccountId account_id: Optional[AccountId],
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
        bint reconciliation,
    ):
        super().__init__(event_id, ts_event, ts_init)

        self.trader_id = trader_id
        self.strategy_id = strategy_id
        self.instrument_id = instrument_id
        self.client_order_id = client_order_id
        self.venue_order_id = venue_order_id
        self.account_id = account_id
        self.reconciliation = reconciliation


cdef class OrderInitialized(OrderEvent):
    """
    Represents an event where an order has been initialized.

    This is a seed event which can instantiate any order through a creation
    method. This event should contain enough information to be able to send it
    'over the wire' and have a valid order created with exactly the same
    properties as if it had been instantiated locally.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    order_side : OrderSide {``BUY``, ``SELL``}
        The order side.
    order_type : OrderType
        The order type.
    quantity : Quantity
        The order quantity.
    time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``, ``AT_THE_OPEN``, ``AT_THE_CLOSE``}
        The order time in force.
    post_only : bool
        If the order will only provide liquidity (make a market).
    reduce_only : bool
        If the order carries the 'reduce-only' execution instruction.
    options : dict[str, str]
        The order initialization options. Contains mappings for specific
        order parameters.
    emulation_trigger : EmulationTrigger
        The emulation trigger for the order.
    contingency_type : ContingencyType
        The order contingency type.
    order_list_id : OrderListId, optional with no default so ``None`` must be passed explicitly
        The order list ID associated with the order.
    linked_order_ids : list[ClientOrderId], optional with no default so ``None`` must be passed explicitly
        The order linked client order ID(s).
    parent_order_id : ClientOrderId, optional with no default so ``None`` must be passed explicitly
        The orders parent client order ID.
    tags : str, optional with no default so ``None`` must be passed explicitly
        The custom user tags for the order. These are optional and can
        contain any arbitrary delimiter if required.
    event_id : UUID4
        The event ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.

    Raises
    ------
    ValueError
        If `order_side` is ``NO_ORDER_SIDE``.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        OrderSide order_side,
        OrderType order_type,
        Quantity quantity not None,
        TimeInForce time_in_force,
        bint post_only,
        bint reduce_only,
        dict options not None,
        TriggerType emulation_trigger,
        ContingencyType contingency_type,
        OrderListId order_list_id: Optional[OrderListId],
        list linked_order_ids: Optional[list[ClientOrderId]],
        ClientOrderId parent_order_id: Optional[ClientOrderId],
        str tags: Optional[str],
        UUID4 event_id not None,
        uint64_t ts_init,
        bint reconciliation=False,
    ):
        Condition.not_equal(order_side, OrderSide.NO_ORDER_SIDE, "order_side", "NONE")

        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            None,  # Pending assignment by venue
            None,  # Pending assignment by system
            event_id,
            ts_init,  # Timestamp identical to ts_init
            ts_init,
            reconciliation,
        )

        self.side = order_side
        self.order_type = order_type
        self.quantity = quantity
        self.time_in_force = time_in_force
        self.post_only = post_only
        self.reduce_only = reduce_only
        self.options = options
        self.emulation_trigger = emulation_trigger
        self.contingency_type = contingency_type
        self.order_list_id = order_list_id
        self.linked_order_ids = linked_order_ids
        self.parent_order_id = parent_order_id
        self.tags = tags

    def __str__(self) -> str:
        cdef ClientOrderId o
        cdef str linked_order_ids = "None"
        if self.linked_order_ids:
            linked_order_ids = str([o.to_str() for o in self.linked_order_ids])
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id}, "
            f"side={order_side_to_str(self.side)}, "
            f"type={order_type_to_str(self.order_type)}, "
            f"quantity={self.quantity.to_str()}, "
            f"time_in_force={time_in_force_to_str(self.time_in_force)}, "
            f"post_only={self.post_only}, "
            f"reduce_only={self.reduce_only}, "
            f"options={self.options}, "
            f"emulation_trigger={trigger_type_to_str(self.emulation_trigger)}, "
            f"contingency_type={contingency_type_to_str(self.contingency_type)}, "
            f"order_list_id={self.order_list_id}, "  # Can be None
            f"linked_order_ids={linked_order_ids}, "
            f"parent_order_id={self.parent_order_id}, "
            f"tags={self.tags})"
        )

    def __repr__(self) -> str:
        cdef ClientOrderId o
        cdef str linked_order_ids = "None"
        if self.linked_order_ids:
            linked_order_ids = str([o.to_str() for o in self.linked_order_ids])
        return (
            f"{type(self).__name__}("
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"side={order_side_to_str(self.side)}, "
            f"type={order_type_to_str(self.order_type)}, "
            f"quantity={self.quantity.to_str()}, "
            f"time_in_force={time_in_force_to_str(self.time_in_force)}, "
            f"post_only={self.post_only}, "
            f"reduce_only={self.reduce_only}, "
            f"options={self.options}, "
            f"emulation_trigger={trigger_type_to_str(self.emulation_trigger)}, "
            f"contingency_type={contingency_type_to_str(self.contingency_type)}, "
            f"order_list_id={self.order_list_id}, "  # Can be None
            f"linked_order_ids={linked_order_ids}, "
            f"parent_order_id={self.parent_order_id}, "
            f"tags={self.tags}, "
            f"event_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderInitialized from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str order_list_id_str = values["order_list_id"]
        cdef str linked_order_ids_str = values["linked_order_ids"]
        cdef str parent_order_id_str = values["parent_order_id"]
        return OrderInitialized(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            order_side=order_side_from_str(values["order_side"]),
            order_type=order_type_from_str(values["order_type"]),
            quantity=Quantity.from_str_c(values["quantity"]),
            time_in_force=time_in_force_from_str(values["time_in_force"]),
            post_only=values["post_only"],
            reduce_only=values["reduce_only"],
            options=json.loads(values["options"]),  # Using vanilla json due mixed schema types
            emulation_trigger=trigger_type_from_str(values["emulation_trigger"]),
            contingency_type=contingency_type_from_str(values["contingency_type"]),
            order_list_id=OrderListId(order_list_id_str) if order_list_id_str else None,
            linked_order_ids=[ClientOrderId(o_str) for o_str in linked_order_ids_str.split(",")] if linked_order_ids_str is not None else None,
            parent_order_id=ClientOrderId(parent_order_id_str) if parent_order_id_str else None,
            tags=values["tags"],
            event_id=UUID4(values["event_id"]),
            ts_init=values["ts_init"],
            reconciliation=values.get("reconciliation", False),
        )

    @staticmethod
    cdef dict to_dict_c(OrderInitialized obj):
        Condition.not_none(obj, "obj")
        cdef ClientOrderId o
        return {
            "type": "OrderInitialized",
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "order_side": order_side_to_str(obj.side),
            "order_type": order_type_to_str(obj.order_type),
            "quantity": str(obj.quantity),
            "time_in_force": time_in_force_to_str(obj.time_in_force),
            "post_only": obj.post_only,
            "reduce_only": obj.reduce_only,
            "options": json.dumps(obj.options),  # Using vanilla json due mixed schema types
            "emulation_trigger": trigger_type_to_str(obj.emulation_trigger),
            "contingency_type": contingency_type_to_str(obj.contingency_type),
            "order_list_id": obj.order_list_id.to_str() if obj.order_list_id is not None else None,
            "linked_order_ids": ",".join([o.to_str() for o in obj.linked_order_ids]) if obj.linked_order_ids is not None else None,  # noqa
            "parent_order_id": obj.parent_order_id.to_str() if obj.parent_order_id is not None else None,
            "tags": obj.tags,
            "event_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
            "reconciliation": obj.reconciliation,
        }

    @staticmethod
    def from_dict(dict values) -> OrderInitialized:
        """
        Return an order initialized event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderInitialized

        """
        return OrderInitialized.from_dict_c(values)

    @staticmethod
    def to_dict(OrderInitialized obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderInitialized.to_dict_c(obj)


cdef class OrderDenied(OrderEvent):
    """
    Represents an event where an order has been denied by the Nautilus system.

    This could be due an unsupported feature, a risk limit exceedance, or for
    any other reason that an otherwise valid order is not able to be submitted.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    reason : str
        The order denied reason.
    event_id : UUID4
        The event ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.

    Raises
    ------
    ValueError
        If `denied_reason` is not a valid_string.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        str reason not None,
        UUID4 event_id not None,
        uint64_t ts_init,
    ):
        Condition.valid_string(reason, "denied_reason")
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            None,  # Never assigned
            None,  # Never assigned
            event_id,
            ts_init,  # Timestamp identical to ts_init
            ts_init,
            reconciliation=False,  # Internal system event
        )

        self.reason = reason

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"reason={self.reason})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"reason={self.reason}, "
            f"event_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderDenied from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderDenied(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            reason=values["reason"],
            event_id=UUID4(values["event_id"]),
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderDenied obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderDenied",
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "reason": obj.reason,
            "event_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> OrderDenied:
        """
        Return an order denied event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderDenied

        """
        return OrderDenied.from_dict_c(values)

    @staticmethod
    def to_dict(OrderDenied obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderDenied.to_dict_c(obj)


cdef class OrderSubmitted(OrderEvent):
    """
    Represents an event where an order has been submitted by the system to the
    trading venue.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    account_id : AccountId
        The account ID (with the venue).
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the order submitted event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        AccountId account_id not None,
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            None,  # Pending accepted
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation=False,  # Internal system event
        )

        self.account_id = account_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"account_id={self.account_id.to_str()}, "
            f"ts_event={self.ts_event})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"account_id={self.account_id.to_str()}, "
            f"event_id={self.id.to_str()}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderSubmitted from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderSubmitted(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            account_id=AccountId(values["account_id"]),
            event_id=UUID4(values["event_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderSubmitted obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderSubmitted",
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "account_id": obj.account_id.to_str(),
            "event_id": obj.id.to_str(),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> OrderSubmitted:
        """
        Return an order submitted event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderSubmitted

        """
        return OrderSubmitted.from_dict_c(values)

    @staticmethod
    def to_dict(OrderSubmitted obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderSubmitted.to_dict_c(obj)


cdef class OrderAccepted(OrderEvent):
    """
    Represents an event where an order has been accepted by the trading venue.

    This event often corresponds to a `NEW` OrdStatus <39> field in FIX
    trade reports.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId
        The venue order ID (assigned by the venue).
    account_id : AccountId
        The account ID (with the venue).
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the order accepted event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/tagNum_39.html
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        AccountId account_id not None,
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
        bint reconciliation=False,
    ):
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
        )

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id.to_str()}, "
            f"account_id={self.account_id.to_str()}, "
            f"ts_event={self.ts_event})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id.to_str()}, "
            f"account_id={self.account_id.to_str()}, "
            f"event_id={self.id.to_str()}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderAccepted from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderAccepted(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            account_id=AccountId(values["account_id"]),
            event_id=UUID4(values["event_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            reconciliation=values.get("reconciliation", False),
        )

    @staticmethod
    cdef dict to_dict_c(OrderAccepted obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderAccepted",
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "venue_order_id": obj.venue_order_id.to_str(),
            "account_id": obj.account_id.to_str(),
            "event_id": obj.id.to_str(),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "reconciliation": obj.reconciliation,
        }

    @staticmethod
    def from_dict(dict values) -> OrderAccepted:
        """
        Return an order accepted event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderAccepted

        """
        return OrderAccepted.from_dict_c(values)

    @staticmethod
    def to_dict(OrderAccepted obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderAccepted.to_dict_c(obj)


cdef class OrderRejected(OrderEvent):
    """
    Represents an event where an order has been rejected by the trading venue.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    account_id : AccountId
        The account ID (with the venue).
    reason : datetime
        The order rejected reason.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the order rejected event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.

    Raises
    ------
    ValueError
        If `reason` is not a valid string.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        AccountId account_id not None,
        str reason not None,
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
        bint reconciliation=False,
    ):
        Condition.valid_string(reason, "reason")
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            None,  # Not always assigned on rejection
            account_id,  # We know the account with the venue
            event_id,
            ts_event,
            ts_init,
            reconciliation,
        )

        self.reason = reason

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"account_id={self.account_id.to_str()}, "
            f"reason='{self.reason}', "
            f"ts_event={self.ts_event})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"account_id={self.account_id.to_str()}, "
            f"reason='{self.reason}', "
            f"event_id={self.id.to_str()}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderRejected from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderRejected(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            account_id=AccountId(values["account_id"]),
            reason=values["reason"],
            event_id=UUID4(values["event_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            reconciliation=values.get("reconciliation", False),
        )

    @staticmethod
    cdef dict to_dict_c(OrderRejected obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderRejected",
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "account_id": obj.account_id.to_str(),
            "reason": obj.reason,
            "event_id": obj.id.to_str(),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "reconciliation": obj.reconciliation,
        }

    @staticmethod
    def from_dict(dict values) -> OrderRejected:
        """
        Return an order rejected event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderRejected

        """
        return OrderRejected.from_dict_c(values)

    @staticmethod
    def to_dict(OrderRejected obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderRejected.to_dict_c(obj)


cdef class OrderCanceled(OrderEvent):
    """
    Represents an event where an order has been canceled at the trading venue.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId, optional with no default so ``None`` must be passed explicitly
        The venue order ID (assigned by the venue).
    account_id : AccountId, optional with no default so ``None`` must be passed explicitly
        The account ID (with the venue).
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when order canceled event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id: Optional[VenueOrderId],
        AccountId account_id: Optional[AccountId],
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
        bint reconciliation=False,
    ):
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
        )

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"account_id={self.account_id}, "  # Can be None
            f"ts_event={self.ts_event})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"account_id={self.account_id}, "  # Can be None
            f"event_id={self.id.to_str()}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderCanceled from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str v = values["venue_order_id"]
        cdef str a = values["account_id"]
        return OrderCanceled(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(v) if v is not None else None,
            account_id=AccountId(a) if a is not None else None,
            event_id=UUID4(values["event_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            reconciliation=values.get("reconciliation", False),
        )

    @staticmethod
    cdef dict to_dict_c(OrderCanceled obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderCanceled",
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "venue_order_id": obj.venue_order_id.to_str() if obj.venue_order_id is not None else None,
            "account_id": obj.account_id.to_str() if obj.account_id is not None else None,
            "event_id": obj.id.to_str(),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "reconciliation": obj.reconciliation,
        }

    @staticmethod
    def from_dict(dict values) -> OrderCanceled:
        """
        Return an order canceled event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderCanceled

        """
        return OrderCanceled.from_dict_c(values)

    @staticmethod
    def to_dict(OrderCanceled obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderCanceled.to_dict_c(obj)


cdef class OrderExpired(OrderEvent):
    """
    Represents an event where an order has expired at the trading venue.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId, optional with no default so ``None`` must be passed explicitly
        The venue order ID (assigned by the venue).
    account_id : AccountId, optional with no default so ``None`` must be passed explicitly
        The account ID (with the venue).
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the order expired event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id: Optional[VenueOrderId],
        AccountId account_id: Optional[AccountId],
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
        bint reconciliation=False,
    ):
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
        )

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"account_id={self.account_id}, "  # Can be None
            f"ts_event={self.ts_event})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"account_id={self.account_id}, "  # Can be None
            f"event_id={self.id.to_str()}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderExpired from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str v = values["venue_order_id"]
        cdef str a = values["account_id"]
        return OrderExpired(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(v) if v is not None else None,
            account_id=AccountId(a) if a is not None else None,
            event_id=UUID4(values["event_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            reconciliation=values.get("reconciliation", False),
        )

    @staticmethod
    cdef dict to_dict_c(OrderExpired obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderExpired",
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "venue_order_id": obj.venue_order_id.to_str() if obj.venue_order_id is not None else None,
            "account_id": obj.account_id.to_str() if obj.account_id is not None else None,
            "event_id": obj.id.to_str(),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "reconciliation": obj.reconciliation,
        }

    @staticmethod
    def from_dict(dict values) -> OrderExpired:
        """
        Return an order expired event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderExpired

        """
        return OrderExpired.from_dict_c(values)

    @staticmethod
    def to_dict(OrderExpired obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderExpired.to_dict_c(obj)


cdef class OrderTriggered(OrderEvent):
    """
    Represents an event where an order has triggered.

    Applicable to :class:`StopLimit` orders only.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId, optional with no default so ``None`` must be passed explicitly
        The venue order ID (assigned by the venue).
    account_id : AccountId, optional with no default so ``None`` must be passed explicitly
        The account ID (with the venue).
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the order triggered event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id: Optional[VenueOrderId],
        AccountId account_id: Optional[AccountId],
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
        bint reconciliation=False,
    ):
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
        )

        self.account_id = account_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"account_id={self.account_id}, "  # Can be None
            f"ts_event={self.ts_event})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"account_id={self.account_id}, "  # Can be None
            f"event_id={self.id.to_str()}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderTriggered from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str v = values["venue_order_id"]
        cdef str a = values["account_id"]
        return OrderTriggered(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(v) if v is not None else None,
            account_id=AccountId(a) if a is not None else None,
            event_id=UUID4(values["event_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            reconciliation=values.get("reconciliation", False),
        )

    @staticmethod
    cdef dict to_dict_c(OrderTriggered obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderTriggered",
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "venue_order_id": obj.venue_order_id.to_str() if obj.venue_order_id is not None else None,
            "account_id": obj.account_id.to_str() if obj.account_id is not None else None,
            "event_id": obj.id.to_str(),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "reconciliation": obj.reconciliation,
        }

    @staticmethod
    def from_dict(dict values) -> OrderTriggered:
        """
        Return an order triggered event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderTriggered

        """
        return OrderTriggered.from_dict_c(values)

    @staticmethod
    def to_dict(OrderTriggered obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderTriggered.to_dict_c(obj)


cdef class OrderPendingUpdate(OrderEvent):
    """
    Represents an event where an `ModifyOrder` command has been sent to the
    trading venue.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId, optional with no default so ``None`` must be passed explicitly
        The venue order ID (assigned by the venue).
    account_id : AccountId
        The account ID (with the venue).
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the order pending update event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id: Optional[VenueOrderId],
        AccountId account_id not None,
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
        bint reconciliation=False,
    ):
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
        )

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"account_id={self.account_id.to_str()}, "
            f"ts_event={self.ts_event})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"account_id={self.account_id.to_str()}, "
            f"event_id={self.id.to_str()}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderPendingUpdate from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str v = values["venue_order_id"]
        return OrderPendingUpdate(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(v) if v is not None else None,
            account_id=AccountId(values["account_id"]),
            event_id=UUID4(values["event_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            reconciliation=values.get("reconciliation", False),
        )

    @staticmethod
    cdef dict to_dict_c(OrderPendingUpdate obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderPendingUpdate",
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "venue_order_id": obj.venue_order_id.to_str() if obj.venue_order_id is not None else None,
            "account_id": obj.account_id.to_str(),
            "event_id": obj.id.to_str(),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "reconciliation": obj.reconciliation,
        }

    @staticmethod
    def from_dict(dict values) -> OrderPendingUpdate:
        """
        Return an order pending update event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderPendingUpdate

        """
        return OrderPendingUpdate.from_dict_c(values)

    @staticmethod
    def to_dict(OrderPendingUpdate obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderPendingUpdate.to_dict_c(obj)


cdef class OrderPendingCancel(OrderEvent):
    """
    Represents an event where a `CancelOrder` command has been sent to the
    trading venue.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId, optional with no default so ``None`` must be passed explicitly
        The venue order ID (assigned by the venue).
    account_id : AccountId
        The account ID (with the venue).
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the order pending cancel event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id: Optional[VenueOrderId],
        AccountId account_id not None,
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
        bint reconciliation=False,
    ):
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
        )

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"account_id={self.account_id.to_str()}, "
            f"ts_event={self.ts_event})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"account_id={self.account_id.to_str()}, "
            f"event_id={self.id.to_str()}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderPendingCancel from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str v = values["venue_order_id"]
        return OrderPendingCancel(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(v) if v is not None else None,
            account_id=AccountId(values["account_id"]),
            event_id=UUID4(values["event_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            reconciliation=values.get("reconciliation", False),
        )

    @staticmethod
    cdef dict to_dict_c(OrderPendingCancel obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderPendingCancel",
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "venue_order_id": obj.venue_order_id.to_str() if obj.venue_order_id is not None else None,
            "account_id": obj.account_id.to_str(),
            "event_id": obj.id.to_str(),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "reconciliation": obj.reconciliation,
        }

    @staticmethod
    def from_dict(dict values) -> OrderPendingCancel:
        """
        Return an order pending cancel event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderPendingCancel

        """
        return OrderPendingCancel.from_dict_c(values)

    @staticmethod
    def to_dict(OrderPendingCancel obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderPendingCancel.to_dict_c(obj)


cdef class OrderModifyRejected(OrderEvent):
    """
    Represents an event where a `ModifyOrder` command has been rejected by the
    trading venue.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId, optional with no default so ``None`` must be passed explicitly
        The venue order ID (assigned by the venue).
    account_id : AccountId, optional with no default so ``None`` must be passed explicitly
        The account ID (with the venue).
    reason : str
        The order update rejected reason.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the order update rejected event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.

    Raises
    ------
    ValueError
        If `reason` is not a valid string.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id: Optional[VenueOrderId],
        AccountId account_id: Optional[AccountId],
        str reason not None,
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
        bint reconciliation=False,
    ):
        Condition.valid_string(reason, "reason")
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
        )

        self.reason = reason

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"account_id={self.account_id}, "  # Can be None
            f"reason={self.reason}, "
            f"ts_event={self.ts_event})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"account_id={self.account_id}, "  # Can be None
            f"reason={self.reason}, "
            f"event_id={self.id.to_str()}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderModifyRejected from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str v = values["venue_order_id"]
        cdef str a = values["account_id"]
        return OrderModifyRejected(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(v) if v is not None else None,
            account_id=AccountId(a) if a is not None else None,
            reason=values["reason"],
            event_id=UUID4(values["event_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            reconciliation=values.get("reconciliation", False),
        )

    @staticmethod
    cdef dict to_dict_c(OrderModifyRejected obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderModifyRejected",
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "venue_order_id": obj.venue_order_id.to_str() if obj.venue_order_id is not None else None,
            "account_id": obj.account_id.to_str() if obj.account_id is not None else None,
            "reason": obj.reason,
            "event_id": obj.id.to_str(),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "reconciliation": obj.reconciliation,
        }

    @staticmethod
    def from_dict(dict values) -> OrderModifyRejected:
        """
        Return an order update rejected event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderModifyRejected

        """
        return OrderModifyRejected.from_dict_c(values)

    @staticmethod
    def to_dict(OrderModifyRejected obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderModifyRejected.to_dict_c(obj)


cdef class OrderCancelRejected(OrderEvent):
    """
    Represents an event where a `CancelOrder` command has been rejected by the
    trading venue.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId, optional with no default so ``None`` must be passed explicitly
        The venue order ID (assigned by the venue).
    account_id : AccountId, optional with no default so ``None`` must be passed explicitly
        The account ID (with the venue).
    reason : str
        The order cancel rejected reason.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the order cancel rejected event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.

    Raises
    ------
    ValueError
        If `reason` is not a valid string.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id: Optional[VenueOrderId],
        AccountId account_id: Optional[AccountId],
        str reason not None,
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
        bint reconciliation=False,
    ):
        Condition.valid_string(reason, "reason")
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
        )

        self.reason = reason

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"account_id={self.account_id}, "  # Can be None
            f"reason={self.reason}, "
            f"ts_event={self.ts_event})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"account_id={self.account_id}, "  # Can be None
            f"reason={self.reason}, "
            f"event_id={self.id.to_str()}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderCancelRejected from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str v = values["venue_order_id"]
        cdef str a = values["account_id"]
        return OrderCancelRejected(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(v) if v is not None else None,
            account_id=AccountId(a) if a is not None else None,
            reason=values["reason"],
            event_id=UUID4(values["event_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            reconciliation=values.get("reconciliation", False),
        )

    @staticmethod
    cdef dict to_dict_c(OrderCancelRejected obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderCancelRejected",
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "venue_order_id": obj.venue_order_id.to_str() if obj.venue_order_id is not None else None,
            "account_id": obj.account_id.to_str() if obj.account_id is not None else None,
            "reason": obj.reason,
            "event_id": obj.id.to_str(),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "reconciliation": obj.reconciliation,
        }

    @staticmethod
    def from_dict(dict values) -> OrderCancelRejected:
        """
        Return an order cancel rejected event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderCancelRejected

        """
        return OrderCancelRejected.from_dict_c(values)

    @staticmethod
    def to_dict(OrderCancelRejected obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderCancelRejected.to_dict_c(obj)


cdef class OrderUpdated(OrderEvent):
    """
    Represents an event where an order has been updated at the trading venue.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId, optional with no default so ``None`` must be passed explicitly
        The venue order ID (assigned by the venue).
    account_id : AccountId, optional with no default so ``None`` must be passed explicitly
        The account ID (with the venue).
    quantity : Quantity
        The orders current quantity.
    price : Price, optional with no default so ``None`` must be passed explicitly
        The orders current price.
    trigger_price : Price, optional with no default so ``None`` must be passed explicitly
        The orders current trigger.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the order updated event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.

    Raises
    ------
    ValueError
        If `quantity` is not positive (> 0).
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id: Optional[VenueOrderId],
        AccountId account_id: Optional[AccountId],
        Quantity quantity not None,
        Price price: Optional[Price],
        Price trigger_price: Optional[Price],
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
        bint reconciliation=False,
    ):
        Condition.positive(quantity, "quantity")

        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
        )

        self.quantity = quantity
        self.price = price
        self.trigger_price = trigger_price

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"account_id={self.account_id}, "  # Can be None
            f"quantity={self.quantity.to_str()}, "
            f"price={self.price}, "
            f"trigger_price={self.trigger_price}, "
            f"ts_event={self.ts_event})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"account_id={self.account_id}, "  # Can be None
            f"quantity={self.quantity.to_str()}, "
            f"price={self.price}, "
            f"trigger_price={self.trigger_price}, "
            f"event_id={self.id.to_str()}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderUpdated from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str v = values["venue_order_id"]
        cdef str a = values["account_id"]
        cdef str p = values["price"]
        cdef str t = values["trigger_price"]
        return OrderUpdated(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(v) if v is not None else None,
            account_id=AccountId(a) if a is not None else None,
            quantity=Quantity.from_str_c(values["quantity"]),
            price=Price.from_str_c(p) if p is not None else None,
            trigger_price=Price.from_str_c(t) if t is not None else None,
            event_id=UUID4(values["event_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            reconciliation=values.get("reconciliation", False),
        )

    @staticmethod
    cdef dict to_dict_c(OrderUpdated obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderUpdated",
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "venue_order_id": obj.venue_order_id.to_str() if obj.venue_order_id is not None else None,
            "account_id": obj.account_id.to_str() if obj.account_id is not None else None,
            "quantity": str(obj.quantity),
            "price": str(obj.price) if obj.price is not None else None,
            "trigger_price": str(obj.trigger_price) if obj.trigger_price is not None else None,
            "event_id": obj.id.to_str(),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "reconciliation": obj.reconciliation,
        }

    @staticmethod
    def from_dict(dict values) -> OrderUpdated:
        """
        Return an order updated event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderUpdated

        """
        return OrderUpdated.from_dict_c(values)

    @staticmethod
    def to_dict(OrderUpdated obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderUpdated.to_dict_c(obj)


cdef class OrderFilled(OrderEvent):
    """
    Represents an event where an order has been filled at the exchange.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId
        The venue order ID (assigned by the venue).
    account_id : AccountId
        The account ID (with the venue).
    trade_id : TradeId
        The trade match ID (assigned by the venue).
    position_id : PositionId, optional with no default so ``None`` must be passed explicitly
        The position ID associated with the order fill (assigned by the venue).
    order_side : OrderSide {``BUY``, ``SELL``}
        The execution order side.
    order_side : OrderType
        The execution order type.
    last_qty : Quantity
        The fill quantity for this execution.
    last_px : Price
        The fill price for this execution (not average price).
    currency : Currency
        The currency of the price.
    commission : Money
        The fill commission.
    liquidity_side : LiquiditySide {``NO_LIQUIDITY_SIDE``, ``MAKER``, ``TAKER``}
        The execution liquidity side.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the order filled event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    info : dict[str, object], optional
        The additional fill information.
    reconciliation : bool, default False
        If the event was generated during reconciliation.

    Raises
    ------
    ValueError
        If `order_side` is ``NO_ORDER_SIDE``.
    ValueError
        If `last_qty` is not positive (> 0).
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        AccountId account_id not None,
        TradeId trade_id not None,
        PositionId position_id: Optional[PositionId],
        OrderSide order_side,
        OrderType order_type,
        Quantity last_qty not None,
        Price last_px not None,
        Currency currency not None,
        Money commission not None,
        LiquiditySide liquidity_side,
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
        bint reconciliation=False,
        dict info = None,
    ):
        Condition.not_equal(order_side, OrderSide.NO_ORDER_SIDE, "order_side", "NONE")
        Condition.positive(last_qty, "last_qty")

        if info is None:
            info = {}
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
        )

        self.trade_id = trade_id
        self.position_id = position_id
        self.order_side = order_side
        self.order_type = order_type
        self.last_qty = last_qty
        self.last_px = last_px
        self.currency = currency
        self.commission = commission
        self.liquidity_side = liquidity_side
        self.info = info

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id.to_str()}, "
            f"account_id={self.account_id.to_str()}, "
            f"trade_id={self.trade_id.to_str()}, "
            f"position_id={self.position_id}, "
            f"order_side={order_side_to_str(self.order_side)}, "
            f"order_type={order_type_to_str(self.order_type)}, "
            f"last_qty={self.last_qty.to_str()}, "
            f"last_px={self.last_px} {self.currency.code}, "
            f"commission={self.commission.to_str()}, "
            f"liquidity_side={liquidity_side_to_str(self.liquidity_side)}, "
            f"ts_event={self.ts_event})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id.to_str()}, "
            f"account_id={self.account_id.to_str()}, "
            f"trade_id={self.trade_id.to_str()}, "
            f"position_id={self.position_id}, "
            f"order_side={order_side_to_str(self.order_side)}, "
            f"order_type={order_type_to_str(self.order_type)}, "
            f"last_qty={self.last_qty.to_str()}, "
            f"last_px={self.last_px} {self.currency.code}, "
            f"commission={self.commission.to_str()}, "
            f"liquidity_side={liquidity_side_to_str(self.liquidity_side)}, "
            f"event_id={self.id.to_str()}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef OrderFilled from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str position_id_str = values["position_id"]
        return OrderFilled(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            account_id=AccountId(values["account_id"]),
            trade_id=TradeId(values["trade_id"]),
            position_id=PositionId(position_id_str) if position_id_str is not None else None,
            order_side=order_side_from_str(values["order_side"]),
            order_type=order_type_from_str(values["order_type"]),
            last_qty=Quantity.from_str_c(values["last_qty"]),
            last_px=Price.from_str_c(values["last_px"]),
            currency=Currency.from_str_c(values["currency"]),
            commission=Money.from_str_c(values["commission"]),
            liquidity_side=liquidity_side_from_str(values["liquidity_side"]),
            event_id=UUID4(values["event_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            info=msgspec.json.decode(values["info"]),
            reconciliation=values.get("reconciliation", False),
        )

    @staticmethod
    cdef dict to_dict_c(OrderFilled obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderFilled",
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "venue_order_id": obj.venue_order_id.to_str(),
            "account_id": obj.account_id.to_str(),
            "trade_id": obj.trade_id.to_str(),
            "position_id": obj.position_id.to_str() if obj.position_id else None,
            "order_side": order_side_to_str(obj.order_side),
            "order_type": order_type_to_str(obj.order_type),
            "last_qty": str(obj.last_qty),
            "last_px": str(obj.last_px),
            "currency": obj.currency.code,
            "commission": obj.commission.to_str(),
            "liquidity_side": liquidity_side_to_str(obj.liquidity_side),
            "event_id": obj.id.to_str(),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "info": msgspec.json.encode(obj.info),
            "reconciliation": obj.reconciliation,
        }

    @staticmethod
    def from_dict(dict values) -> OrderFilled:
        """
        Return an order filled event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderFilled

        """
        return OrderFilled.from_dict_c(values)

    @staticmethod
    def to_dict(OrderFilled obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderFilled.to_dict_c(obj)

    cdef bint is_buy_c(self) except *:
        return self.order_side == OrderSide.BUY

    cdef bint is_sell_c(self) except *:
        return self.order_side == OrderSide.SELL

    @property
    def is_buy(self):
        """
        Return whether the fill order side is ``BUY``.

        Returns
        -------
        bool

        """
        return self.is_buy_c()

    @property
    def is_sell(self):
        """
        Return whether the fill order side is ``SELL``.

        Returns
        -------
        bool

        """
        return self.is_sell_c()
