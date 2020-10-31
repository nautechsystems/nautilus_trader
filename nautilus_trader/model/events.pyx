# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side cimport liquidity_side_to_string
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport order_side_to_string
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_type cimport order_type_to_string
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport time_in_force_to_string
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.position cimport Position


cdef class AccountState(Event):
    """
    Represents an event which includes information on the state of the account.
    """

    def __init__(
            self,
            AccountId account_id not None,
            Currency currency not None,
            Money balance not None,
            Money margin_balance not None,
            Money margin_available not None,
            UUID event_id not None,
            datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the AccountState class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        currency : Currency
            The currency for the account.
        balance : Money
            The account balance.
        margin_balance : Money
            The account margin balance.
        margin_available : Money
            The account margin available.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(event_id, event_timestamp)

        self._account_id = account_id
        self._currency = currency
        self._balance = balance
        self._margin_balance = margin_balance
        self._margin_available = margin_available

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self._account_id.value}, "
                f"balance={self._balance.to_string()})")

    @property
    def account_id(self):
        """
        The account identifier associated with the event.

        Returns
        -------
        AccountId

        """
        return self._account_id

    @property
    def currency(self):
        """
        The currency of the event.

        Returns
        -------
        Currency

        """
        return self._currency

    @property
    def balance(self):
        """
        The account balance of the event.

        Returns
        -------
        Money

        """
        return self._balance

    @property
    def margin_balance(self):
        """
        The margin balance of the event.

        Returns
        -------
        Money

        """
        return self._margin_balance

    @property
    def margin_available(self):
        """
        The margin available of the event.

        Returns
        -------
        Money

        """
        return self._margin_available


