# -------------------------------------------------------------------------------------------------
# <copyright file="events.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport ValidString, GUID
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.functions cimport format_iso8601
from nautilus_trader.model.c_enums.currency cimport Currency, currency_to_string
from nautilus_trader.model.c_enums.order_side cimport OrderSide, order_side_to_string
from nautilus_trader.model.c_enums.order_type cimport OrderType, order_type_to_string
from nautilus_trader.model.c_enums.order_purpose cimport OrderPurpose
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce, time_in_force_to_string
from nautilus_trader.model.identifiers cimport (  # noqa: E211
    Symbol,
    Label,
    AccountId,
    StrategyId,
    OrderId,
    ExecutionId,
    PositionIdBroker)
from nautilus_trader.model.objects cimport Quantity, Price
from nautilus_trader.model.position cimport Position


cdef class AccountStateEvent(Event):
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
                 Decimal margin_ratio not None,
                 ValidString margin_call_status not None,
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the AccountEvent class.

        :param account_id: The account_id.
        :param currency: The currency for the account.
        :param cash_balance: The account cash balance.
        :param cash_start_day: The account cash start of day.
        :param cash_activity_day: The account activity for the trading day.
        :param margin_used_liquidation: The account margin used before liquidation.
        :param margin_used_maintenance: The account margin used for maintenance.
        :param margin_ratio: The account margin ratio.
        :param margin_call_status: The account margin call status (can be empty).
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        Condition.not_equal(currency, Currency.UNDEFINED, 'currency', 'UNDEFINED')
        Condition.not_negative(margin_ratio.as_double(), 'margin_ratio')

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

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id.value}, "
                f"cash={self.cash_balance.to_string(format_commas=True)}, "
                f"margin_used={self.margin_used_maintenance.to_string(format_commas=True)})")

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{str(self)} object at {id(self)}>"


