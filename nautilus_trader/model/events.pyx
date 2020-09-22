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
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.currency cimport currency_to_string
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side cimport liquidity_side_to_string
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport order_side_to_string
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_type cimport order_type_to_string
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport time_in_force_to_string
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport ClientOrderId
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

    def __init__(self,
                 AccountId account_id not None,
                 Currency currency,
                 Money cash_balance not None,
                 Money cash_start_day not None,
                 Money cash_activity_day not None,
                 Money margin_used_liquidation not None,
                 Money margin_used_maintenance not None,
                 Decimal64 margin_ratio not None,
                 str margin_call_status not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initialize a new instance of the AccountState class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        currency : Currency
            The currency for the account.
        cash_balance : Money
            The account cash balance.
        cash_start_day : Money
            The account cash start of day.
        cash_activity_day : Money
            The account activity for the trading day.
        margin_used_liquidation : Money
            The account margin used before liquidation.
        margin_used_maintenance : Money
            The account margin used for maintenance.
        margin_ratio : Decimal64
            The account margin ratio.
        margin_call_status : str
            The account margin call status (can be empty string).
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        Raises
        ------
        ValueError
            If currency is UNDEFINED.
        ValueError
            If margin ratio is negative (<0).
        ValueError
            If margin_call_status is not a valid string.

        """
        Condition.not_equal(currency, Currency.UNDEFINED, "currency", "UNDEFINED")
        Condition.not_negative(margin_ratio.as_double(), "margin_ratio")
        Condition.valid_string(margin_call_status, "margin_call_status")
        super().__init__(event_id, event_timestamp)

        self.account_id = account_id
        self.broker = self.account_id.broker
        self.number = self.account_id.account_number
        self.currency = currency
        self.cash_balance = cash_balance
        self.cash_start_day = cash_start_day
        self.cash_activity_day = cash_activity_day
        self.margin_used_liquidation = margin_used_liquidation
        self.margin_used_maintenance = margin_used_maintenance
        self.margin_ratio = margin_ratio
        self.margin_call_status = margin_call_status

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id.value}, "
                f"cash={self.cash_balance.to_string(format_commas=True)}, "
                f"margin_used={self.margin_used_maintenance.to_string(format_commas=True)})")

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        Returns
        -------
        str

        """
        return f"<{str(self)} object at {id(self)}>"


cdef class OrderEvent(Event):
    """
    The base class for all order events.
    """

    def __init__(self,
                 ClientOrderId cl_ord_id not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
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

        self.cl_ord_id = cl_ord_id

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return f"{self.__class__.__name__}(cl_ord_id={self.cl_ord_id})"

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        Returns
        -------
        str

        """
        return f"<{str(self)} object at {id(self)}>"


cdef class OrderInitialized(OrderEvent):
    """
    Represents an event where an order has been initialized.
    """

    def __init__(self,
                 ClientOrderId cl_ord_id not None,
                 Symbol symbol not None,
                 OrderSide order_side,
                 OrderType order_type,
                 Quantity quantity not None,
                 TimeInForce time_in_force,
                 UUID event_id not None,
                 datetime event_timestamp not None,
                 dict options not None):
        """
        Initialize a new instance of the OrderInitialized class.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.
        symbol : Symbol
            The order symbol.
        order_side : OrderSide
            The order side.
        order_type : OrderType
            The order type.
        quantity : Quantity
            The order quantity.
        time_in_force : TimeInForce
            The order time in force.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.
        options : Dict[str, str]
            The order options. Contains mappings for specific order params.

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
        super().__init__(cl_ord_id,
                         event_id,
                         event_timestamp)

        self.symbol = symbol
        self.order_side = order_side
        self.order_type = order_type
        self.quantity = quantity
        self.time_in_force = time_in_force
        self.options = options


cdef class OrderSubmitted(OrderEvent):
    """
    Represents an event where an order has been submitted by the system to the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 ClientOrderId order_id not None,
                 datetime submitted_time not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initialize a new instance of the OrderSubmitted class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        order_id : ClientOrderId
            The order identifier.
        submitted_time : datetime
            The order submitted time.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(order_id,
                         event_id,
                         event_timestamp)

        self.account_id = account_id
        self.submitted_time = submitted_time

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"order_id={self.cl_ord_id})")


cdef class OrderInvalid(OrderEvent):
    """
    Represents an event where an order has been invalidated by the system.
    """

    def __init__(self,
                 ClientOrderId cl_ord_id not None,
                 str reason not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
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
        super().__init__(cl_ord_id,
                         event_id,
                         event_timestamp)

        self.reason = reason

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return (f"{self.__class__.__name__}("
                f"cl_ord_id={self.cl_ord_id}, "
                f"reason={self.reason})")


cdef class OrderDenied(OrderEvent):
    """
    Represents an event where an order has been denied by the system.
    """

    def __init__(self,
                 ClientOrderId cl_ord_id not None,
                 str reason not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
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
        super().__init__(cl_ord_id,
                         event_id,
                         event_timestamp)

        self.reason = reason

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return (f"{self.__class__.__name__}("
                f"order_id={self.cl_ord_id}, "
                f"reason={self.reason})")


cdef class OrderRejected(OrderEvent):
    """
    Represents an event where an order has been rejected by the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 ClientOrderId cl_ord_id not None,
                 datetime rejected_time not None,
                 str reason not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
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
        super().__init__(cl_ord_id,
                         event_id,
                         event_timestamp)

        self.account_id = account_id
        self.rejected_time = rejected_time
        self.reason = reason

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"order_id={self.cl_ord_id}, "
                f"reason={self.reason})")


cdef class OrderAccepted(OrderEvent):
    """
    Represents an event where an order has been accepted by the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 ClientOrderId cl_ord_id not None,
                 OrderId order_id not None,
                 datetime accepted_time not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initialize a new instance of the OrderAccepted class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        order_id : OrderId
            The broker/exchange order identifier.
        accepted_time : datetime
            The order accepted time.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(cl_ord_id,
                         event_id,
                         event_timestamp)

        self.account_id = account_id
        self.order_id = order_id
        self.accepted_time = accepted_time

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"cl_ord_id={self.account_id}, "
                f"order_id={self.order_id})")


cdef class OrderWorking(OrderEvent):
    """
    Represents an event where an order is working with the broker.
    """

    def __init__(self,
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
                 datetime event_timestamp not None):
        """
        Initialize a new instance of the OrderWorking class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        order_id : OrderId
            The broker/exchange order identifier.
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
            The order time in force.
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

        super().__init__(cl_ord_id,
                         event_id,
                         event_timestamp)

        self.account_id = account_id
        self.order_id = order_id
        self.symbol = symbol
        self.order_side = order_side
        self.order_type = order_type
        self.quantity = quantity
        self.price = price
        self.time_in_force = time_in_force
        self.expire_time = expire_time
        self.working_time = working_time

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        cdef str expire_time = "" if self.expire_time is None else f" {format_iso8601(self.expire_time)}"
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"cl_ord_id={self.cl_ord_id}, "
                f"order_id={self.order_id}, "
                f"{order_side_to_string(self.order_side)} {self.quantity.to_string_formatted()} "
                f"{self.symbol} {order_type_to_string(self.order_type)} @ "
                f"{self.price} {time_in_force_to_string(self.time_in_force)}{expire_time})")