cdef class OrderEvent(Event):
    """
    The base class for all order events.
    """

    def __init__(
            self,
            ClientOrderId cl_ord_id not None,
            UUID event_id not None,
            datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the OrderEvent base class.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(event_id, event_timestamp)

        self._cl_ord_id = cl_ord_id

    def __repr__(self) -> str:
        return f"{type(self).__name__}(cl_ord_id={self._cl_ord_id}, id={self._id})"

    @property
    def cl_ord_id(self):
        """
        Returns
        -------
        ClientOrderId
            The client order identifier associated with the event.

        """
        return self._cl_ord_id

    @property
    def is_completion_trigger(self):
        """
        If this event represents an `Order` completion trigger (where an order
        will subsequently be considered `completed` when this event is applied).

        Returns
        -------
        bool
            True if completion trigger, else False.

        """
        return self._is_completion_trigger


cdef class OrderInitialized(OrderEvent):
    """
    Represents an event where an order has been initialized.
    """

    def __init__(
            self,
            ClientOrderId cl_ord_id not None,
            StrategyId strategy_id not None,
            Symbol symbol not None,
            OrderSide order_side,
            OrderType order_type,
            Quantity quantity not None,
            TimeInForce time_in_force,
            UUID event_id not None,
            datetime event_timestamp not None,
            dict options not None,
    ):
        """
        Initialize a new instance of the OrderInitialized class.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.
        strategy_id : ClientOrderId
            The strategy identifier.
        symbol : Symbol
            The order symbol.
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
        event_timestamp : datetime
            The event timestamp.
        options : dict[str, str]
            The order initialization options. Contains mappings for specific order parameters.

        Raises
        ------
        ValueError
            If order_side is UNDEFINED.
        ValueError
            If order_type is UNDEFINED.
        ValueError
            If time_in_force is UNDEFINED.

        """
        Condition.not_equal(order_side, OrderSide.UNDEFINED, "order_side", "UNDEFINED")
        Condition.not_equal(order_type, OrderType.UNDEFINED, "order_type", "UNDEFINED")
        Condition.not_equal(time_in_force, TimeInForce.UNDEFINED, "time_in_force", "UNDEFINED")
        super().__init__(
            cl_ord_id,
            event_id,
            event_timestamp,
        )

        self._cl_ord_id = cl_ord_id
        self._strategy_id = strategy_id
        self._symbol = symbol
        self._order_side = order_side
        self._order_type = order_type
        self._quantity = quantity
        self._time_in_force = time_in_force
        self._options = options
        self._is_completion_trigger = False

    @property
    def strategy_id(self):
        """
        The strategy identifier associated with the event.

        Returns
        -------
        StrategyId

        """
        return self._strategy_id

    @property
    def symbol(self):
        """
        The order symbol of the event.

        Returns
        -------
        Symbol

        """
        return self._symbol

    @property
    def order_side(self):
        """
        The order side of the event.

        Returns
        -------
        OrderSide

        """
        return self._order_side

    @property
    def order_type(self):
        """
        The order type of the event.

        Returns
        -------
        OrderType

        """
        return self._order_type

    @property
    def quantity(self):
        """
        The order quantity of the event.

        Returns
        -------
        Quantity

        """
        return self._quantity

    @property
    def time_in_force(self):
        """
        The order time-in-force of the event.

        Returns
        -------
        TimeInForce

        """
        return self._time_in_force

    @property
    def options(self):
        """
        The order initialization options of the event.

        Returns
        -------
        dict

        """
        return self._options


cdef class OrderInvalid(OrderEvent):
    """
    Represents an event where an order has been invalidated by the Nautilus
    system.
    """

    def __init__(
            self,
            ClientOrderId cl_ord_id not None,
            str reason not None,
            UUID event_id not None,
            datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the OrderInvalid class.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.
        reason : str
            The order invalid reason.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        Raises
        ------
        ValueError
            If invalid_reason is not a valid_string.

        """
        Condition.valid_string(reason, "invalid_reason")
        super().__init__(
            cl_ord_id,
            event_id,
            event_timestamp,
        )

        self._reason = reason
        self._is_completion_trigger = True

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"cl_ord_id={self._cl_ord_id}, "
                f"reason={self._reason}, "
                f"id={self._id})")

    @property
    def reason(self):
        """
        The reason the order was considered invalid.

        Returns
        -------
        str

        """
        return self._reason


cdef class OrderDenied(OrderEvent):
    """
    Represents an event where an order has been denied by the Nautilus system.
    """

    def __init__(
            self,
            ClientOrderId cl_ord_id not None,
            str reason not None,
            UUID event_id not None,
            datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the OrderDenied class.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.
        reason : str
            The order denied reason.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        Raises
        ------
        ValueError
            If denied_reason is not a valid_string.

        """
        Condition.valid_string(reason, "denied_reason")
        super().__init__(
            cl_ord_id,
            event_id,
            event_timestamp,
        )

        self._reason = reason
        self._is_completion_trigger = True

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"cl_ord_id={self._cl_ord_id}, "
                f"reason={self._reason}, "
                f"id={self._id})")

    @property
    def reason(self):
        """
        The reason the order was denied.

        Returns
        -------
        str

        """
        return self._reason


cdef class OrderSubmitted(OrderEvent):
    """
    Represents an event where an order has been submitted by the system to the
    exchange/broker.
    """

    def __init__(
            self,
            AccountId account_id not None,
            ClientOrderId cl_ord_id not None,
            datetime submitted_time not None,
            UUID event_id not None,
            datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the OrderSubmitted class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        submitted_time : datetime
            The order submitted time.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(
            cl_ord_id,
            event_id,
            event_timestamp,
        )

        self._account_id = account_id
        self._submitted_time = submitted_time
        self._is_completion_trigger = False

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self._account_id}, "
                f"cl_ord_id={self._cl_ord_id}, "
                f"id={self._id})")

    @property
    def account_id(self):
        """
        Returns
        -------
        AccountId
            The account identifier associated with the event.

        """
        return self._account_id

    @property
    def submitted_time(self):
        """
        Returns
        -------
        datetime
            The order submitted time of the event.

        """
        return self._submitted_time


