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

import pandas as pd
from libc.stdint cimport int64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.account_type cimport AccountTypeParser
from nautilus_trader.model.c_enums.instrument_status cimport InstrumentStatus
from nautilus_trader.model.c_enums.instrument_status cimport InstrumentStatusParser
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySideParser
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.venue_status cimport VenueStatus
from nautilus_trader.model.c_enums.venue_status cimport VenueStatusParser
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.position cimport Position


# These imports are currently being skipped from sorting as isort 5.8 was breaking on them
from nautilus_trader.model.c_enums.instrument_close_type cimport InstrumentCloseType  # isort:skip
from nautilus_trader.model.c_enums.instrument_close_type cimport InstrumentCloseTypeParser  # isort:skip


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
            The account identifier.
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
            The event identifier.
        ts_updated_ns : int64
            The UNIX timestamp (nanos) when the account was updated.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

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
            The client order identifier.
        venue_order_id : VenueOrderId
            The venue order identifier.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

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
            The client order identifier.
        strategy_id : StrategyId
            The strategy identifier associated with the order.
        instrument_id : InstrumentId
            The order instrument identifier.
        order_side : OrderSide
            The order side.
        order_type : OrderType
            The order type.
        quantity : Quantity
            The order quantity.
        time_in_force : TimeInForce
            The order time-in-force.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) when the order was initialized.
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


cdef class OrderInvalid(OrderEvent):
    """
    Represents an event where an order has been invalidated by the Nautilus
    system.

    This could be due to a duplicated identifier, invalid combination of
    parameters, or for any other reason that the order is considered to be
    invalid.
    """

    def __init__(
        self,
        ClientOrderId client_order_id not None,
        str reason not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``OrderInvalid`` class.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order identifier.
        reason : str
            The order invalid reason.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

        Raises
        ------
        ValueError
            If invalid_reason is not a valid_string.

        """
        Condition.valid_string(reason, "invalid_reason")
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
            The client order identifier.
        reason : str
            The order denied reason.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

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
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        ts_submitted_ns : int64
            The UNIX timestamp (nanos) when the order was submitted.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

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
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        reason : datetime
            The order rejected reason.
        ts_rejected_ns : int64
            The UNIX timestamp (nanos) when the order was rejected.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

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
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        venue_order_id : VenueOrderId
            The venue order identifier.
        ts_accepted_ns : int64
            The UNIX timestamp (nanos) when the order was accepted.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

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


cdef class OrderPendingReplace(OrderEvent):
    """
    Represents an event where a `UpdateOrder` command has been sent to the
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
        Initialize a new instance of the ``OrderPendingReplace`` class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        venue_order_id : VenueOrderId
            The venue order identifier.
        ts_pending_ns : datetime
            The UNIX timestamp (nanos) when the replace was pending.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

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
                f"ts_pending_ns={self.ts_pending_ns}, "
                f"event_id={self.id})")


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
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        venue_order_id : VenueOrderId
            The venue order identifier.
        ts_pending_ns : datetime
            The UNIX timestamp (nanos) when the cancel was pending.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

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
                f"ts_pending_ns={self.ts_pending_ns}, "
                f"event_id={self.id})")


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
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        venue_order_id : VenueOrderId
            The venue order identifier.
        response_to : str
            The order update rejected response.
        reason : str
            The order update rejected reason.
        ts_rejected_ns : datetime
            The UNIX timestamp (nanos) when the order update was rejected.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

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
                f"response_to={self.response_to}, "
                f"reason='{self.reason}', "
                f"event_id={self.id})")


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
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        venue_order_id : VenueOrderId
            The venue order identifier.
        response_to : str
            The order cancel rejected response.
        reason : str
            The order cancel rejected reason.
        ts_rejected_ns : datetime
            The UNIX timestamp (nanos) when the order cancel was rejected.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

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
                f"response_to={self.response_to}, "
                f"reason='{self.reason}', "
                f"event_id={self.id})")


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
        int64_t ts_updated_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``OrderUpdated`` class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        venue_order_id : VenueOrderId
            The venue order identifier.
        quantity : Quantity
            The orders current quantity.
        price : Price
            The orders current price.
        ts_updated_ns : int64
            The UNIX timestamp (nanos) when the order was updated.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

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
        self.ts_updated_ns = ts_updated_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"cl_order_id={self.client_order_id}, "
                f"venue_order_id={self.venue_order_id}, "
                f"qty={self.quantity.to_str()}, "
                f"price={self.price}, "
                f"event_id={self.id})")


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
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        venue_order_id : VenueOrderId
            The venue order identifier.
        ts_canceled_ns : int64
            The UNIX timestamp (nanos) when order was canceled.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

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
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        venue_order_id : VenueOrderId
            The venue order identifier.
        ts_triggered_ns : int64
            The UNIX timestamp (nanos) when the order was triggered.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

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
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        venue_order_id : VenueOrderId
            The venue order identifier.
        ts_expired_ns : int64
            The UNIX timestamp (nanos) when the order expired.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

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
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        venue_order_id : VenueOrderId
            The venue order identifier.
        execution_id : ExecutionId
            The execution identifier.
        position_id : PositionId
            The position identifier associated with the order.
        strategy_id : StrategyId
            The strategy identifier associated with the order.
        instrument_id : InstrumentId
            The instrument identifier.
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
            The UNIX timestamp (nanos) when the order was filled.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.
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
                f"last_px={self.last_px}, "
                f"commission={self.commission.to_str()}, "
                f"event_id={self.id})")

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
        Position position not None,
        OrderFilled order_fill not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``PositionEvent`` class.

        Parameters
        ----------
        position : Position
            The position.
        order_fill : OrderFilled
            The order fill event which triggered the event.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

        """
        super().__init__(event_id, timestamp_ns)

        self.position = position
        self.order_fill = order_fill


cdef class PositionOpened(PositionEvent):
    """
    Represents an event where a position has been opened.
    """

    def __init__(
        self,
        Position position not None,
        OrderFilled order_fill not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``PositionOpened`` class.

        Parameters
        ----------
        position : Position
            The position.
        order_fill : OrderFilled
            The order fill event which triggered the event.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

        """
        assert position.is_open_c()  # Design-time check
        super().__init__(
            position,
            order_fill,
            event_id,
            timestamp_ns,
        )

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"{self.position.status_string_c()}, "
                f"account_id={self.position.account_id}, "
                f"position_id={self.position.id}, "
                f"strategy_id={self.position.strategy_id}, "
                f"entry={OrderSideParser.to_str(self.position.entry)}, "
                f"avg_px_open={round(self.position.avg_px_open, 5)}, "
                f"{self.position.status_string_c()}, "
                f"event_id={self.id})")