cdef class OrderEvent(Event):
    """
    The base class for all order events.
    """

    def __init__(self,
                 OrderId order_id not None,
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the OrderEvent base class.

        :param order_id: The event order_id.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        super().__init__(event_id, event_timestamp)
        self.order_id = order_id

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return f"{self.__class__.__name__}(order_id={self.order_id})"

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{str(self)} object at {id(self)}>"


cdef class OrderFillEvent(OrderEvent):
    """
    The base class for all order fill events.
    """

    def __init__(self,
                 AccountId account_id not None,
                 OrderId order_id not None,
                 ExecutionId execution_id not None,
                 PositionIdBroker position_id_broker not None,
                 Symbol symbol not None,
                 OrderSide order_side,
                 Quantity filled_quantity not None,
                 Price average_price not None,
                 Currency transaction_currency,
                 datetime execution_time not None,
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the OrderFillEvent class.

        :param account_id: The event account_id.
        :param order_id: The event order_id.
        :param execution_id: The event order execution_id.
        :param position_id_broker: The event broker position identifier.
        :param symbol: The event order symbol.
        :param order_side: The event execution order side.
        :param filled_quantity: The event execution filled quantity.
        :param average_price: The event execution average price.
        :param transaction_currency: The event order transaction currency.
        :param execution_time: The event execution time.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        super().__init__(order_id,
                         event_id,
                         event_timestamp)
        self.account_id = account_id
        self.execution_id = execution_id
        self.position_id_broker = position_id_broker
        self.symbol = symbol
        self.order_side = order_side
        self.filled_quantity = filled_quantity
        self.average_price = average_price
        self.transaction_currency = transaction_currency
        self.execution_time = execution_time


cdef class OrderInitialized(OrderEvent):
    """
    Represents an event where an order has been initialized.
    """

    def __init__(self,
                 OrderId order_id not None,
                 Symbol symbol not None,
                 Label label,  # Can be None
                 OrderSide order_side,
                 OrderType order_type,
                 Quantity quantity not None,
                 Price price,  # Can be None
                 OrderPurpose order_purpose,
                 TimeInForce time_in_force,
                 datetime expire_time,  # Can be None
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the OrderInitialized class.

        :param order_id: The event order_id.
        :param symbol: The event order symbol.
        :param label: The event order label.
        :param order_side: The event order side.
        :param order_type: The event order type.
        :param quantity: The event order quantity.
        :param price: The event order price.
        :param order_purpose: The event order purpose.
        :param time_in_force: The event order time in force.
        :param expire_time: The event order expire time.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        super().__init__(order_id,
                         event_id,
                         event_timestamp)
        self.symbol = symbol
        self.label = label
        self.order_side = order_side
        self.order_type = order_type
        self.quantity = quantity
        self.price = price
        self.order_purpose = order_purpose
        self.time_in_force = time_in_force
        self.expire_time = expire_time


cdef class OrderSubmitted(OrderEvent):
    """
    Represents an event where an order has been submitted by the system to the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 OrderId order_id not None,
                 datetime submitted_time not None,
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the OrderSubmitted class.

        :param account_id: The event account_id.
        :param order_id: The event order_id.
        :param submitted_time: The event order submitted time.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        super().__init__(order_id,
                         event_id,
                         event_timestamp)
        self.account_id = account_id
        self.submitted_time = submitted_time

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"order_id={self.order_id})")


cdef class OrderInvalid(OrderEvent):
    """
    Represents an event where an order has been invalidated by the system.
    """

    def __init__(self,
                 OrderId order_id not None,
                 str invalid_reason not None,
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the OrderInvalid class.

        :param order_id: The event order_id.
        :param invalid_reason: The event invalid reason.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        super().__init__(order_id,
                         event_id,
                         event_timestamp)
        self.invalid_reason = invalid_reason

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"order_id={self.order_id}, "
                f"invalid_reason={self.invalid_reason})")


cdef class OrderDenied(OrderEvent):
    """
    Represents an event where an order has been denied by the system.
    """

    def __init__(self,
                 OrderId order_id not None,
                 str denied_reason not None,
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the OrderDenied class.

        :param order_id: The event order_id.
        :param denied_reason: The event denied reason.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        super().__init__(order_id,
                         event_id,
                         event_timestamp)
        self.denied_reason = denied_reason

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"order_id={self.order_id}, "
                f"denied_reason={self.denied_reason})")


cdef class OrderRejected(OrderEvent):
    """
    Represents an event where an order has been rejected by the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 OrderId order_id not None,
                 datetime rejected_time not None,
                 ValidString rejected_reason not None,
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the OrderRejected class.

        :param account_id: The event account_id.
        :param order_id: The event order_id.
        :param rejected_time: The event order rejected time.
        :param rejected_reason: The event order rejected reason.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        super().__init__(order_id,
                         event_id,
                         event_timestamp)
        self.account_id = account_id
        self.rejected_time = rejected_time
        self.rejected_reason = rejected_reason

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"order_id={self.order_id}, "
                f"rejected_reason={self.rejected_reason})")


cdef class OrderAccepted(OrderEvent):
    """
    Represents an event where an order has been accepted by the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 OrderId order_id not None,
                 OrderIdBroker order_id_broker not None,
                 Label label,  # Can be None
                 datetime accepted_time not None,
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the OrderAccepted class.

        :param account_id: The event account_id.
        :param order_id: The event order_id.
        :param order_id_broker: The event broker order_id.
        :param label: The event order label.
        :param accepted_time: The event order accepted time.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        super().__init__(order_id,
                         event_id,
                         event_timestamp)
        self.account_id = account_id
        self.order_id_broker = order_id_broker
        self.label = label
        self.accepted_time = accepted_time

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"order_id={self.order_id}, "
                f"label={self.label})")


cdef class OrderWorking(OrderEvent):
    """
    Represents an event where an order is working with the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 OrderId order_id not None,
                 OrderIdBroker order_id_broker not None,
                 Symbol symbol not None,
                 Label label,  # Can be None
                 OrderSide order_side,
                 OrderType order_type,
                 Quantity quantity not None,
                 Price price not None,
                 TimeInForce time_in_force,
                 datetime working_time not None,
                 GUID event_id not None,
                 datetime event_timestamp not None,
                 datetime expire_time=None):
        """
        Initializes a new instance of the OrderWorking class.

        :param account_id: The event account_id.
        :param order_id: The event order_id.
        :param order_id_broker: The event broker order_id.
        :param symbol: The event order symbol.
        :param label: The event order label.
        :param order_side: The event order side.
        :param order_type: The event order type.
        :param quantity: The event order quantity.
        :param price: The event order price.
        :param time_in_force: The event order time in force.
        :param working_time: The event order working time.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        :param expire_time: The optional event order expire time (for GTD orders).
        """
        Condition.not_equal(order_side, OrderSide.UNDEFINED, 'order_side', 'UNDEFINED')
        Condition.not_equal(order_type, OrderType.UNDEFINED, 'order_type', 'UNDEFINED')
        Condition.type_or_none(expire_time, datetime, 'expire_time')

        super().__init__(order_id,
                         event_id,
                         event_timestamp)
        self.account_id = account_id
        self.order_id_broker = order_id_broker
        self.symbol = symbol
        self.label = label
        self.order_side = order_side
        self.order_type = order_type
        self.quantity = quantity
        self.price = price
        self.time_in_force = time_in_force
        self.working_time = working_time
        self.expire_time = expire_time

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        cdef str expire_time = '' if self.expire_time is None else f' {format_iso8601(self.expire_time)}'
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"order_id={self.order_id}, "
                f"label={self.label}, "
                f"{order_side_to_string(self.order_side)} {self.quantity.to_string_formatted()} "
                f"{self.symbol} {order_type_to_string(self.order_type)} @ "
                f"{self.price} {time_in_force_to_string(self.time_in_force)}{expire_time})")