cdef class OrderRejected(OrderEvent):
    """
    Represents an event where an order has been rejected by the exchange/broker.
    """

    def __init__(
            self,
            AccountId account_id not None,
            ClientOrderId cl_ord_id not None,
            datetime rejected_time not None,
            str reason not None,
            UUID event_id not None,
            datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the OrderRejected class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        rejected_time : datetime
            The order rejected time.
        reason : datetime
            The order rejected reason.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        Raises
        ------
        ValueError
            If rejected_reason is not a valid_string.

        """
        Condition.valid_string(reason, "rejected_reason")
        super().__init__(
            cl_ord_id,
            event_id,
            event_timestamp,
        )

        self._account_id = account_id
        self._rejected_time = rejected_time
        self._reason = reason
        self._is_completion_trigger = True

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self._account_id}, "
                f"cl_ord_id={self._cl_ord_id}, "
                f"reason={self._reason}, "
                f"id={self._id})")

    @property
    def account_id(self):
        """
        The account identifier associated with the event.

        Returns
        -------
        AccountId

        """
        return self._account_id

    @property
    def rejected_time(self):
        """
        The order rejected time of the event.

        Returns
        -------
        datetime

        """
        return self._rejected_time

    @property
    def reason(self):
        """
        The reason the order was rejected.

        Returns
        -------
        str

        """
        return self._reason


cdef class OrderAccepted(OrderEvent):
    """
    Represents an event where an order has been accepted by the exchange/broker.
    """

    def __init__(
            self,
            AccountId account_id not None,
            ClientOrderId cl_ord_id not None,
            OrderId order_id not None,
            datetime accepted_time not None,
            UUID event_id not None,
            datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the OrderAccepted class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        order_id : OrderId
            The exchange/broker order identifier.
        accepted_time : datetime
            The order accepted time.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(
            cl_ord_id,
            event_id,
            event_timestamp,
        )

        self._account_id = account_id
        self._order_id = order_id
        self._accepted_time = accepted_time
        self._is_completion_trigger = False

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self._account_id}, "
                f"cl_ord_id={self._cl_ord_id}, "
                f"order_id={self._order_id}, "
                f"id={self._id})")

    @property
    def account_id(self):
        """
        Returns
        -------
        AccountId
            The account identifier associated with the event.

        """
        return self._account_id

    @property
    def order_id(self):
        """
        Returns
        -------
        OrderId
            The order identifier associated with the event.

        """
        return self._order_id

    @property
    def accepted_time(self):
        """
        Returns
        -------
        datetime
            The order accepted time of the event.

        """
        return self._accepted_time


cdef class OrderWorking(OrderEvent):
    """
    Represents an event where an order is working with the exchange/broker.
    """

    def __init__(
            self,
            AccountId account_id not None,
            ClientOrderId cl_ord_id not None,
            OrderId order_id not None,
            Symbol symbol not None,
            OrderSide order_side,
            OrderType order_type,
            Quantity quantity not None,
            Price price not None,
            TimeInForce time_in_force,
            datetime expire_time,  # Can be None
            datetime working_time not None,
            UUID event_id not None,
            datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the OrderWorking class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        order_id : OrderId
            The exchange/broker order identifier.
        symbol : Symbol
            The order symbol.
        order_side : OrderSide
            The order side.
        order_type : OrderType
            The order type.
        quantity : Quantity
            The order quantity.
        price : Price
            The order price.
        time_in_force : TimeInForce
            The order time-in-force.
        expire_time : datetime, optional
            The order expire time (for GTD orders only).
        working_time : datetime
            The order working time.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        Raises
        ------
        ValueError
            If order_side is UNDEFINED.
        ValueError
            If order_type is UNDEFINED.
        ValueError
            If time_in_force is UNDEFINED.

        """
        Condition.not_equal(order_side, OrderSide.UNDEFINED, "order_side", "UNDEFINED")
        Condition.not_equal(order_type, OrderType.UNDEFINED, "order_type", "UNDEFINED")
        Condition.not_equal(time_in_force, TimeInForce.UNDEFINED, "time_in_force", "UNDEFINED")
        Condition.type_or_none(expire_time, datetime, "expire_time")

        super().__init__(
            cl_ord_id,
            event_id,
            event_timestamp,
        )

        self._account_id = account_id
        self._order_id = order_id
        self._symbol = symbol
        self._order_side = order_side
        self._order_type = order_type
        self._quantity = quantity
        self._price = price
        self._time_in_force = time_in_force
        self._expire_time = expire_time
        self._working_time = working_time
        self._is_completion_trigger = False

    def __repr__(self) -> str:
        cdef str expire_time = "" if self._expire_time is None else f" {format_iso8601(self._expire_time)}"
        return (f"{type(self).__name__}("
                f"account_id={self._account_id}, "
                f"cl_ord_id={self._cl_ord_id}, "
                f"order_id={self._order_id}, "
                f"{order_side_to_string(self._order_side)} {self._quantity.to_string()} "
                f"{self._symbol} {order_type_to_string(self._order_type)} @ "
                f"{self._price} {time_in_force_to_string(self._time_in_force)}{expire_time}, "
                f"id={self._id})")

    @property
    def account_id(self):
        """
        The account identifier associated with the event.

        Returns
        -------
        AccountId

        """
        return self._account_id

    @property
    def order_id(self):
        """
        The order identifier associated with the event.

        Returns
        -------
        OrderId

        """
        return self._order_id

    @property
    def symbol(self):
        """
        The order symbol of the event.

        Returns
        -------
        Symbol

        """
        return self._symbol

    @property
    def order_side(self):
        """
        The order symbol of the event.

        Returns
        -------
        datetime

        """
        return self._order_side

    @property
    def order_type(self):
        """
        The order type of the event.

        Returns
        -------
        OrderType

        """
        return self._order_type

    @property
    def quantity(self):
        """
        The order quantity of the event.

        Returns
        -------
        Quantity

        """
        return self._quantity

    @property
    def price(self):
        """
        The order price of the event.

        Returns
        -------
        Price

        """
        return self._price

    @property
    def time_in_force(self):
        """
        The order time-in-force of the event.

        Returns
        -------
        TimeInForce

        """
        return self._time_in_force

    @property
    def expire_time(self):
        """
        The order expire time of the event.

        Returns
        -------
        datetime or None

        """
        return self._expire_time

    @property
    def working_time(self):
        """
        The order working time of the event.

        Returns
        -------
        datetime

        """
        return self._working_time


cdef class OrderCancelReject(OrderEvent):
    """
    Represents an event where an order cancel or modify command has been
    rejected by the exchange/broker.
    """

    def __init__(
            self,
            AccountId account_id not None,
            ClientOrderId cl_ord_id not None,
            datetime rejected_time not None,
            str response_to not None,
            str reason not None,
            UUID event_id not None,
            datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the OrderCancelReject class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        rejected_time : datetime
            The order cancel reject time.
        response_to : str
            The order cancel reject response.
        reason : str
            The order cancel reject reason.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

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
            cl_ord_id,
            event_id,
            event_timestamp,
        )

        self._account_id = account_id
        self._rejected_time = rejected_time
        self._response_to = response_to
        self._reason = reason
        self._is_completion_trigger = False

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self._account_id}, "
                f"cl_ord_id={self._cl_ord_id}, "
                f"response_to={self._response_to}, "
                f"reason={self._reason}, "
                f"id={self._id})")

    @property
    def account_id(self):
        """
        The account identifier associated with the event.

        Returns
        -------
        AccountId

        """
        return self._account_id

    @property
    def rejected_time(self):
        """
        The requests rejected time of the event.

        Returns
        -------
        datetime

        """
        return self._rejected_time

    @property
    def response_to(self):
        """
        The cancel rejection response to.

        Returns
        -------
        str

        """
        return self._response_to

    @property
    def reason(self):
        """
        The reason for order cancel rejection.

        Returns
        -------
        str

        """
        return self._reason