cdef class PositionChanged(PositionEvent):
    """
    Represents an event where a position has changed.
    """

    def __init__(
        self,
        Position position not None,
        OrderFilled order_fill not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``PositionChanged`` class.

        Parameters
        ----------
        position : Position
            The position.
        order_fill : OrderFilled
            The order fill event which triggered the event.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

        """
        assert position.is_open_c()  # Design-time check
        super().__init__(
            position,
            order_fill,
            event_id,
            timestamp_ns,
        )

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"{self.position.status_string_c()}, "
                f"account_id={self.position.account_id}, "
                f"position_id={self.position.id}, "
                f"strategy_id={self.position.strategy_id}, "
                f"entry={OrderSideParser.to_str(self.position.entry)}, "
                f"avg_px_open={self.position.avg_px_open}, "
                f"realized_points={self.position.realized_points}, "
                f"realized_return={round(self.position.realized_return * 100, 3)}%, "
                f"realized_pnl={self.position.realized_pnl.to_str()}, "
                f"{self.position.status_string_c()}, "
                f"event_id={self.id})")


cdef class PositionClosed(PositionEvent):
    """
    Represents an event where a position has been closed.
    """

    def __init__(
        self,
        Position position not None,
        OrderEvent order_fill not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``PositionClosed`` class.

        Parameters
        ----------
        position : Position
            The position.
        order_fill : OrderEvent
            The order fill event which triggered the event.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

        """
        assert position.is_closed_c()  # Design-time check
        super().__init__(
            position,
            order_fill,
            event_id,
            timestamp_ns,
        )

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"{self.position.status_string_c()}, "
                f"account_id={self.position.account_id}, "
                f"position_id={self.position.id}, "
                f"strategy_id={self.position.strategy_id}, "
                f"entry={OrderSideParser.to_str(self.position.entry)}, "
                f"duration={pd.Timedelta(self.position.open_duration_ns, unit='ns')}, "
                f"avg_px_open={self.position.avg_px_open}, "
                f"avg_px_close={self.position.avg_px_close}, "
                f"realized_points={round(self.position.realized_points, 5)}, "
                f"realized_return={round(self.position.realized_return * 100, 3)}%, "
                f"realized_pnl={self.position.realized_pnl.to_str()}, "
                f"event_id={self.id})")


cdef class StatusEvent(Event):
    """
    The abstract base class for all status events.

    This class should not be used directly, but through a concrete subclass.
    """
    def __init__(
        self,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``StatusEvent` base class.

        Parameters
        ----------
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

        """
        super().__init__(event_id, timestamp_ns)


cdef class VenueStatusEvent(StatusEvent):
    """
    Represents an event that indicates a change in a Venue status.
    """
    def __init__(
        self,
        Venue venue,
        VenueStatus status,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``VenueStatusEvent` base class.

        Parameters
        ----------
        status : VenueStatus
            The venue status.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

        """
        super().__init__(event_id, timestamp_ns)
        self.venue = venue
        self.status = status

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"venue={self.venue}, "
                f"status={VenueStatusParser.to_str(self.status)}, "
                f"event_id={self.id})")


cdef class InstrumentStatusEvent(StatusEvent):
    """
    Represents an event that indicates a change in an instrument status.
    """
    def __init__(
        self,
        InstrumentId instrument_id,
        InstrumentStatus status,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``InstrumentStatusEvent` base class.

        Parameters
        ----------
        status : InstrumentStatus
            The instrument status.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

        """
        super().__init__(event_id, timestamp_ns)
        self.instrument_id = instrument_id
        self.status = status

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"instrument_id={self.instrument_id}, "
                f"status={InstrumentStatusParser.to_str(self.status)}, "
                f"event_id={self.id})")


cdef class InstrumentClosePrice(Event):
    """
    Represents an event that indicates a change in an instrument status.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Price close_price not None,
        InstrumentCloseType close_type,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``InstrumentClosePrice` base class.

        Parameters
        ----------
        close_price : Price
            The closing price for the instrument.
        close_type : InstrumentCloseType
            The type of closing price.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

        """
        super().__init__(event_id, timestamp_ns)
        self.instrument_id = instrument_id
        self.close_price = close_price
        self.close_type = close_type

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"instrument_id={self.instrument_id}, "
                f"close_price={self.close_price}, "
                f"close_type={InstrumentCloseTypeParser.to_str(self.close_type)}, "
                f"event_id={self.id})")