cdef class OrderCancelReject(OrderEvent):
    """
    Represents an event where an order cancel or modify command has been rejected by the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 ClientOrderId cl_ord_id not None,
                 datetime rejected_time not None,
                 str response_to not None,
                 str reason not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
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
        super().__init__(cl_ord_id,
                         event_id,
                         event_timestamp)

        self.account_id = account_id
        self.rejected_time = rejected_time
        self.response_to = response_to
        self.reason = reason

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"cl_ord_id={self.cl_ord_id}, "
                f"response_to={self.response_to}, "
                f"reason={self.reason})")


cdef class OrderCancelled(OrderEvent):
    """
    Represents an event where an order has been cancelled with the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 ClientOrderId cl_ord_id not None,
                 OrderId order_id not None,
                 datetime cancelled_time not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initialize a new instance of the OrderCancelled class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        order_id : OrderId
            The broker/exchange order identifier.
        cancelled_time : datetime
            The event order cancelled time.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(cl_ord_id,
                         event_id,
                         event_timestamp)

        self.account_id = account_id
        self.order_id = order_id
        self.cancelled_time = cancelled_time

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"cl_ord_id={self.cl_ord_id}, "
                f"order_id={self.order_id})")


cdef class OrderModified(OrderEvent):
    """
    Represents an event where an order has been modified with the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 ClientOrderId cl_ord_id not None,
                 OrderId order_id not None,
                 Quantity modified_quantity not None,
                 Price modified_price not None,
                 datetime modified_time not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initialize a new instance of the OrderModified class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        order_id : OrderId
            The broker/exchange order identifier.
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
        super().__init__(cl_ord_id,
                         event_id,
                         event_timestamp)

        self.account_id = account_id
        self.order_id = order_id
        self.modified_quantity = modified_quantity
        self.modified_price = modified_price
        self.modified_time = modified_time

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"cl_order_id={self.cl_ord_id}, "
                f"order_id={self.order_id}, "
                f"quantity={self.modified_quantity.to_string_formatted()}, "
                f"price={self.modified_price})")


cdef class OrderExpired(OrderEvent):
    """
    Represents an event where an order has expired with the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 ClientOrderId cl_ord_id not None,
                 OrderId order_id not None,
                 datetime expired_time not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initialize a new instance of the OrderExpired class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        order_id : OrderId
            The broker/exchange order identifier.
        expired_time : datetime
            The order expired time.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(cl_ord_id,
                         event_id,
                         event_timestamp)

        self.account_id = account_id
        self.order_id = order_id
        self.expired_time = expired_time

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"cl_ord_id={self.cl_ord_id}, "
                f"order_id={self.order_id})")


