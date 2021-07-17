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

import orjson

from libc.stdint cimport int64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySideParser
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_type cimport OrderTypeParser
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForceParser
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class OrderEvent(Event):
    """
    The abstract base class for all order events.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``OrderEvent` base class.

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
            The venue order ID.
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        """
        super().__init__(event_id, timestamp_ns)

        self.trader_id = trader_id
        self.strategy_id = strategy_id
        self.instrument_id = instrument_id
        self.client_order_id = client_order_id
        self.venue_order_id = venue_order_id


cdef class OrderInitialized(OrderEvent):
    """
    Represents an event where an order has been initialized.

    This is a seed event that any order can then be instantiated from through
    a creation method. This event should contain enough information to be able
    send it over a wire and have a valid order instantiated with exactly the
    same parameters as if it had been instantiated locally.
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
        UUID event_id not None,
        int64_t timestamp_ns,
        dict options not None,
    ):
        """
        Initialize a new instance of the ``OrderInitialized`` class.

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
        order_side : OrderSide
            The order side.
        order_type : OrderType
            The order type.
        quantity : Quantity
            The order quantity.
        time_in_force : TimeInForce
            The order time-in-force.
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) when the order was initialized.
        options : dict[str, str]
            The order initialization options. Contains mappings for specific
            order parameters.

        """
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            VenueOrderId.null_c(),  # Pending assignment by venue
            event_id,
            timestamp_ns,
        )

        self.side = order_side
        self.type = order_type
        self.quantity = quantity
        self.time_in_force = time_in_force
        self.options = options

    def __str__(self) -> str:
        return (f"{type(self).__name__}("
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"side={OrderSideParser.to_str(self.side)}, "
                f"type={OrderTypeParser.to_str(self.type)}, "
                f"quantity={self.quantity.to_str()}, "
                f"options={self.options})")

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"side={OrderSideParser.to_str(self.side)}, "
                f"type={OrderTypeParser.to_str(self.type)}, "
                f"quantity={self.quantity.to_str()}, "
                f"options={self.options}, "
                f"event_id={self.id}, "
                f"timestamp_ns={self.timestamp_ns})")

    @staticmethod
    cdef OrderInitialized from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderInitialized(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            order_side=OrderSideParser.from_str(values["order_side"]),
            order_type=OrderTypeParser.from_str(values["order_type"]),
            quantity=Quantity.from_str_c(values["quantity"]),
            time_in_force=TimeInForceParser.from_str(values["time_in_force"]),
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
            options=orjson.loads(values["options"]),
        )

    @staticmethod
    cdef dict to_dict_c(OrderInitialized obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderInitialized",
            "trader_id": obj.trader_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "client_order_id": obj.client_order_id.value,
            "order_side": OrderSideParser.to_str(obj.side),
            "order_type": OrderTypeParser.to_str(obj.type),
            "quantity": str(obj.quantity),
            "time_in_force": TimeInForceParser.to_str(obj.time_in_force),
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
            "options": orjson.dumps(obj.options).decode(),
        }

    @staticmethod
    def from_dict(dict values):
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
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        str reason not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``OrderDenied`` class.

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
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        Raises
        ------
        ValueError
            If denied_reason is not a valid_string.

        """
        Condition.valid_string(reason, "denied_reason")
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            VenueOrderId.null_c(),  # Never assigned
            event_id,
            timestamp_ns,
        )

        self.reason = reason

    def __str__(self) -> str:
        return (f"{type(self).__name__}("
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"reason={self.reason})")

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"reason={self.reason}, "
                f"event_id={self.id}, "
                f"timestamp_ns={self.timestamp_ns})")

    @staticmethod
    cdef OrderDenied from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderDenied(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            reason=values["reason"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderDenied obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderDenied",
            "trader_id": obj.trader_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "client_order_id": obj.client_order_id.value,
            "reason": obj.reason,
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
        }

    @staticmethod
    def from_dict(dict values):
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
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        AccountId account_id not None,
        ClientOrderId client_order_id not None,
        int64_t ts_submitted_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``OrderSubmitted`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID.
        strategy_id : StrategyId
            The strategy ID.
        instrument_id : InstrumentId
            The instrument ID.
        account_id : AccountId
            The account ID.
        client_order_id : ClientOrderId
            The client order ID.
        ts_submitted_ns : int64
            The UNIX timestamp (nanoseconds) when the order was submitted.
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        """
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            VenueOrderId.null_c(),  # Pending accepted
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.ts_submitted_ns = ts_submitted_ns

    def __str__(self) -> str:
        return (f"{type(self).__name__}("
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"ts_submitted_ns={self.ts_submitted_ns})")

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"ts_submitted_ns={self.ts_submitted_ns}, "
                f"event_id={self.id}, "
                f"timestamp_ns={self.timestamp_ns})")

    @staticmethod
    cdef OrderSubmitted from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderSubmitted(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            ts_submitted_ns=values["ts_submitted_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderSubmitted obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderSubmitted",
            "trader_id": obj.trader_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "account_id": obj.account_id.value,
            "client_order_id": obj.client_order_id.value,
            "ts_submitted_ns": obj.ts_submitted_ns,
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
        }

    @staticmethod
    def from_dict(dict values):
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
    execution reports.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/tagNum_39.html
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        AccountId account_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        int64_t ts_accepted_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``OrderAccepted`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID.
        strategy_id : StrategyId
            The strategy ID.
        instrument_id : InstrumentId
            The instrument ID.
        account_id : AccountId
            The account ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID.
        ts_accepted_ns : int64
            The UNIX timestamp (nanoseconds) when the order was accepted.
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        Raises
        ------
        ValueError
            If venue_order_id has a 'NULL' value.

        """
        Condition.true(venue_order_id.not_null(), "venue_order_id was 'NULL'")
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.ts_accepted_ns = ts_accepted_ns

    def __str__(self) -> str:
        return (f"{type(self).__name__}("
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"ts_accepted_ns={self.ts_accepted_ns})")

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"ts_accepted_ns={self.ts_accepted_ns}, "
                f"event_id={self.id}, "
                f"timestamp_ns={self.timestamp_ns})")

    @staticmethod
    cdef OrderAccepted from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderAccepted(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            ts_accepted_ns=values["ts_accepted_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderAccepted obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderAccepted",
            "trader_id": obj.trader_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "account_id": obj.account_id.value,
            "client_order_id": obj.client_order_id.value,
            "venue_order_id": obj.venue_order_id.value,
            "ts_accepted_ns": obj.ts_accepted_ns,
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
        }

    @staticmethod
    def from_dict(dict values):
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
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        AccountId account_id not None,
        ClientOrderId client_order_id not None,
        str reason not None,
        int64_t ts_rejected_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``OrderRejected`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID.
        strategy_id : StrategyId
            The strategy ID.
        instrument_id : InstrumentId
            The instrument ID.
        account_id : AccountId
            The account ID.
        client_order_id : ClientOrderId
            The client order ID.
        reason : datetime
            The order rejected reason.
        ts_rejected_ns : int64
            The UNIX timestamp (nanoseconds) when the order was rejected.
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        Raises
        ------
        ValueError
            If rejected_reason is not a valid_string.

        """
        Condition.valid_string(reason, "rejected_reason")
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            VenueOrderId.null_c(),  # Not assigned on rejection
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.reason = reason
        self.ts_rejected_ns = ts_rejected_ns

    def __str__(self) -> str:
        return (f"{type(self).__name__}("
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"reason='{self.reason}', "
                f"ts_accepted_ns={self.ts_rejected_ns})")

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"reason='{self.reason}', "
                f"ts_accepted_ns={self.ts_rejected_ns}, "
                f"event_id={self.id}, "
                f"timestamp_ns={self.timestamp_ns})")

    @staticmethod
    cdef OrderRejected from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderRejected(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            reason=values["reason"],
            ts_rejected_ns=values["ts_rejected_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderRejected obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderRejected",
            "trader_id": obj.trader_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "account_id": obj.account_id.value,
            "client_order_id": obj.client_order_id.value,
            "reason": obj.reason,
            "ts_rejected_ns": obj.ts_rejected_ns,
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
        }

    @staticmethod
    def from_dict(dict values):
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
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        AccountId account_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        int64_t ts_canceled_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``OrderCanceled`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID.
        strategy_id : StrategyId
            The strategy ID.
        instrument_id : InstrumentId
            The instrument ID.
        account_id : AccountId
            The account ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID.
        ts_canceled_ns : int64
            The UNIX timestamp (nanoseconds) when order was canceled.
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        Raises
        ------
        ValueError
            If venue_order_id has a 'NULL' value.

        """
        Condition.true(venue_order_id.not_null(), "venue_order_id was 'NULL'")
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.ts_canceled_ns = ts_canceled_ns

    def __str__(self) -> str:
        return (f"{type(self).__name__}("
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"ts_canceled_ns={self.ts_canceled_ns})")

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"ts_canceled_ns={self.ts_canceled_ns}, "
                f"event_id={self.id}, "
                f"timestamp_ns={self.timestamp_ns})")

    @staticmethod
    cdef OrderCanceled from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderCanceled(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            ts_canceled_ns=values["ts_canceled_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderCanceled obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderCanceled",
            "trader_id": obj.trader_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "account_id": obj.account_id.value,
            "client_order_id": obj.client_order_id.value,
            "venue_order_id": obj.venue_order_id.value,
            "ts_canceled_ns": obj.ts_canceled_ns,
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
        }

    @staticmethod
    def from_dict(dict values):
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
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        AccountId account_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        int64_t ts_expired_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``OrderExpired`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID.
        strategy_id : StrategyId
            The strategy ID.
        instrument_id : InstrumentId
            The instrument ID.
        account_id : AccountId
            The account ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID.
        ts_expired_ns : int64
            The UNIX timestamp (nanoseconds) when the order expired.
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        Raises
        ------
        ValueError
            If venue_order_id has a 'NULL' value.

        """
        Condition.true(venue_order_id.not_null(), "venue_order_id was 'NULL'")
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.ts_expired_ns = ts_expired_ns

    def __str__(self) -> str:
        return (f"{type(self).__name__}("
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"ts_expired_ns={self.ts_expired_ns})")

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"ts_expired_ns={self.ts_expired_ns}, "
                f"event_id={self.id}, "
                f"timestamp_ns={self.timestamp_ns})")

    @staticmethod
    cdef OrderExpired from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderExpired(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            ts_expired_ns=values["ts_expired_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderExpired obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderExpired",
            "trader_id": obj.trader_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "account_id": obj.account_id.value,
            "client_order_id": obj.client_order_id.value,
            "venue_order_id": obj.venue_order_id.value,
            "ts_expired_ns": obj.ts_expired_ns,
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
        }

    @staticmethod
    def from_dict(dict values):
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
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        AccountId account_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        int64_t ts_triggered_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``OrderTriggered`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID.
        strategy_id : StrategyId
            The strategy ID.
        instrument_id : InstrumentId
            The instrument ID.
        account_id : AccountId
            The account ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID.
        ts_triggered_ns : int64
            The UNIX timestamp (nanoseconds) when the order was triggered.
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        Raises
        ------
        ValueError
            If venue_order_id has a 'NULL' value.

        """
        Condition.true(venue_order_id.not_null(), "venue_order_id was 'NULL'")
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.ts_triggered_ns = ts_triggered_ns

    def __str__(self) -> str:
        return (f"{type(self).__name__}("
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"ts_triggered_ns={self.ts_triggered_ns})")

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"ts_triggered_ns={self.ts_triggered_ns}, "
                f"event_id={self.id}, "
                f"timestamp_ns={self.timestamp_ns})")

    @staticmethod
    cdef OrderTriggered from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderTriggered(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            ts_triggered_ns=values["ts_triggered_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderTriggered obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderTriggered",
            "trader_id": obj.trader_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "account_id": obj.account_id.value,
            "client_order_id": obj.client_order_id.value,
            "venue_order_id": obj.venue_order_id.value,
            "ts_triggered_ns": obj.ts_triggered_ns,
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
        }

    @staticmethod
    def from_dict(dict values):
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
    Represents an event where an `UpdateOrder` command has been sent to the
    trading venue.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        AccountId account_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        int64_t ts_pending_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``OrderPendingUpdate`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID.
        strategy_id : StrategyId
            The strategy ID.
        instrument_id : InstrumentId
            The instrument ID.
        account_id : AccountId
            The account ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID.
        ts_pending_ns : datetime
            The UNIX timestamp (nanoseconds) when the replace was pending.
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        Raises
        ------
        ValueError
            If venue_order_id has a 'NULL' value.

        """
        Condition.true(venue_order_id.not_null(), "venue_order_id was 'NULL'")
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.ts_pending_ns = ts_pending_ns

    def __str__(self) -> str:
        return (f"{type(self).__name__}("
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"ts_pending_ns={self.ts_pending_ns})")

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"ts_pending_ns={self.ts_pending_ns}, "
                f"event_id={self.id}, "
                f"timestamp_ns={self.timestamp_ns})")

    @staticmethod
    cdef OrderPendingUpdate from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderPendingUpdate(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            ts_pending_ns=values["ts_pending_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderPendingUpdate obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderPendingUpdate",
            "trader_id": obj.trader_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "account_id": obj.account_id.value,
            "client_order_id": obj.client_order_id.value,
            "venue_order_id": obj.venue_order_id.value,
            "ts_pending_ns": obj.ts_pending_ns,
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
        }

    @staticmethod
    def from_dict(dict values):
        """
        Return an order pending replace event from the given dict values.

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
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        AccountId account_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        int64_t ts_pending_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``OrderPendingCancel`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID.
        strategy_id : StrategyId
            The strategy ID.
        instrument_id : InstrumentId
            The instrument ID.
        account_id : AccountId
            The account ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID.
        ts_pending_ns : datetime
            The UNIX timestamp (nanoseconds) when the cancel was pending.
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        Raises
        ------
        ValueError
            If venue_order_id has a 'NULL' value.

        """
        Condition.true(venue_order_id.not_null(), "venue_order_id was 'NULL'")
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.ts_pending_ns = ts_pending_ns

    def __str__(self) -> str:
        return (f"{type(self).__name__}("
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"ts_pending_ns={self.ts_pending_ns})")

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"ts_pending_ns={self.ts_pending_ns}, "
                f"event_id={self.id}, "
                f"timestamp_ns={self.timestamp_ns})")

    @staticmethod
    cdef OrderPendingCancel from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderPendingCancel(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            ts_pending_ns=values["ts_pending_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderPendingCancel obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderPendingCancel",
            "trader_id": obj.trader_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "account_id": obj.account_id.value,
            "client_order_id": obj.client_order_id.value,
            "venue_order_id": obj.venue_order_id.value,
            "ts_pending_ns": obj.ts_pending_ns,
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
        }

    @staticmethod
    def from_dict(dict values):
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


cdef class OrderUpdateRejected(OrderEvent):
    """
    Represents an event where an `UpdateOrder` command has been rejected by the
    trading venue.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        AccountId account_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        str response_to not None,
        str reason not None,
        int64_t ts_rejected_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``OrderUpdateRejected`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID.
        strategy_id : StrategyId
            The strategy ID.
        instrument_id : InstrumentId
            The instrument ID.
        account_id : AccountId
            The account ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID.
        response_to : str
            The order update rejected response.
        reason : str
            The order update rejected reason.
        ts_rejected_ns : datetime
            The UNIX timestamp (nanoseconds) when the order update was rejected.
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        Raises
        ------
        ValueError
            If rejected_response_to is not a valid string.
        ValueError
            If rejected_reason is not a valid string.

        """
        Condition.valid_string(response_to, "rejected_response_to")
        Condition.valid_string(reason, "rejected_reason")
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.response_to = response_to
        self.reason = reason
        self.ts_rejected_ns = ts_rejected_ns

    def __str__(self) -> str:
        return (f"{type(self).__name__}("
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"response_to={self.response_to}, "
                f"reason={self.reason}, "
                f"ts_rejected_ns={self.ts_rejected_ns})")

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"response_to={self.response_to}, "
                f"reason={self.reason}, "
                f"ts_rejected_ns={self.ts_rejected_ns}, "
                f"event_id={self.id}, "
                f"timestamp_ns={self.timestamp_ns})")

    @staticmethod
    cdef OrderUpdateRejected from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderUpdateRejected(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            response_to=values["response_to"],
            reason=values["reason"],
            ts_rejected_ns=values["ts_rejected_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderUpdateRejected obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderUpdateRejected",
            "trader_id": obj.trader_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "account_id": obj.account_id.value,
            "client_order_id": obj.client_order_id.value,
            "venue_order_id": obj.venue_order_id.value,
            "response_to": obj.response_to,
            "reason": obj.reason,
            "ts_rejected_ns": obj.ts_rejected_ns,
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
        }

    @staticmethod
    def from_dict(dict values):
        """
        Return an order update rejected event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderUpdateRejected

        """
        return OrderUpdateRejected.from_dict_c(values)

    @staticmethod
    def to_dict(OrderUpdateRejected obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderUpdateRejected.to_dict_c(obj)


cdef class OrderCancelRejected(OrderEvent):
    """
    Represents an event where a `CancelOrder` command has been rejected by the
    trading venue.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        AccountId account_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        str response_to not None,
        str reason not None,
        int64_t ts_rejected_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``OrderCancelRejected`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID.
        strategy_id : StrategyId
            The strategy ID.
        instrument_id : InstrumentId
            The instrument ID.
        account_id : AccountId
            The account ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID.
        response_to : str
            The order cancel rejected response.
        reason : str
            The order cancel rejected reason.
        ts_rejected_ns : datetime
            The UNIX timestamp (nanoseconds) when the order cancel was rejected.
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        Raises
        ------
        ValueError
            If rejected_response_to is not a valid string.
        ValueError
            If rejected_reason is not a valid string.

        """
        Condition.valid_string(response_to, "rejected_response_to")
        Condition.valid_string(reason, "rejected_reason")
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.response_to = response_to
        self.reason = reason
        self.ts_rejected_ns = ts_rejected_ns

    def __str__(self) -> str:
        return (f"{type(self).__name__}("
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"response_to={self.response_to}, "
                f"reason={self.reason}, "
                f"ts_rejected_ns={self.ts_rejected_ns})")

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"response_to={self.response_to}, "
                f"reason={self.reason}, "
                f"ts_rejected_ns={self.ts_rejected_ns}, "
                f"event_id={self.id}, "
                f"timestamp_ns={self.timestamp_ns})")

    @staticmethod
    cdef OrderCancelRejected from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderCancelRejected(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            response_to=values["response_to"],
            reason=values["reason"],
            ts_rejected_ns=values["ts_rejected_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderCancelRejected obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderCancelRejected",
            "trader_id": obj.trader_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "account_id": obj.account_id.value,
            "client_order_id": obj.client_order_id.value,
            "venue_order_id": obj.venue_order_id.value,
            "response_to": obj.response_to,
            "reason": obj.reason,
            "ts_rejected_ns": obj.ts_rejected_ns,
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
        }

    @staticmethod
    def from_dict(dict values):
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
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        AccountId account_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        Quantity quantity not None,
        Price price not None,
        Price trigger,  # Can be None
        int64_t ts_updated_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``OrderUpdated`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID.
        strategy_id : StrategyId
            The strategy ID.
        instrument_id : InstrumentId
            The instrument ID.
        account_id : AccountId
            The account ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID.
        quantity : Quantity
            The orders current quantity.
        price : Price
            The orders current price.
        trigger : Price, optional
            The orders current trigger.
        ts_updated_ns : int64
            The UNIX timestamp (nanoseconds) when the order was updated.
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        Raises
        ------
        ValueError
            If venue_order_id has a 'NULL' value.

        """
        Condition.true(venue_order_id.not_null(), "venue_order_id was 'NULL'")
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.quantity = quantity
        self.price = price
        self.trigger = trigger
        self.ts_updated_ns = ts_updated_ns

    def __str__(self) -> str:
        return (f"{type(self).__name__}("
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"quantity={self.quantity.to_str()}, "
                f"price={self.price}, "
                f"trigger={self.trigger}, "
                f"ts_updated_ns={self.ts_updated_ns})")

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"quantity={self.quantity.to_str()}, "
                f"price={self.price}, "
                f"trigger={self.trigger}, "
                f"ts_updated_ns={self.ts_updated_ns}, "
                f"event_id={self.id}, "
                f"timestamp_ns={self.timestamp_ns})")

    @staticmethod
    cdef OrderUpdated from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str t = values["trigger"]
        return OrderUpdated(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            quantity=Quantity.from_str_c(values["quantity"]),
            price=Price.from_str_c(values["price"]),
            trigger=Price.from_str_c(t) if t is not None else None,
            ts_updated_ns=values["ts_updated_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderUpdated obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderUpdated",
            "trader_id": obj.trader_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "account_id": obj.account_id.value,
            "client_order_id": obj.client_order_id.value,
            "venue_order_id": obj.venue_order_id.value,
            "quantity": str(obj.quantity),
            "price": str(obj.price),
            "trigger": str(obj.trigger) if obj.trigger is not None else None,
            "ts_updated_ns": obj.ts_updated_ns,
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
        }

    @staticmethod
    def from_dict(dict values):
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
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        AccountId account_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        ExecutionId execution_id not None,
        PositionId position_id not None,
        OrderSide order_side,
        OrderType order_type,
        Quantity last_qty not None,
        Price last_px not None,
        Currency currency not None,
        Money commission not None,
        LiquiditySide liquidity_side,
        int64_t ts_filled_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
        dict info=None,
    ):
        """
        Initialize a new instance of the ``OrderFilled`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID.
        strategy_id : StrategyId
            The strategy ID.
        instrument_id : InstrumentId
            The instrument ID.
        account_id : AccountId
            The account ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID.
        execution_id : ExecutionId
            The execution ID.
        position_id : PositionId
            The position ID associated with the order.
        order_side : OrderSide
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
        liquidity_side : LiquiditySide
            The execution liquidity side.
        ts_filled_ns : int64
            The UNIX timestamp (nanoseconds) when the order was filled.
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.
        info : dict[str, object], optional
            The additional fill information.

        Raises
        ------
        ValueError
            If venue_order_id has a 'NULL' value.
        ValueError
            If last_qty is not positive (> 0).

        """
        Condition.true(venue_order_id.not_null(), "venue_order_id was 'NULL'")
        Condition.positive(last_qty, "last_qty")
        if info is None:
            info = {}
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.execution_id = execution_id
        self.position_id = position_id
        self.side = order_side
        self.type = order_type
        self.last_qty = last_qty
        self.last_px = last_px
        self.currency = currency
        self.commission = commission
        self.liquidity_side = liquidity_side
        self.ts_filled_ns = ts_filled_ns
        self.info = info

    def __str__(self) -> str:
        return (f"{type(self).__name__}("
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"execution_id={self.execution_id.value}, "
                f"position_id={self.position_id.value}, "
                f"side={OrderSideParser.to_str(self.side)}"
                f"-{LiquiditySideParser.to_str(self.liquidity_side)}, "
                f"type={OrderTypeParser.to_str(self.type)}, "
                f"last_qty={self.last_qty.to_str()}, "
                f"last_px={self.last_px} {self.currency.code}, "
                f"commission={self.commission.to_str()}, "
                f"ts_filled_ns={self.ts_filled_ns})")

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"account_id={self.account_id.value}, "
                f"execution_id={self.execution_id.value}, "
                f"position_id={self.position_id.value}, "
                f"side={OrderSideParser.to_str(self.side)}"
                f"-{LiquiditySideParser.to_str(self.liquidity_side)}, "
                f"type={OrderTypeParser.to_str(self.type)}, "
                f"last_qty={self.last_qty.to_str()}, "
                f"last_px={self.last_px} {self.currency.code}, "
                f"commission={self.commission.to_str()}, "
                f"ts_filled_ns={self.ts_filled_ns}, "
                f"event_id={self.id}, "
                f"timestamp_ns={self.timestamp_ns})")

    @staticmethod
    cdef OrderFilled from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderFilled(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            execution_id=ExecutionId(values["execution_id"]),
            position_id=PositionId(values["position_id"]),
            order_side=OrderSideParser.from_str(values["order_side"]),
            order_type=OrderTypeParser.from_str(values["order_type"]),
            last_qty=Quantity.from_str_c(values["last_qty"]),
            last_px=Price.from_str_c(values["last_px"]),
            currency=Currency.from_str_c(values["currency"]),
            commission=Money.from_str_c(values["commission"]),
            liquidity_side=LiquiditySideParser.from_str(values["liquidity_side"]),
            ts_filled_ns=values["ts_filled_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
            info=orjson.loads(values["info"])
        )

    @staticmethod
    cdef dict to_dict_c(OrderFilled obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OrderFilled",
            "trader_id": obj.trader_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "account_id": obj.account_id.value,
            "client_order_id": obj.client_order_id.value,
            "venue_order_id": obj.venue_order_id.value,
            "execution_id": obj.execution_id.value,
            "position_id": obj.position_id.value,
            "order_side": OrderSideParser.to_str(obj.side),
            "order_type": OrderTypeParser.to_str(obj.type),
            "last_qty": str(obj.last_qty),
            "last_px": str(obj.last_px),
            "currency": obj.currency.code,
            "commission": obj.commission.to_str(),
            "liquidity_side": LiquiditySideParser.to_str(obj.liquidity_side),
            "ts_filled_ns": obj.ts_filled_ns,
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
            "info": orjson.dumps(obj.info).decode(),
        }

    @staticmethod
    def from_dict(dict values):
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
        return self.side == OrderSide.BUY

    cdef bint is_sell_c(self) except *:
        return self.side == OrderSide.SELL

    @property
    def is_buy(self):
        """
        If the fill order side is BUY.

        Returns
        -------
        bool
            True if BUY, else False.

        """
        return self.is_buy_c()

    @property
    def is_sell(self):
        """
        If the fill order side is SELL.

        Returns
        -------
        bool
            True if SELL, else False.

        """
        return self.is_sell_c()
