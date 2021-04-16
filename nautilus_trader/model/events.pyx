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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID
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


from nautilus_trader.model.c_enums.instrument_close_type cimport InstrumentCloseType  # isort:skip
from nautilus_trader.model.c_enums.instrument_close_type cimport InstrumentCloseTypeParser  # isort:skip


cdef class AccountState(Event):
    """
    Represents an event which includes information on the state of the account.
    """

    def __init__(
        self,
        AccountId account_id not None,
        list balances not None,
        list balances_free not None,
        list balances_locked not None,
        dict info not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the `AccountState` class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        balances : list[Money]
            The current account balances.
        balances_free : list[Money]
            The account balances free for trading.
        balances_locked : list[Money]
            The account balances locked (assigned as margin collateral).
        info : dict [str, object]
            The additional implementation specific account information.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

        """
        super().__init__(event_id, timestamp_ns)

        self.account_id = account_id
        self.balances = balances
        self.balances_free = balances_free
        self.balances_locked = balances_locked
        self.info = info

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id.value}, "
                f"free=[{', '.join([b.to_str() for b in self.balances_free])}], "
                f"locked=[{', '.join([b.to_str() for b in self.balances_locked])}], "
                f"event_id={self.id})")


cdef class OrderEvent(Event):
    """
    The abstract base class for all order events.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(
        self,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the `OrderEvent` base class.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order identifier.
        venue_order_id : VenueOrderId
            The venue order identifier.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

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
        Initialize a new instance of the `OrderInitialized` class.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order identifier.
        strategy_id : StrategyId
            The strategy identifier associated with the order.
        instrument_id : InstrumentId
            The order instrument identifier.
        order_side : OrderSide (Enum)
            The order side.
        order_type : OrderType (Enum)
            The order type.
        quantity : Quantity
            The order quantity.
        time_in_force : TimeInForce (Enum)
            The order time-in-force.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.
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
        Initialize a new instance of the `OrderInvalid` class.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order identifier.
        reason : str
            The order invalid reason.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

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
                f"reason='{self.reason}', "
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
        Initialize a new instance of the `OrderDenied` class.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order identifier.
        reason : str
            The order denied reason.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

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
                f"reason='{self.reason}', "
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
        int64_t submitted_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the `OrderSubmitted` class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        submitted_ns : int64
            The Unix timestamp (nanos) when the order was submitted.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

        """
        super().__init__(
            client_order_id,
            VenueOrderId.null_c(),  # Pending accepted
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.submitted_ns = submitted_ns

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
        int64_t rejected_ns,
        str reason not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the `OrderRejected` class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        rejected_ns : int64
            The order rejected time.
        reason : datetime
            The order rejected reason.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

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
        self.rejected_ns = rejected_ns
        self.reason = reason

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
    https://www.onixs.biz/fix-dictionary/4.4/tagNum_39.html

    """

    def __init__(
        self,
        AccountId account_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        int64_t accepted_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the `OrderAccepted` class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        venue_order_id : VenueOrderId
            The venue order identifier.
        accepted_ns : int64
            The order accepted time.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

        Raises
        ------
        ValueError
            If order_id has a 'NULL' value.

        """
        Condition.true(venue_order_id.not_null(), "venue_order_id was 'NULL'")
        super().__init__(
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.accepted_ns = accepted_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"client_order_id={self.client_order_id}, "
                f"venue_order_id={self.venue_order_id}, "
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
        int64_t rejected_ns,
        str response_to not None,
        str reason not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the `OrderUpdateRejected` class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        venue_order_id : VenueOrderId
            The venue order identifier.
        rejected_ns : datetime
            The order update rejected time.
        response_to : str
            The order update rejected response.
        reason : str
            The order update rejected reason.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

        Raises
        ------
        ValueError
            If order_id has a 'NULL' value.
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
        self.rejected_ns = rejected_ns
        self.response_to = response_to
        self.reason = reason

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
        int64_t rejected_ns,
        str response_to not None,
        str reason not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the `OrderCancelRejected` class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        venue_order_id : VenueOrderId
            The venue order identifier.
        rejected_ns : datetime
            The order cancel rejected time.
        response_to : str
            The order cancel rejected response.
        reason : str
            The order cancel rejected reason.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

        Raises
        ------
        ValueError
            If order_id has a 'NULL' value.
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
        self.rejected_ns = rejected_ns
        self.response_to = response_to
        self.reason = reason

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"client_order_id={self.client_order_id}, "
                f"response_to={self.response_to}, "
                f"reason='{self.reason}', "
                f"event_id={self.id})")


cdef class OrderCancelled(OrderEvent):
    """
    Represents an event where an order has been cancelled at the trading venue.
    """

    def __init__(
        self,
        AccountId account_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        int64_t cancelled_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the `OrderCancelled` class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        venue_order_id : VenueOrderId
            The venue order identifier.
        cancelled_ns : int64
            The event order cancelled time.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

        Raises
        ------
        ValueError
            If order_id has a 'NULL' value.

        """
        Condition.true(venue_order_id.not_null(), "venue_order_id was 'NULL'")
        super().__init__(
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.cancelled_ns = cancelled_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"client_order_id={self.client_order_id}, "
                f"venue_order_id={self.venue_order_id}, "
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
        int64_t updated_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the `OrderUpdated` class.

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
        updated_ns : int64
            The updated time.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

        Raises
        ------
        ValueError
            If order_id has a 'NULL' value.

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
        self.updated_ns = updated_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"cl_order_id={self.client_order_id}, "
                f"venue_order_id={self.venue_order_id}, "
                f"qty={self.quantity.to_str()}, "
                f"price={self.price}, "
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
        int64_t expired_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the `OrderExpired` class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        venue_order_id : VenueOrderId
            The venue order identifier.
        expired_ns : int64
            The order expired time.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

        Raises
        ------
        ValueError
            If order_id has a 'NULL' value.

        """
        Condition.true(venue_order_id.not_null(), "venue_order_id was 'NULL'")
        super().__init__(
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.expired_ns = expired_ns

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
        int64_t triggered_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the `OrderTriggered` class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        client_order_id : ClientOrderId
            The client order identifier.
        venue_order_id : VenueOrderId
            The venue order identifier.
        triggered_ns : int64
            The order triggered time.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

        Raises
        ------
        ValueError
            If order_id has a 'NULL' value.

        """
        Condition.true(venue_order_id.not_null(), "venue_order_id was 'NULL'")
        super().__init__(
            client_order_id,
            venue_order_id,
            event_id,
            timestamp_ns,
        )

        self.account_id = account_id
        self.triggered_ns = triggered_ns

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
        Quantity cum_qty not None,
        Quantity leaves_qty not None,
        Currency currency not None,
        bint is_inverse,
        Money commission not None,
        LiquiditySide liquidity_side,
        int64_t execution_ns,
        UUID event_id not None,
        int64_t timestamp_ns,
        dict info=None,
    ):
        """
        Initialize a new instance of the `OrderFilled` class.

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
        order_side : OrderSide (Enum)
            The execution order side.
        last_qty : Quantity
            The fill quantity for this execution.
        last_px : Price
            The fill price for this execution (not average price).
        cum_qty : Quantity
            The cumulative filled quantity for the order.
        leaves_qty : Quantity
            The order quantity open for further execution.
        currency : Currency
            The currency of the price.
        is_inverse : bool
            If quantity is expressed in quote currency.
        commission : Money
            The fill commission.
        liquidity_side : LiquiditySide (Enum)
            The execution liquidity side.
        execution_ns : int64
            The Unix timestamp (nanos) of the execution.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.
        info : dict[str, object], optional
            The additional fill information.

        Raises
        ------
        ValueError
            If order_id has a 'NULL' value.

        """
        Condition.true(venue_order_id.not_null(), "venue_order_id was 'NULL'")
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
        self.cum_qty = cum_qty
        self.leaves_qty = leaves_qty
        self.currency = currency
        self.is_inverse = is_inverse
        self.commission = commission
        self.liquidity_side = liquidity_side
        self.execution_ns = execution_ns
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
                f"cum_qty={self.cum_qty.to_str()}, "
                f"leaves_qty={self.leaves_qty.to_str()}, "
                f"commission={self.commission.to_str()}, "
                f"event_id={self.id})")


cdef class PositionEvent(Event):
    """
    The abstract base class for all position events.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(
        self,
        Position position not None,
        OrderFilled order_fill not None,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the `PositionEvent` class.

        Parameters
        ----------
        position : Position
            The position.
        order_fill : OrderFilled
            The order fill event which triggered the event.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

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
        Initialize a new instance of the `PositionOpened` class.

        Parameters
        ----------
        position : Position
            The position.
        order_fill : OrderFilled
            The order fill event which triggered the event.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

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
        Initialize a new instance of the `PositionChanged` class.

        Parameters
        ----------
        position : Position
            The position.
        order_fill : OrderFilled
            The order fill event which triggered the event.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

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
        Initialize a new instance of the `PositionClosed` class.

        Parameters
        ----------
        position : Position
            The position.
        order_fill : OrderEvent
            The order fill event which triggered the event.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

        """
        assert position.is_closed_c()  # Design-time check
        super().__init__(
            position,
            order_fill,
            event_id,
            timestamp_ns,
        )

    def __repr__(self) -> str:
        cdef str duration = str(self.position.open_duration_ns).replace("0 days ", "", 1)
        return (f"{type(self).__name__}("
                f"account_id={self.position.account_id}, "
                f"position_id={self.position.id}, "
                f"strategy_id={self.position.strategy_id}, "
                f"entry={OrderSideParser.to_str(self.position.entry)}, "
                f"duration={duration}, "
                f"avg_px_open={self.position.avg_px_open}, "
                f"avg_px_close={self.position.avg_px_close}, "
                f"realized_points={round(self.position.realized_points, 5)}, "
                f"realized_return={round(self.position.realized_return * 100, 3)}%, "
                f"realized_pnl={self.position.realized_pnl.to_str()}, "
                f"event_id={self.id})")


cdef class StatusEvent(Event):
    """
    The abstract base class for all status events.

    This class should not be used directly, but through its concrete subclasses.
    """
    def __init__(
        self,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the `StatusEvent` base class.

        Parameters
        ----------
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

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
        Initialize a new instance of the `VenueStatusEvent` base class.

        Parameters
        ----------
        status : VenueStatus
            The venue status.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

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
        Initialize a new instance of the `InstrumentStatusEvent` base class.

        Parameters
        ----------
        status : InstrumentStatus
            The instrument status.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

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
        Initialize a new instance of the `InstrumentClosePrice` base class.

        Parameters
        ----------
        close_price : Price
            The closing price for the instrument.
        close_type : InstrumentCloseType
            The type of closing price.
        event_id : UUID
            The event identifier.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the event initialization.

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