cdef class OrderCancelReject(OrderEvent):
    """
    Represents an event where an order cancel or modify command has been rejected by the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 OrderId order_id not None,
                 datetime rejected_time not None,
                 ValidString rejected_response_to not None,
                 ValidString rejected_reason not None,
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the OrderCancelReject class.

        :param account_id: The event account_id.
        :param order_id: The event order_id.
        :param rejected_time: The event order cancel reject time.
        :param rejected_response_to: The event order cancel reject response.
        :param rejected_reason: The event order cancel reject reason.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        super().__init__(order_id,
                         event_id,
                         event_timestamp)
        self.account_id = account_id
        self.rejected_time = rejected_time
        self.rejected_response_to = rejected_response_to
        self.rejected_reason = rejected_reason

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"order_id={self.order_id}, "
                f"response_to={self.rejected_response_to}, "
                f"reason={self.rejected_reason})")


cdef class OrderCancelled(OrderEvent):
    """
    Represents an event where an order has been cancelled with the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 OrderId order_id not None,
                 datetime cancelled_time not None,
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the OrderCancelled class.

        :param account_id: The event account_id.
        :param order_id: The event order_id.
        :param cancelled_time: The event order cancelled time.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        super().__init__(order_id,
                         event_id,
                         event_timestamp)
        self.account_id = account_id
        self.cancelled_time = cancelled_time

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"order_id={self.order_id})")