cdef class OrderCancelled(OrderEvent):
    """
    Represents an event where an order has been cancelled with the
    exchange/broker.
    """

    def __init__(
            self,
            AccountId account_id not None,
            ClientOrderId cl_ord_id not None,
            OrderId order_id not None,
            datetime cancelled_time not None,
            UUID event_id not None,
            datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the OrderCancelled class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        order_id : OrderId
            The exchange/broker order identifier.
        cancelled_time : datetime
            The event order cancelled time.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(
            cl_ord_id,
            event_id,
            event_timestamp,
        )

        self._account_id = account_id
        self._order_id = order_id
        self._cancelled_time = cancelled_time
        self._is_completion_trigger = True

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self._account_id}, "
                f"cl_ord_id={self._cl_ord_id}, "
                f"order_id={self._order_id}, "
                f"id={self._id})")

    @property
    def account_id(self):
        """
        The account identifier associated with the event.

        Returns
        -------
        AccountId

        """
        return self._account_id

    @property
    def order_id(self):
        """
        The order identifier associated with the event.

        Returns
        -------
        OrderId

        """
        return self._order_id

    @property
    def cancelled_time(self):
        """
        The order cancelled time of the event.

        Returns
        -------
        datetime

        """
        return self._cancelled_time


cdef class OrderModified(OrderEvent):
    """
    Represents an event where an order has been modified with the
    exchange/broker.
    """

    def __init__(
            self,
            AccountId account_id not None,
            ClientOrderId cl_ord_id not None,
            OrderId order_id not None,
            Quantity modified_quantity not None,
            Price modified_price not None,
            datetime modified_time not None,
            UUID event_id not None,
            datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the OrderModified class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        order_id : OrderId
            The exchange/broker order identifier.
        modified_quantity : Quantity
            The modified quantity.
        modified_price : Price
            The modified price.
        modified_time : datetime
            The modified time.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(
            cl_ord_id,
            event_id,
            event_timestamp,
        )

        self._account_id = account_id
        self._order_id = order_id
        self._modified_quantity = modified_quantity
        self._modified_price = modified_price
        self._modified_time = modified_time
        self._is_completion_trigger = False

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self._account_id}, "
                f"cl_order_id={self._cl_ord_id}, "
                f"order_id={self._order_id}, "
                f"qty={self._modified_quantity.to_string()}, "
                f"price={self._modified_price}, "
                f"id={self._id})")

    @property
    def account_id(self):
        """
        The account identifier associated with the event.

        Returns
        -------
        AccountId

        """
        return self._account_id

    @property
    def order_id(self):
        """
        The order identifier associated with the event.

        Returns
        -------
        OrderId

        """
        return self._order_id

    @property
    def modified_quantity(self):
        """
        The order quantity of the event.

        Returns
        -------
        Quantity

        """
        return self._modified_quantity

    @property
    def modified_price(self):
        """
        The order price of the event.

        Returns
        -------
        Price

        """
        return self._modified_price

    @property
    def modified_time(self):
        """
        The order modified time of the event.

        Returns
        -------
        datetime

        """
        return self._modified_time


