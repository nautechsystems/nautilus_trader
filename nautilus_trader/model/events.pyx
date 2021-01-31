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

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySideParser
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
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
        list balances not None,
        list balances_free not None,
        list balances_locked not None,
        dict info not None,
        UUID event_id not None,
        datetime event_timestamp not None,
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
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(event_id, event_timestamp)

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
                f"id={self.id})")


cdef class OrderEvent(Event):
    """
    The abstract base class for all order events.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(
        self,
        ClientOrderId cl_ord_id not None,
        OrderId order_id not None,
        UUID event_id not None,
        datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the `OrderEvent` base class.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.
        order_id : OrderId
            The exchange/broker order identifier.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(event_id, event_timestamp)

        self.cl_ord_id = cl_ord_id
        self.order_id = order_id


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
        Initialize a new instance of the `OrderInitialized` class.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.
        strategy_id : StrategyId
            The strategy identifier associated with the order.
        symbol : Symbol
            The order symbol.
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
        event_timestamp : datetime
            The event timestamp.
        options : dict[str, str]
            The order initialization options. Contains mappings for specific
            order parameters.

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
            OrderId.null_c(),  # Pending assignment by exchange/broker
            event_id,
            event_timestamp,
        )

        self.cl_ord_id = cl_ord_id
        self.strategy_id = strategy_id
        self.symbol = symbol
        self.order_side = order_side
        self.order_type = order_type
        self.quantity = quantity
        self.time_in_force = time_in_force
        self.options = options

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"cl_ord_id={self.cl_ord_id}, "
                f"strategy_id={self.strategy_id}, "
                f"id={self.id})")


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
        ClientOrderId cl_ord_id not None,
        str reason not None,
        UUID event_id not None,
        datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the `OrderInvalid` class.

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
            OrderId.null_c(),  # Never assigned
            event_id,
            event_timestamp,
        )

        self.reason = reason

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"cl_ord_id={self.cl_ord_id}, "
                f"reason='{self.reason}', "
                f"id={self.id})")


cdef class OrderDenied(OrderEvent):
    """
    Represents an event where an order has been denied by the Nautilus system.

    This could be due an unsupported feature, a risk limit exceedance, or for
    any other reason that an otherwise valid order is not able to be submitted.
    """

    def __init__(
        self,
        ClientOrderId cl_ord_id not None,
        str reason not None,
        UUID event_id not None,
        datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the `OrderDenied` class.

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
            OrderId.null_c(),  # Never assigned
            event_id,
            event_timestamp,
        )

        self.reason = reason

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"cl_ord_id={self.cl_ord_id}, "
                f"reason='{self.reason}', "
                f"id={self.id})")


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
        Initialize a new instance of the `OrderSubmitted` class.

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
            OrderId.null_c(),  # Pending accepted
            event_id,
            event_timestamp,
        )

        self.account_id = account_id
        self.submitted_time = submitted_time

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"cl_ord_id={self.cl_ord_id}, "
                f"id={self.id})")


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
        Initialize a new instance of the `OrderRejected` class.

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
            OrderId.null_c(),  # Not assigned on rejection
            event_id,
            event_timestamp,
        )

        self.account_id = account_id
        self.rejected_time = rejected_time
        self.reason = reason

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"cl_ord_id={self.cl_ord_id}, "
                f"reason='{self.reason}', "
                f"id={self.id})")


cdef class OrderAccepted(OrderEvent):
    """
    Represents an event where an order has been accepted by the exchange/broker.

    This event often corresponds to a `NEW` OrdStatus <39> field in FIX
    execution reports.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/4.4/tagNum_39.html

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
        Initialize a new instance of the `OrderAccepted` class.

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
            order_id,
            event_id,
            event_timestamp,
        )

        self.account_id = account_id
        self.accepted_time = accepted_time

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"cl_ord_id={self.cl_ord_id}, "
                f"order_id={self.order_id}, "
                f"id={self.id})")


