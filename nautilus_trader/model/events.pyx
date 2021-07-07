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

import json

import pandas as pd

from libc.stdint cimport int64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.account_type cimport AccountTypeParser
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
from nautilus_trader.model.objects cimport AccountBalance
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class AccountState(Event):
    """
    Represents an event which includes information on the state of the account.
    """

    def __init__(
        self,
        AccountId account_id not None,
        AccountType account_type,
        Currency base_currency,
        bint reported,
        list balances not None,
        dict info not None,
        UUID event_id not None,
        int64_t ts_updated_ns,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``AccountState`` class.

        Parameters
        ----------
        account_id : AccountId
            The account ID.
        account_type : AccountId
            The account type for the event.
        base_currency : Currency, optional
            The account base currency. Use None for multi-currency accounts.
        reported : bool
            If the state is reported from the exchange (otherwise system calculated).
        balances : list[AccountBalance]
            The account balances
        info : dict [str, object]
            The additional implementation specific account information.
        event_id : UUID
            The event ID.
        ts_updated_ns : int64
            The UNIX timestamp (nanoseconds) when the account was updated.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        """
        super().__init__(event_id, timestamp_ns)

        self.account_id = account_id
        self.account_type = account_type
        self.base_currency = base_currency
        self.balances = balances
        self.is_reported = reported
        self.info = info
        self.ts_updated_ns = ts_updated_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id.value}, "
                f"account_type={AccountTypeParser.to_str(self.account_type)}, "
                f"base_currency={self.base_currency}, "
                f"is_reported={self.is_reported}, "
                f"balances=[{', '.join([str(b) for b in self.balances])}], "
                f"event_id={self.id})")

    @staticmethod
    cdef AccountState from_dict_c(dict values):
        cdef str base_str = values["base_currency"]
        return AccountState(
            account_id=AccountId.from_str_c(values["account_id"]),
            account_type=AccountTypeParser.from_str(values["account_type"]),
            base_currency=Currency.from_str_c(base_str) if base_str is not None else None,
            reported=values["reported"],
            balances=[AccountBalance.from_dict(b) for b in json.loads(values["balances"])],
            info=json.loads(values["info"]),
            event_id=UUID.from_str_c(values["event_id"]),
            ts_updated_ns=values["ts_updated_ns"],
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(AccountState obj):
        return {
            "type": "AccountState",
            "account_id": obj.account_id.value,
            "account_type": AccountTypeParser.to_str(obj.account_type),
            "base_currency": obj.base_currency.code if obj.base_currency else None,
            "balances": json.dumps([b.to_dict() for b in obj.balances]),
            "reported": obj.is_reported,
            "info": json.dumps(obj.info),
            "event_id": obj.id.value,
            "ts_updated_ns": obj.ts_updated_ns,
            "timestamp_ns": obj.timestamp_ns,
        }

    @staticmethod
    def from_dict(dict values):
        """
        Return an account state event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        AccountState

        """
        return AccountState.from_dict_c(values)

    @staticmethod
    def to_dict(AccountState obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return AccountState.to_dict_c(obj)


cdef class OrderEvent(Event):
    """
    The abstract base class for all order events.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``OrderEvent` base class.

        Parameters
        ----------
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
        ClientOrderId client_order_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
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
        client_order_id : ClientOrderId
            The client order ID.
        strategy_id : StrategyId
            The strategy ID associated with the order.
        instrument_id : InstrumentId
            The order instrument ID.
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
            client_order_id,
            VenueOrderId.null_c(),  # Pending assignment by venue
            event_id,
            timestamp_ns,
        )

        self.client_order_id = client_order_id
        self.strategy_id = strategy_id
        self.instrument_id = instrument_id
        self.order_side = order_side
        self.order_type = order_type
        self.quantity = quantity
        self.time_in_force = time_in_force
        self.options = options

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"client_order_id={self.client_order_id}, "
                f"strategy_id={self.strategy_id}, "
                f"event_id={self.id})")

    @staticmethod
    cdef OrderInitialized from_dict_c(dict values):
        return OrderInitialized(
            client_order_id=ClientOrderId(values["client_order_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            order_side=OrderSideParser.from_str(values["order_side"]),
            order_type=OrderTypeParser.from_str(values["order_type"]),
            quantity=Quantity.from_str_c(values["quantity"]),
            time_in_force=TimeInForceParser.from_str(values["time_in_force"]),
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
            options=json.loads(values["options"]),
        )

    @staticmethod
    cdef dict to_dict_c(OrderInitialized obj):
        return {
            "type": "OrderInitialized",
            "client_order_id": obj.client_order_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "order_side": OrderSideParser.to_str(obj.order_side),
            "order_type": OrderTypeParser.to_str(obj.order_type),
            "quantity": str(obj.quantity),
            "time_in_force": TimeInForceParser.to_str(obj.time_in_force),
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
            "options": json.dumps(obj.options),
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
        ClientOrderId client_order_id not None,
        str reason not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``OrderDenied`` class.

        Parameters
        ----------
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
            client_order_id,
            VenueOrderId.null_c(),  # Never assigned
            event_id,
            timestamp_ns,
        )

        self.reason = reason

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"client_order_id={self.client_order_id}, "
                f"reason={self.reason}, "
                f"event_id={self.id})")

    @staticmethod
    cdef OrderDenied from_dict_c(dict values):
        return OrderDenied(
            client_order_id=ClientOrderId(values["client_order_id"]),
            reason=values["reason"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderDenied obj):
        return {
            "type": "OrderDenied",
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
            client_order_id,
            VenueOrderId.null_c(),  # Pending accepted
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.ts_submitted_ns = ts_submitted_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"client_order_id={self.client_order_id}, "
                f"event_id={self.id})")

    @staticmethod
    cdef OrderSubmitted from_dict_c(dict values):
        return OrderSubmitted(
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            ts_submitted_ns=values["ts_submitted_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderSubmitted obj):
        return {
            "type": "OrderSubmitted",
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
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.ts_accepted_ns = ts_accepted_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"client_order_id={self.client_order_id}, "
                f"venue_order_id={self.venue_order_id}, "
                f"event_id={self.id})")

    @staticmethod
    cdef OrderAccepted from_dict_c(dict values):
        return OrderAccepted(
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            ts_accepted_ns=values["ts_accepted_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderAccepted obj):
        return {
            "type": "OrderAccepted",
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
            client_order_id,
            VenueOrderId.null_c(),  # Not assigned on rejection
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.reason = reason
        self.ts_rejected_ns = ts_rejected_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"client_order_id={self.client_order_id}, "
                f"reason='{self.reason}', "
                f"event_id={self.id})")

    @staticmethod
    cdef OrderRejected from_dict_c(dict values):
        return OrderRejected(
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            reason=values["reason"],
            ts_rejected_ns=values["ts_rejected_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderRejected obj):
        return {
            "type": "OrderRejected",
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
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.ts_canceled_ns = ts_canceled_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"client_order_id={self.client_order_id}, "
                f"venue_order_id={self.venue_order_id}, "
                f"event_id={self.id})")

    @staticmethod
    cdef OrderCanceled from_dict_c(dict values):
        return OrderCanceled(
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            ts_canceled_ns=values["ts_canceled_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderCanceled obj):
        return {
            "type": "OrderCanceled",
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
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.ts_expired_ns = ts_expired_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"client_order_id={self.client_order_id}, "
                f"venue_order_id={self.venue_order_id}, "
                f"event_id={self.id})")

    @staticmethod
    cdef OrderExpired from_dict_c(dict values):
        return OrderExpired(
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            ts_expired_ns=values["ts_expired_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderExpired obj):
        return {
            "type": "OrderExpired",
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
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.ts_triggered_ns = ts_triggered_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"client_order_id={self.client_order_id}, "
                f"venue_order_id={self.venue_order_id}, "
                f"event_id={self.id})")

    @staticmethod
    cdef OrderTriggered from_dict_c(dict values):
        return OrderTriggered(
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            ts_triggered_ns=values["ts_triggered_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderTriggered obj):
        return {
            "type": "OrderTriggered",
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
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.ts_pending_ns = ts_pending_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"client_order_id={self.client_order_id}, "
                f"venue_order_id={self.venue_order_id}, "
                f"ts_pending_ns={self.ts_pending_ns}, "
                f"event_id={self.id})")

    @staticmethod
    cdef OrderPendingUpdate from_dict_c(dict values):
        return OrderPendingUpdate(
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            ts_pending_ns=values["ts_pending_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderPendingUpdate obj):
        return {
            "type": "OrderPendingUpdate",
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
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.ts_pending_ns = ts_pending_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"client_order_id={self.client_order_id}, "
                f"venue_order_id={self.venue_order_id}, "
                f"ts_pending_ns={self.ts_pending_ns}, "
                f"event_id={self.id})")

    @staticmethod
    cdef OrderPendingCancel from_dict_c(dict values):
        return OrderPendingCancel(
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            ts_pending_ns=values["ts_pending_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderPendingCancel obj):
        return {
            "type": "OrderPendingCancel",
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
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.response_to = response_to
        self.reason = reason
        self.ts_rejected_ns = ts_rejected_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"client_order_id={self.client_order_id}, "
                f"venue_order_id={self.venue_order_id}, "
                f"response_to={self.response_to}, "
                f"reason='{self.reason}', "
                f"event_id={self.id})")

    @staticmethod
    cdef OrderUpdateRejected from_dict_c(dict values):
        return OrderUpdateRejected(
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
        return {
            "type": "OrderUpdateRejected",
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
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.response_to = response_to
        self.reason = reason
        self.ts_rejected_ns = ts_rejected_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"client_order_id={self.client_order_id}, "
                f"venue_order_id={self.venue_order_id}, "
                f"response_to={self.response_to}, "
                f"reason='{self.reason}', "
                f"event_id={self.id})")

    @staticmethod
    cdef OrderCancelRejected from_dict_c(dict values):
        return OrderCancelRejected(
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
        return {
            "type": "OrderCancelRejected",
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

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"client_order_id={self.client_order_id}, "
                f"venue_order_id={self.venue_order_id}, "
                f"qty={self.quantity.to_str()}, "
                f"price={self.price}, "
                f"trigger={self.trigger}, "
                f"event_id={self.id})")

    @staticmethod
    cdef OrderUpdated from_dict_c(dict values):
        cdef str t = values["trigger"]
        return OrderUpdated(
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
        return {
            "type": "OrderUpdated",
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
        AccountId account_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        ExecutionId execution_id not None,
        PositionId position_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        OrderSide order_side,
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
        strategy_id : StrategyId
            The strategy ID associated with the order.
        instrument_id : InstrumentId
            The instrument ID.
        order_side : OrderSide
            The execution order side.
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
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.execution_id = execution_id
        self.position_id = position_id
        self.strategy_id = strategy_id
        self.instrument_id = instrument_id
        self.order_side = order_side
        self.last_qty = last_qty
        self.last_px = last_px
        self.currency = currency
        self.commission = commission
        self.liquidity_side = liquidity_side
        self.ts_filled_ns = ts_filled_ns
        self.info = info

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"client_order_id={self.client_order_id}, "
                f"venue_order_id={self.venue_order_id}, "
                f"position_id={self.position_id}, "
                f"strategy_id={self.strategy_id}, "
                f"instrument_id={self.instrument_id}, "
                f"side={OrderSideParser.to_str(self.order_side)}"
                f"-{LiquiditySideParser.to_str(self.liquidity_side)}, "
                f"last_qty={self.last_qty.to_str()}, "
                f"last_px={self.last_px} {self.currency.code}, "
                f"commission={self.commission.to_str()}, "
                f"event_id={self.id})")

    @staticmethod
    cdef OrderFilled from_dict_c(dict values):
        return OrderFilled(
            account_id=AccountId.from_str_c(values["account_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            execution_id=ExecutionId(values["execution_id"]),
            position_id=PositionId(values["position_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            order_side=OrderSideParser.from_str(values["order_side"]),
            last_qty=Quantity.from_str_c(values["last_qty"]),
            last_px=Price.from_str_c(values["last_px"]),
            currency=Currency.from_str_c(values["currency"]),
            commission=Money.from_str_c(values["commission"]),
            liquidity_side=LiquiditySideParser.from_str(values["liquidity_side"]),
            ts_filled_ns=values["ts_filled_ns"],
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
            info=json.loads(values["info"])
        )

    @staticmethod
    cdef dict to_dict_c(OrderFilled obj):
        return {
            "type": "OrderFilled",
            "account_id": obj.account_id.value,
            "client_order_id": obj.client_order_id.value,
            "venue_order_id": obj.venue_order_id.value,
            "execution_id": obj.execution_id.value,
            "position_id": obj.position_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "order_side": OrderSideParser.to_str(obj.order_side),
            "last_qty": str(obj.last_qty),
            "last_px": str(obj.last_px),
            "currency": obj.currency.code,
            "commission": obj.commission.to_str(),
            "liquidity_side": LiquiditySideParser.to_str(obj.liquidity_side),
            "ts_filled_ns": obj.ts_filled_ns,
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
            "info": json.dumps(obj.info),
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
        return self.order_side == OrderSide.BUY

    cdef bint is_sell_c(self) except *:
        return self.order_side == OrderSide.SELL

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


cdef class PositionEvent(Event):
    """
    The abstract base class for all position events.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        PositionId position_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        dict position_status not None,
        OrderFilled order_fill not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``PositionEvent`` class.

        Parameters
        ----------
        position_id : PositionId
            The position ID associated with the event.
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The position instrument ID.
        position_status : dict[str, object]
            The position status.
        order_fill : OrderFilled
            The order fill event which triggered the event.
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        """
        super().__init__(event_id, timestamp_ns)

        self.position_id = position_id
        self.strategy_id = strategy_id
        self.instrument_id = instrument_id
        self.position_status = position_status
        self.order_fill = order_fill

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"position_id={self.position_id}, "
                f"strategy_id={self.strategy_id}, "
                f"instrument_id={self.instrument_id}, "
                f"position_status={self.position_status}, "
                f"event_id={self.id})")


cdef class PositionOpened(PositionEvent):
    """
    Represents an event where a position has been opened.
    """

    def __init__(
        self,
        PositionId position_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        dict position_status not None,
        OrderFilled order_fill not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``PositionOpened`` class.

        Parameters
        ----------
        position_id : PositionId
            The position ID associated with the event.
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The position instrument ID.
        position_status : dict[str, object]
            The position status.
        order_fill : OrderFilled
            The order fill event which triggered the event.
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        """
        assert position_status["side"] != "FLAT"  # Design-time check: position status matched event
        super().__init__(
            position_id,
            strategy_id,
            instrument_id,
            position_status,
            order_fill,
            event_id,
            timestamp_ns,
        )

    @staticmethod
    cdef PositionOpened from_dict_c(dict values):
        return PositionOpened(
            position_id=PositionId(values["position_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            position_status=json.loads(values["position_status"]),
            order_fill=OrderFilled.from_dict_c(json.loads(values["order_fill"])),
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(PositionOpened obj):
        return {
            "type": "PositionOpened",
            "position_id": obj.position_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "position_status": json.dumps(obj.position_status),
            "order_fill": json.dumps(OrderFilled.to_dict_c(obj.order_fill)),
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
        }

    @staticmethod
    def from_dict(dict values):
        """
        Return a position opened event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        PositionOpened

        """
        return PositionOpened.from_dict_c(values)

    @staticmethod
    def to_dict(PositionOpened obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return PositionOpened.to_dict_c(obj)


cdef class PositionChanged(PositionEvent):
    """
    Represents an event where a position has changed.
    """

    def __init__(
        self,
        PositionId position_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        dict position_status not None,
        OrderFilled order_fill not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``PositionChanged`` class.

        Parameters
        ----------
        position_id : PositionId
            The position ID associated with the event.
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The position instrument ID.
        position_status : dict[str, object]
            The position status.
        order_fill : OrderFilled
            The order fill event which triggered the event.
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        """
        assert position_status["side"] != "FLAT"  # Design-time check: position status matched event
        super().__init__(
            position_id,
            strategy_id,
            instrument_id,
            position_status,
            order_fill,
            event_id,
            timestamp_ns,
        )

    @staticmethod
    cdef PositionChanged from_dict_c(dict values):
        return PositionChanged(
            position_id=PositionId(values["position_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            position_status=json.loads(values["position_status"]),
            order_fill=OrderFilled.from_dict_c(json.loads(values["order_fill"])),
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(PositionChanged obj):
        return {
            "type": "PositionChanged",
            "position_id": obj.position_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "position_status": json.dumps(obj.position_status),
            "order_fill": json.dumps(OrderFilled.to_dict_c(obj.order_fill)),
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
        }

    @staticmethod
    def from_dict(dict values):
        """
        Return a position changed event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        PositionChanged

        """
        return PositionChanged.from_dict_c(values)

    @staticmethod
    def to_dict(PositionChanged obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return PositionChanged.to_dict_c(obj)


cdef class PositionClosed(PositionEvent):
    """
    Represents an event where a position has been closed.
    """

    def __init__(
        self,
        PositionId position_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        dict position_status not None,
        OrderEvent order_fill not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``PositionClosed`` class.

        Parameters
        ----------
        position_id : PositionId
            The position ID associated with the event.
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The position instrument ID.
        position_status : dict[str, object]
            The position status.
        order_fill : OrderEvent
            The order fill event which triggered the event.
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        """
        assert position_status["side"] == "FLAT"  # Design-time check: position status matched event
        super().__init__(
            position_id,
            strategy_id,
            instrument_id,
            position_status,
            order_fill,
            event_id,
            timestamp_ns,
        )

    @staticmethod
    cdef PositionClosed from_dict_c(dict values):
        return PositionClosed(
            position_id=PositionId(values["position_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            position_status=json.loads(values["position_status"]),
            order_fill=OrderFilled.from_dict_c(json.loads(values["order_fill"])),
            event_id=UUID.from_str_c(values["event_id"]),
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(PositionClosed obj):
        return {
            "type": "PositionClosed",
            "position_id": obj.position_id.value,
            "strategy_id": obj.strategy_id.value,
            "instrument_id": obj.instrument_id.value,
            "position_status": json.dumps(obj.position_status),
            "order_fill": json.dumps(OrderFilled.to_dict_c(obj.order_fill)),
            "event_id": obj.id.value,
            "timestamp_ns": obj.timestamp_ns,
        }

    @staticmethod
    def from_dict(dict values):
        """
        Return a position closed event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        PositionClosed

        """
        return PositionClosed.from_dict_c(values)

    @staticmethod
    def to_dict(PositionClosed obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return PositionClosed.to_dict_c(obj)