cdef class OrderFillEvent(OrderEvent):
    """
    The base class for all order fill events.
    """

    def __init__(self,
                 AccountId account_id not None,
                 ClientOrderId cl_ord_id not None,
                 OrderId order_id not None,
                 ExecutionId execution_id not None,
                 PositionId position_id not None,
                 Symbol symbol not None,
                 OrderSide order_side,
                 Quantity filled_quantity not None,
                 Price average_price not None,
                 Money commission not None,
                 LiquiditySide liquidity_side,
                 Currency quote_currency,
                 datetime execution_time not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initialize a new instance of the OrderFillEvent class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        order_id : OrderId
            The broker/exchange order identifier.
        execution_id : ExecutionId
            The execution identifier.
        position_id : PositionId
            The broker/exchange position identifier.
        symbol : Symbol
            The order symbol.
        order_side : OrderSide
            The execution order side.
        filled_quantity : Quantity
            The execution filled quantity.
        average_price : Price
            The execution average price.
        liquidity_side : LiquiditySide
            The execution liquidity side.
        quote_currency : Currency
            The order quote currency.
        execution_time : datetime
            The execution time.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        Raises
        ------
        ValueError
            If order_side is UNDEFINED.
        ValueError
            If liquidity_side is UNDEFINED.
        ValueError
            If quote_currency is UNDEFINED.

        """
        Condition.not_equal(order_side, OrderSide.UNDEFINED, "order_side", "UNDEFINED")
        Condition.not_equal(liquidity_side, OrderSide.UNDEFINED, "order_side", "UNDEFINED")
        Condition.not_equal(quote_currency, Currency.UNDEFINED, "quote_currency", "UNDEFINED")
        super().__init__(cl_ord_id,
                         event_id,
                         event_timestamp)

        self.account_id = account_id
        self.order_id = order_id
        self.execution_id = execution_id
        self.position_id = position_id
        self.symbol = symbol
        self.order_side = order_side
        self.filled_quantity = filled_quantity
        self.average_price = average_price
        self.commission = commission
        self.liquidity_side = liquidity_side
        self.quote_currency = quote_currency
        self.execution_time = execution_time


cdef class OrderPartiallyFilled(OrderFillEvent):
    """
    Represents an event where an order has been partially filled with the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 ClientOrderId cl_ord_id not None,
                 OrderId order_id not None,
                 ExecutionId execution_id not None,
                 PositionId position_id not None,
                 Symbol symbol not None,
                 OrderSide order_side,
                 Quantity filled_quantity not None,
                 Quantity leaves_quantity not None,
                 Price average_price not None,
                 Money commission not None,
                 LiquiditySide liquidity_side,
                 Currency quote_currency,
                 datetime execution_time not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initialize a new instance of the OrderPartiallyFilled class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        order_id : OrderId
            The broker/exchange order identifier.
        execution_id : ExecutionId
            The execution identifier.
        position_id : PositionId
            The broker/exchange position identifier.
        symbol : Symbol
            The order symbol.
        order_side : OrderSide
            The execution order side.
        filled_quantity : Quantity
            The execution filled quantity.
        leaves_quantity : Quantity
            The leaves quantity.
        average_price : Price
            The execution average price.
        liquidity_side : LiquiditySide
            The execution liquidity side.
        quote_currency : Currency
            The order quote currency.
        execution_time : datetime
            The execution time.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        # Enums checked in base class
        super().__init__(account_id,
                         cl_ord_id,
                         order_id,
                         execution_id,
                         position_id,
                         symbol,
                         order_side,
                         filled_quantity,
                         average_price,
                         commission,
                         liquidity_side,
                         quote_currency,
                         execution_time,
                         event_id,
                         event_timestamp)

        self.leaves_quantity = leaves_quantity

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"cl_ord_id={self.cl_ord_id}, "
                f"order_id={self.order_id}, "
                f"symbol={self.symbol}, "
                f"side={order_side_to_string(self.order_side)}"
                f"-{liquidity_side_to_string(self.liquidity_side)}, "
                f"quantity={self.filled_quantity.to_string_formatted()}, "
                f"leaves_quantity={self.leaves_quantity.to_string_formatted()}, "
                f"avg_price={self.average_price}, "
                f"commission={self.commission})")