cdef class OrderCancelReject(OrderEvent):
    """
    Represents an event where an order cancel or amend command has been
    rejected by the exchange/broker.
    """

    def __init__(
        self,
        AccountId account_id not None,
        ClientOrderId cl_ord_id not None,
        OrderId order_id not None,
        datetime rejected_time not None,
        str response_to not None,
        str reason not None,
        UUID event_id not None,
        datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the `OrderCancelReject` class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        order_id : OrderId
            The exchange/broker order identifier.
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
            order_id,
            event_id,
            event_timestamp,
        )

        self.account_id = account_id
        self.rejected_time = rejected_time
        self.response_to = response_to
        self.reason = reason

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"cl_ord_id={self.cl_ord_id}, "
                f"response_to={self.response_to}, "
                f"reason='{self.reason}', "
                f"id={self.id})")


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
        Initialize a new instance of the `OrderCancelled` class.

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
            order_id,
            event_id,
            event_timestamp,
        )

        self.account_id = account_id
        self.cancelled_time = cancelled_time

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"cl_ord_id={self.cl_ord_id}, "
                f"order_id={self.order_id}, "
                f"id={self.id})")


cdef class OrderAmended(OrderEvent):
    """
    Represents an event where an order has been amended with the
    exchange/broker.
    """

    def __init__(
        self,
        AccountId account_id not None,
        ClientOrderId cl_ord_id not None,
        OrderId order_id not None,
        Quantity quantity not None,
        Price price not None,
        datetime amended_time not None,
        UUID event_id not None,
        datetime event_timestamp not None,
    ):
        """
        Initialize a new instance of the `OrderAmended` class.

        Parameters
        ----------
        account_id : AccountId
            The account identifier.
        cl_ord_id : ClientOrderId
            The client order identifier.
        order_id : OrderId
            The exchange/broker order identifier.
        quantity : Quantity
            The orders current quantity.
        price : Price
            The orders current price.
        amended_time : datetime
            The amended time.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(
            cl_ord_id,
            order_id,
            event_id,
            event_timestamp,
        )

        self.account_id = account_id
        self.quantity = quantity
        self.price = price
        self.amended_time = amended_time

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"cl_order_id={self.cl_ord_id}, "
                f"order_id={self.order_id}, "
                f"qty={self.quantity.to_str()}, "
                f"price={self.price}, "
                f"id={self.id})")


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
        Initialize a new instance of the `OrderExpired` class.

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
            order_id,
            event_id,
            event_timestamp,
        )

        self.account_id = account_id
        self.expired_time = expired_time

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"cl_ord_id={self.cl_ord_id}, "
                f"order_id={self.order_id}, "
                f"id={self.id})")


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
        Quantity fill_qty not None,
        Quantity cum_qty not None,
        Quantity leaves_qty not None,
        Price fill_price not None,
        Currency currency not None,
        bint is_inverse,
        Money commission not None,
        LiquiditySide liquidity_side,
        datetime execution_time not None,
        UUID event_id not None,
        datetime event_timestamp not None,
        dict info=None,
    ):
        """
        Initialize a new instance of the `OrderFilled` class.

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
            The position identifier associated with the order.
        strategy_id : StrategyId
            The strategy identifier associated with the order.
        symbol : Symbol
            The order symbol.
        order_side : OrderSide (Enum)
            The execution order side.
        fill_qty : Quantity
            The filled quantity for this execution.
        cum_qty : Quantity
            The cumulative filled quantity for the order.
        leaves_qty : Quantity
            The quantity open for further execution.
        fill_price : Price
            The fill price for this execution (not average).
        currency : Currency
            The currency of the price.
        is_inverse : bool
            If quantity is expressed in quote currency.
        commission : Money
            The fill commission.
        liquidity_side : LiquiditySide (Enum)
            The execution liquidity side.
        execution_time : datetime
            The execution time.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp.
        info : dict[str, object], optional
            The additional fill information.

        """
        if info is None:
            info = {}
        Condition.not_equal(order_side, OrderSide.UNDEFINED, "order_side", "UNDEFINED")
        Condition.not_equal(liquidity_side, LiquiditySide.NONE, "liquidity_side", "NONE")
        super().__init__(
            cl_ord_id,
            order_id,
            event_id,
            event_timestamp,
        )

        self.account_id = account_id
        self.execution_id = execution_id
        self.position_id = position_id
        self.strategy_id = strategy_id
        self.symbol = symbol
        self.order_side = order_side
        self.fill_qty = fill_qty
        self.cum_qty = cum_qty
        self.leaves_qty = leaves_qty
        self.fill_price = fill_price
        self.currency = currency
        self.is_inverse = is_inverse
        self.commission = commission
        self.liquidity_side = liquidity_side
        self.execution_time = execution_time
        self.info = info

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id}, "
                f"cl_ord_id={self.cl_ord_id}, "
                f"order_id={self.order_id}, "
                f"position_id={self.position_id}, "
                f"strategy_id={self.strategy_id}, "
                f"symbol={self.symbol}, "
                f"side={OrderSideParser.to_str(self.order_side)}"
                f"-{LiquiditySideParser.to_str(self.liquidity_side)}, "
                f"fill_qty={self.fill_qty.to_str()}, "
                f"fill_price={self.fill_price} {self.currency.code}, "
                f"cum_qty={self.cum_qty.to_str()}, "
                f"leaves_qty={self.leaves_qty.to_str()}, "
                f"commission={self.commission.to_str()}, "
                f"id={self.id})")


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
        datetime event_timestamp not None,
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
        event_timestamp : datetime
            The event timestamp.

        """
        super().__init__(event_id, event_timestamp)
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
        datetime event_timestamp not None,
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
        event_timestamp : datetime
            The event timestamp.

        """
        assert position.is_open_c()
        super().__init__(
            position,
            order_fill,
            event_id,
            event_timestamp,
        )

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.position.account_id}, "
                f"position_id={self.position.id}, "
                f"strategy_id={self.position.strategy_id}, "
                f"entry={OrderSideParser.to_str(self.position.entry)}, "
                f"avg_open={round(self.position.avg_open, 5)}, "
                f"{self.position.status_string_c()}, "
                f"id={self.id})")


cdef class PositionChanged(PositionEvent):
    """
    Represents an event where a position has changed.
    """

    def __init__(
        self,
        Position position not None,
        OrderFilled order_fill not None,
        UUID event_id not None,
        datetime event_timestamp not None,
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
        event_timestamp : datetime
            The event timestamp.

        Raises
        ------
        ValueError
            If position is not open.

        """
        assert position.is_open_c()
        super().__init__(
            position,
            order_fill,
            event_id,
            event_timestamp,
        )

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.position.account_id}, "
                f"position_id={self.position.id}, "
                f"strategy_id={self.position.strategy_id}, "
                f"entry={OrderSideParser.to_str(self.position.entry)}, "
                f"avg_open={self.position.avg_open}, "
                f"realized_points={self.position.realized_points}, "
                f"realized_return={round(self.position.realized_return * 100, 3)}%, "
                f"realized_pnl={self.position.realized_pnl.to_str()}, "
                f"{self.position.status_string_c()}, "
                f"id={self.id})")


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
        Initialize a new instance of the `PositionClosed` class.

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
        assert position.is_closed_c()
        super().__init__(
            position,
            order_fill,
            event_id,
            event_timestamp,
        )

    def __repr__(self) -> str:
        cdef str duration = str(self.position.open_duration).replace("0 days ", "", 1)
        return (f"{type(self).__name__}("
                f"account_id={self.position.account_id}, "
                f"position_id={self.position.id}, "
                f"strategy_id={self.position.strategy_id}, "
                f"entry={OrderSideParser.to_str(self.position.entry)}, "
                f"duration={duration}, "
                f"avg_open={self.position.avg_open}, "
                f"avg_close={self.position.avg_close}, "
                f"realized_points={round(self.position.realized_points, 5)}, "
                f"realized_return={round(self.position.realized_return * 100, 3)}%, "
                f"realized_pnl={self.position.realized_pnl.to_str()}, "
                f"id={self.id})")