cdef class OrderModified(OrderEvent):
    """
    Represents an event where an order has been modified with the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 OrderId order_id not None,
                 OrderIdBroker order_id_broker not None,
                 Quantity modified_quantity not None,
                 Price modified_price not None,
                 datetime modified_time not None,
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the OrderModified class.

        :param account_id: The event account_id.
        :param order_id: The event order_id.
        :param order_id_broker: The event order broker identifier.
        :param modified_quantity: The event modified quantity.
        :param modified_price: The event modified price.
        :param modified_time: The event modified time.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        super().__init__(order_id,
                         event_id,
                         event_timestamp)
        self.account_id = account_id
        self.order_id_broker = order_id_broker
        self.modified_quantity = modified_quantity
        self.modified_price = modified_price
        self.modified_time = modified_time

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"order_id={self.order_id}, "
                f"quantity={self.modified_quantity.to_string_formatted()}, "
                f"price={self.modified_price})")


cdef class OrderExpired(OrderEvent):
    """
    Represents an event where an order has expired with the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 OrderId order_id not None,
                 datetime expired_time not None,
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the OrderExpired class.

        :param account_id: The event account_id.
        :param order_id: The event order_id.
        :param expired_time: The event order expired time.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        super().__init__(order_id,
                         event_id,
                         event_timestamp)
        self.account_id = account_id
        self.expired_time = expired_time

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"order_id={self.order_id})")