cdef class OrderFilled(OrderFillEvent):
    """
    Represents an event where an order has been completely filled with the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 ClientOrderId cl_ord_id not None,
                 OrderId order_id not None,
                 ExecutionId execution_id not None,
                 PositionId position_id not None,
                 Symbol symbol not None,
                 OrderSide order_side,
                 Quantity filled_quantity not None,
                 Price average_price not None,
                 Money commission not None,
                 LiquiditySide liquidity_side,
                 Currency quote_currency,
                 datetime execution_time not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initialize a new instance of the OrderFilled class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        order_id : OrderId
            The broker/exchange order identifier.
        execution_id : ExecutionId
            The execution identifier.
        position_id : PositionId
            The broker/exchange position identifier.
        symbol : Symbol
            The order symbol.
        order_side : OrderSide
            The execution order side.
        filled_quantity : Quantity
            The execution filled quantity.
        average_price : Price
            The execution average price.
        liquidity_side : LiquiditySide
            The execution liquidity side.
        quote_currency : Currency
            The order quote currency.
        execution_time : datetime
            The execution time.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        # Enums checked in base class
        super().__init__(account_id,
                         cl_ord_id,
                         order_id,
                         execution_id,
                         position_id,
                         symbol,
                         order_side,
                         filled_quantity,
                         average_price,
                         commission,
                         liquidity_side,
                         quote_currency,
                         execution_time,
                         event_id,
                         event_timestamp)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"cl_ord_id={self.cl_ord_id}, "
                f"order_id={self.order_id}, "
                f"symbol={self.symbol}, "
                f"side={order_side_to_string(self.order_side)}"
                f"-{liquidity_side_to_string(self.liquidity_side)}, "
                f"quantity={self.filled_quantity.to_string_formatted()}, "
                f"avg_price={self.average_price}, "
                f"commission={self.commission})")


cdef class PositionEvent(Event):
    """
    The base class for all position events.
    """

    def __init__(self,
                 Position position not None,
                 StrategyId strategy_id not None,
                 OrderEvent order_fill not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initialize a new instance of the PositionEvent base class.

        Parameters
        ----------
        position : Position
            The position.
        strategy_id : StrategyId
            The strategy identifier associated with the position.
        order_fill : OrderEvent
            The order fill event which triggered the event.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(event_id, event_timestamp)
        self.position = position
        self.strategy_id = strategy_id
        self.order_fill = order_fill

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        Returns
        -------
        str

        """
        return f"<{str(self)} object at {id(self)}>"