cdef class OrderExpired(OrderEvent):
    """
    Represents an event where an order has expired with the exchange/broker.
    """

    def __init__(
            self,
            AccountId account_id not None,
            ClientOrderId cl_ord_id not None,
            OrderId order_id not None,
            datetime expired_time not None,
            UUID event_id not None,
            datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the OrderExpired class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        order_id : OrderId
            The exchange/broker order identifier.
        expired_time : datetime
            The order expired time.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(
            cl_ord_id,
            event_id,
            event_timestamp,
        )

        self._account_id = account_id
        self._order_id = order_id
        self._expired_time = expired_time
        self._is_completion_trigger = True

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self._account_id}, "
                f"cl_ord_id={self._cl_ord_id}, "
                f"order_id={self._order_id}, "
                f"id={self._id})")

    @property
    def account_id(self):
        """
        The account identifier associated with the event.

        Returns
        -------
        AccountId

        """
        return self._account_id

    @property
    def order_id(self):
        """
        The order identifier associated with the event.

        Returns
        -------
        OrderId

        """
        return self._order_id

    @property
    def expired_time(self):
        """
        The order expired time of the event.

        Returns
        -------
        datetime

        """
        return self._expired_time


cdef class OrderFilled(OrderEvent):
    """
    Represents an event where an order has been filled at the exchange.
    """

    def __init__(
            self,
            AccountId account_id not None,
            ClientOrderId cl_ord_id not None,
            OrderId order_id not None,
            ExecutionId execution_id not None,
            PositionId position_id not None,
            StrategyId strategy_id not None,
            Symbol symbol not None,
            OrderSide order_side,
            Quantity filled_qty not None,
            Quantity cumulative_qty not None,
            Quantity leaves_qty not None,
            Decimal avg_price not None,
            Money commission not None,
            LiquiditySide liquidity_side,
            Currency base_currency not None,
            Currency quote_currency not None,
            bint is_inverse,
            datetime execution_time not None,
            UUID event_id not None,
            datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the OrderFilled class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        order_id : OrderId
            The exchange/broker order identifier.
        execution_id : ExecutionId
            The execution identifier.
        position_id : PositionId
            The exchange/broker position identifier.
        strategy_id : StrategyId
            The strategy identifier.
        symbol : Symbol
            The order symbol.
        order_side : OrderSide
            The execution order side.
        filled_qty : Quantity
            The filled quantity for this execution.
        cumulative_qty : Quantity
            The total filled quantity for the order.
        leaves_qty : Quantity
            The quantity open for further execution.
        avg_price : Decimal
            The average price of all fills on this order.
        liquidity_side : LiquiditySide
            The execution liquidity side.
        base_currency : Currency
            The order securities base currency.
        quote_currency : Currency
            The order securities quote currency.
        is_inverse : bool
            If the instrument base/quote is inverse for quantity and PNL.
        execution_time : datetime
            The execution time.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        Condition.not_equal(order_side, OrderSide.UNDEFINED, "order_side", "UNDEFINED")
        Condition.not_equal(liquidity_side, LiquiditySide.NONE, "liquidity_side", "NONE")
        super().__init__(
            cl_ord_id,
            event_id,
            event_timestamp,
        )

        self._account_id = account_id
        self._order_id = order_id
        self._execution_id = execution_id
        self._position_id = position_id
        self._strategy_id = strategy_id
        self._symbol = symbol
        self._order_side = order_side
        self._filled_qty = filled_qty
        self._cumulative_qty = cumulative_qty
        self._leaves_qty = leaves_qty
        self._avg_price = avg_price
        self._commission = commission
        self._liquidity_side = liquidity_side
        self._base_currency = base_currency
        self._quote_currency = quote_currency
        self._is_inverse = is_inverse
        self._execution_time = execution_time
        self._is_completion_trigger = leaves_qty == 0  # Completely filled

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self._account_id}, "
                f"cl_ord_id={self._cl_ord_id}, "
                f"order_id={self._order_id}, "
                f"position_id={self._position_id}, "
                f"strategy_id={self._strategy_id}, "
                f"symbol={self._symbol}, "
                f"side={order_side_to_string(self._order_side)}"
                f"-{liquidity_side_to_string(self._liquidity_side)}, "
                f"filled_qty={self._filled_qty.to_string()}, "
                f"leaves_qty={self._leaves_qty.to_string()}, "
                f"avg_price={self._avg_price}, "
                f"commission={self._commission.to_string()}, "
                f"id={self._id})")

    @property
    def account_id(self):
        """
        The account identifier associated with the event.

        Returns
        -------
        AccountId

        """
        return self._account_id

    @property
    def order_id(self):
        """
        The order identifier associated with the event.

        Returns
        -------
        OrderId

        """
        return self._order_id

    @property
    def execution_id(self):
        """
        The execution identifier associated with the event.

        Returns
        -------
        ExecutionId

        """
        return self._execution_id

    @property
    def position_id(self):
        """
        The position identifier associated with the event.

        Returns
        -------
        PositionId

        """
        return self._position_id

    @property
    def strategy_id(self):
        """
        The strategy identifier associated with the event.

        Returns
        -------
        StrategyId

        """
        return self._strategy_id

    @property
    def symbol(self):
        """
        The order symbol of the event.

        Returns
        -------
        Symbol

        """
        return self._symbol

    @property
    def order_side(self):
        """
        The order side of the event.

        Returns
        -------
        OrderSide

        """
        return self._order_side

    @property
    def filled_qty(self):
        """
        The order filled quantity of the event.

        Returns
        -------
        Quantity

        """
        return self._filled_qty

    @property
    def cumulative_qty(self):
        """
        The order cumulative filled quantity.

        Returns
        -------
        Quantity

        """
        return self._cumulative_qty

    @property
    def leaves_qty(self):
        """
        The order quantity remaining to be filled.

        Returns
        -------
        Quantity

        """
        return self._leaves_qty

    @property
    def is_partial_fill(self):
        """
        If the event represents a partial fill of the order.

        Returns
        -------
        bool

        """
        return self._leaves_qty > 0

    @property
    def avg_price(self):
        """
        The average fill price of the event.

        Returns
        -------
        Decimal

        """
        return self._avg_price

    @property
    def commission(self):
        """
        The commission generated from the fill event.

        Returns
        -------
        Money

        """
        return self._commission

    @property
    def liquidity_side(self):
        """
        The liquidity side of the event (if the order was MAKER or TAKER).

        Returns
        -------
        LiquiditySide

        """
        return self._liquidity_side

    @property
    def base_currency(self):
        """
        The base currency of the event.

        Returns
        -------
        Currency

        """
        return self._base_currency

    @property
    def quote_currency(self):
        """
        The quote currency of the event.

        Returns
        -------
        Currency

        """
        return self._quote_currency

    @property
    def is_inverse(self):
        """
        If the instrument associated with the event is inverse.

        Returns
        -------
        bool
            True if instrument is inverse, else False.

        """
        return self._is_inverse

    @property
    def execution_time(self):
        """
        The execution timestamp of the event.

        Returns
        -------
        datetime

        """
        return self._execution_time