cdef class OrderPartiallyFilled(OrderFillEvent):
    """
    Represents an event where an order has been partially filled with the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 OrderId order_id not None,
                 ExecutionId execution_id not None,
                 PositionIdBroker position_id_broker not None,
                 Symbol symbol not None,
                 OrderSide order_side,
                 Quantity filled_quantity not None,
                 Quantity leaves_quantity not None,
                 Price average_price not None,
                 Currency transaction_currency,
                 datetime execution_time not None,
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the OrderPartiallyFilled class.

        :param account_id: The event account_id.
        :param order_id: The event order_id.
        :param execution_id: The event order execution_id.
        :param position_id_broker: The event broker position identifier.
        :param symbol: The event order symbol.
        :param order_side: The event execution order side.
        :param filled_quantity: The event execution filled quantity.
        :param leaves_quantity: The event leaves quantity.
        :param average_price: The event execution average price.
        :param transaction_currency: The event order transaction currency.
        :param execution_time: The event execution time.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        super().__init__(account_id,
                         order_id,
                         execution_id,
                         position_id_broker,
                         symbol,
                         order_side,
                         filled_quantity,
                         average_price,
                         transaction_currency,
                         execution_time,
                         event_id,
                         event_timestamp)
        self.leaves_quantity = leaves_quantity

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"order_id={self.order_id}, "
                f"symbol={self.symbol}, "
                f"side={order_side_to_string(self.order_side)}, "
                f"quantity={self.filled_quantity.to_string_formatted()}, "
                f"leaves_quantity={self.leaves_quantity.to_string_formatted()}, "
                f"avg_price={self.average_price} {currency_to_string(self.transaction_currency)})")


cdef class OrderFilled(OrderFillEvent):
    """
    Represents an event where an order has been completely filled with the broker.
    """

    def __init__(self,
                 AccountId account_id not None,
                 OrderId order_id not None,
                 ExecutionId execution_id not None,
                 PositionIdBroker position_id_broker not None,
                 Symbol symbol not None,
                 OrderSide order_side,
                 Quantity filled_quantity not None,
                 Price average_price not None,
                 Currency transaction_currency,
                 datetime execution_time not None,
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the OrderFilled class.

        :param account_id: The event account_id.
        :param order_id: The event order_id.
        :param execution_id: The event order execution_id.
        :param position_id_broker: The event broker position identifier.
        :param symbol: The event order symbol.
        :param order_side: The event execution order side.
        :param filled_quantity: The event execution filled quantity.
        :param average_price: The event execution average price.
        :param transaction_currency: The event order transaction currency.
        :param execution_time: The event execution time.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        Condition.not_equal(order_side, OrderSide.UNDEFINED, 'order_side', 'UNDEFINED')

        super().__init__(account_id,
                         order_id,
                         execution_id,
                         position_id_broker,
                         symbol,
                         order_side,
                         filled_quantity,
                         average_price,
                         transaction_currency,
                         execution_time,
                         event_id,
                         event_timestamp)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.account_id}, "
                f"order_id={self.order_id}, "
                f"symbol={self.symbol}, "
                f"side={order_side_to_string(self.order_side)}, "
                f"quantity={self.filled_quantity.to_string_formatted()}, "
                f"avg_price={self.average_price} {currency_to_string(self.transaction_currency)})")


cdef class PositionEvent(Event):
    """
    The base class for all position events.
    """

    def __init__(self,
                 Position position not None,
                 StrategyId strategy_id not None,
                 OrderEvent order_fill not None,
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the PositionEvent base class.

        :param position: The event position.
        :param strategy_id: The strategy_id associated with the position.
        :param order_fill: The order fill event which triggered the event.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        super().__init__(event_id, event_timestamp)
        self.position = position
        self.strategy_id = strategy_id
        self.order_fill = order_fill

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
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
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the PositionOpened class.

        :param position: The event position.
        :param strategy_id: The strategy_id associated with the position.
        :param order_fill: The order fill event which triggered the event.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        super().__init__(position,
                         strategy_id,
                         order_fill,
                         event_id,
                         event_timestamp)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"account_id={self.position.account_id}, "
                f"position_id={self.position.id}, "
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
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the PositionModified class.

        :param position: The event position.
        :param strategy_id: The strategy_id associated with the position.
        :param order_fill: The order fill event which triggered the event.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        :raises: ValueError: If the position is not open.
        """
        Condition.true(position.is_open, 'position.is_open')
        super().__init__(position,
                         strategy_id,
                         order_fill,
                         event_id,
                         event_timestamp)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        cdef str currency = currency_to_string(self.order_fill.transaction_currency)
        return (f"{self.__class__.__name__}("
                f"account_id={self.position.account_id}, "
                f"position_id={self.position.id}, "
                f"entry={order_side_to_string(self.position.entry_direction)}, "
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
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the PositionClosed class.

        :param position: The event position.
        :param strategy_id: The strategy_id associated with the position.
        :param order_fill: The order fill event which triggered the event.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        :raises: ValueError: If the position is not closed.
        """
        Condition.true(position.is_closed, 'position.is_closed')
        super().__init__(position,
                         strategy_id,
                         order_fill,
                         event_id,
                         event_timestamp)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        cdef str currency = currency_to_string(self.order_fill.transaction_currency)
        cdef str duration = str(self.position.open_duration).replace("0 days ", "")
        return (f"{self.__class__.__name__}("
                f"account_id={self.position.account_id}, "
                f"position_id={self.position.id}, "
                f"entry={order_side_to_string(self.position.entry_direction)}, "
                f"duration={duration}, "
                f"avg_open={self.position.average_open_price}, "
                f"avg_close={self.position.average_close_price}, "
                f"realized_points={round(self.position.realized_points, 5)}, "
                f"realized_return={round(self.position.realized_return * 100, 3)}%, "
                f"realized_pnl={self.position.realized_pnl.to_string(True)} {currency})")


cdef class TimeEvent(Event):
    """
    Represents a time event occurring at the event timestamp.
    """

    def __init__(self,
                 Label label not None,
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the TimeEvent class.

        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        super().__init__(event_id, event_timestamp)
        self.label = label

    def __eq__(self, TimeEvent other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp == other.timestamp

    def __ne__(self, TimeEvent other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp != other.timestamp

    def __lt__(self, TimeEvent other) -> bool:
        """
        Return a value indicating whether this object is less than (<) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp < other.timestamp

    def __le__(self, TimeEvent other) -> bool:
        """
        Return a value indicating whether this object is less than or equal to (<=) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp <= other.timestamp

    def __gt__(self, TimeEvent other) -> bool:
        """
        Return a value indicating whether this object is greater than (>) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp > other.timestamp

    def __ge__(self, TimeEvent other) -> bool:
        """
        Return a value indicating whether this object is greater than or equal to (>=) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp >= other.timestamp

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        :return int.
        """
        return hash(self.id)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"label={self.label}, "
                f"timestamp={format_iso8601(self.timestamp)})")

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{str(self)} object at {id(self)}>"