cdef class PositionOpened(PositionEvent):
    """
    Represents an event where a position has been opened.
    """

    def __init__(self,
                 Position position not None,
                 StrategyId strategy_id not None,
                 OrderEvent order_fill not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initialize a new instance of the PositionOpened class.

        Parameters
        ----------
        position : Position
            The position.
        strategy_id : StrategyId
            The strategy identifier associated with the position.
        order_fill : OrderEvent
            The order fill event which triggered the event.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(position,
                         strategy_id,
                         order_fill,
                         event_id,
                         event_timestamp)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.position.account_id}, "
                f"cl_pos_id={self.position.client_id}, "
                f"entry={order_side_to_string(self.position.entry_direction)}, "
                f"avg_open={round(self.position.average_open_price, 5)}, "
                f"{self.position.status_string()})")


cdef class PositionModified(PositionEvent):
    """
    Represents an event where a position has been modified.
    """

    def __init__(self,
                 Position position not None,
                 StrategyId strategy_id not None,
                 OrderEvent order_fill not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initialize a new instance of the PositionModified class.

        Parameters
        ----------
        position : Position
            The position.
        strategy_id : StrategyId
            The strategy identifier associated with the position.
        order_fill : OrderEvent
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
        Condition.true(position.is_open(), "position.is_open()")
        super().__init__(position,
                         strategy_id,
                         order_fill,
                         event_id,
                         event_timestamp)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        cdef str currency = currency_to_string(self.position.quote_currency)
        return (f"{self.__class__.__name__}("
                f"account_id={self.position.account_id}, "
                f"cl_pos_id={self.position.client_id}, "
                f"position_id={order_side_to_string(self.position.entry_direction)}, "
                f"avg_open={self.position.average_open_price}, "
                f"realized_points={self.position.realized_points}, "
                f"realized_return={round(self.position.realized_return * 100, 3)}%, "
                f"realized_pnl={self.position.realized_pnl.to_string(True)} {currency}, "
                f"{self.position.status_string()})")


cdef class PositionClosed(PositionEvent):
    """
    Represents an event where a position has been closed.
    """

    def __init__(self,
                 Position position not None,
                 StrategyId strategy_id not None,
                 OrderEvent order_fill not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initialize a new instance of the PositionClosed class.

        Parameters
        ----------
        position : Position
            The position.
        strategy_id : StrategyId
            The strategy identifier associated with the position.
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
        Condition.true(position.is_closed(), "position.is_closed()")
        super().__init__(position,
                         strategy_id,
                         order_fill,
                         event_id,
                         event_timestamp)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        cdef str currency = currency_to_string(self.position.quote_currency)
        cdef str duration = str(self.position.open_duration).replace("0 days ", "")
        return (f"{self.__class__.__name__}("
                f"account_id={self.position.account_id}, "
                f"cl_pos_id={self.position.client_id}, "
                f"position_id={self.position.id}, "
                f"entry={order_side_to_string(self.position.entry_direction)}, "
                f"duration={duration}, "
                f"avg_open={self.position.average_open_price}, "
                f"avg_close={self.position.average_close_price}, "
                f"realized_points={round(self.position.realized_points, 5)}, "
                f"realized_return={round(self.position.realized_return * 100, 3)}%, "
                f"realized_pnl={self.position.realized_pnl.to_string(True)} {currency})")