cdef class PositionEvent(Event):
    """
    The base class for all position events.
    """

    def __init__(
            self,
            Position position not None,
            OrderFilled order_fill not None,
            UUID event_id not None,
            datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the PositionEvent base class.

        Parameters
        ----------
        position : Position
            The position.
        order_fill : OrderFilled
            The order fill event which triggered the event.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(event_id, event_timestamp)
        self._position = position
        self._order_fill = order_fill

    @property
    def position(self):
        """
        The position associated with the event.

        Returns
        -------
        Position

        """
        return self._position

    @property
    def order_fill(self):
        """
        The order fill of the event.

        Returns
        -------
        OrderFilled

        """
        return self._order_fill


# noinspection: Object has warned attribute
# noinspection PyUnresolvedReferences
cdef class PositionOpened(PositionEvent):
    """
    Represents an event where a position has been opened.
    """

    def __init__(
            self,
            Position position not None,
            OrderFilled order_fill not None,
            UUID event_id not None,
            datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the PositionOpened class.

        Parameters
        ----------
        position : Position
            The position.
        order_fill : OrderFilled
            The order fill event which triggered the event.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(
            position,
            order_fill,
            event_id,
            event_timestamp,
        )

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self._position.account_id}, "
                f"position_id={self._position.id}, "
                f"strategy_id={self._position.strategy_id}, "
                f"entry={order_side_to_string(self._position.entry)}, "
                f"avg_open={round(self._position.avg_open, 5)}, "
                f"{self._position.status_string()}, "
                f"id={self._id})")


# noinspection: Object has warned attribute
# noinspection PyUnresolvedReferences
cdef class PositionModified(PositionEvent):
    """
    Represents an event where a position has been modified.
    """

    def __init__(
            self,
            Position position not None,
            OrderFilled order_fill not None,
            UUID event_id not None,
            datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the PositionModified class.

        Parameters
        ----------
        position : Position
            The position.
        order_fill : OrderFilled
            The order fill event which triggered the event.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        Raises
        ------
        ValueError
            If position is not open.

        """
        assert position.is_open
        super().__init__(
            position,
            order_fill,
            event_id,
            event_timestamp,
        )

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self._position.account_id}, "
                f"position_id={self._position.id}, "
                f"strategy_id={self._position.strategy_id}, "
                f"entry={order_side_to_string(self._position.entry)}, "
                f"avg_open={self._position.avg_open}, "
                f"realized_points={self._position.realized_points}, "
                f"realized_return={round(self._position.realized_return * 100, 3)}%, "
                f"realized_pnl={self._position.realized_pnl.to_string()}, "
                f"{self._position.status_string()}, "
                f"id={self._id})")


# noinspection: Object has warned attribute
# noinspection PyUnresolvedReferences
cdef class PositionClosed(PositionEvent):
    """
    Represents an event where a position has been closed.
    """

    def __init__(
            self,
            Position position not None,
            OrderEvent order_fill not None,
            UUID event_id not None,
            datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the PositionClosed class.

        Parameters
        ----------
        position : Position
            The position.
        order_fill : OrderEvent
            The order fill event which triggered the event.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        Raises
        ------
        ValueError
            If position is not closed.

        """
        assert position.is_closed
        super().__init__(
            position,
            order_fill,
            event_id,
            event_timestamp,
        )

    def __repr__(self) -> str:
        cdef str duration = str(self._position.open_duration).replace("0 days ", "")
        return (f"{type(self).__name__}("
                f"account_id={self._position.account_id}, "
                f"position_id={self._position.id}, "
                f"strategy_id={self._position.strategy_id}, "
                f"entry={order_side_to_string(self._position.entry)}, "
                f"duration={duration}, "
                f"avg_open={self._position.avg_open}, "
                f"avg_close={self._position.avg_close}, "
                f"realized_points={round(self._position.realized_points, 5)}, "
                f"realized_return={round(self._position.realized_return * 100, 3)}%, "
                f"realized_pnl={self._position.realized_pnl.to_string()}, "
                f"id={self._id})")
