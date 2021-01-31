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

import msgpack

from nautilus_trader.common.cache cimport IdentifierCache
from nautilus_trader.core.cache cimport ObjectCache
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySideParser
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_type cimport OrderTypeParser
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForceParser
from nautilus_trader.model.commands cimport AmendOrder
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.events cimport OrderAccepted
from nautilus_trader.model.events cimport OrderAmended
from nautilus_trader.model.events cimport OrderCancelReject
from nautilus_trader.model.events cimport OrderCancelled
from nautilus_trader.model.events cimport OrderDenied
from nautilus_trader.model.events cimport OrderExpired
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderInitialized
from nautilus_trader.model.events cimport OrderInvalid
from nautilus_trader.model.events cimport OrderRejected
from nautilus_trader.model.events cimport OrderSubmitted
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order.base cimport Order
from nautilus_trader.model.order.base cimport PassiveOrder
from nautilus_trader.model.order.bracket cimport BracketOrder
from nautilus_trader.model.order.limit cimport LimitOrder
from nautilus_trader.model.order.market cimport MarketOrder
from nautilus_trader.model.order.stop_market cimport StopMarketOrder
from nautilus_trader.serialization.base cimport CommandSerializer
from nautilus_trader.serialization.base cimport EventSerializer
from nautilus_trader.serialization.base cimport OrderSerializer
from nautilus_trader.serialization.parsing cimport ObjectParser


cdef class MsgPackSerializer:
    """
    Provides a serializer for the MessagePack specification.

    """
    @staticmethod
    cdef bytes serialize(dict message):
        """
        Serialize the given message to `MessagePack` specification bytes.

        Parameters
        ----------
        message : dict
            The message to serialize.

        Returns
        -------
        bytes

        """
        Condition.not_none(message, "message")

        return msgpack.packb(message)

    @staticmethod
    cdef dict deserialize(bytes message_bytes):
        """
        Deserialize the given `MessagePack` specification bytes to a dictionary.

        Parameters
        ----------
        message_bytes : bytes
            The message bytes to deserialize.

        Returns
        -------
        dict[str, object]

        """
        Condition.not_none(message_bytes, "message_bytes")

        return msgpack.unpackb(message_bytes)


cdef class MsgPackOrderSerializer(OrderSerializer):
    """
    Provides a `Command` serializer for the `MessagePack` specification.

    """

    def __init__(self):
        """
        Initialize a new instance of the `MsgPackOrderSerializer` class.

        """
        super().__init__()

        self.symbol_cache = ObjectCache(Symbol, Symbol.from_str_c)

    cpdef bytes serialize(self, Order order):  # Can be None
        """
        Return the serialized MessagePack specification bytes from the given order.

        Parameters
        ----------
        order : Order
            The order to serialize.

        Returns
        -------
        bytes

        """
        if order is None:
            return MsgPackSerializer.serialize({})  # Null order

        cdef dict package = {
            ID: order.cl_ord_id.value,
            STRATEGY_ID: order.strategy_id.value,
            SYMBOL: order.symbol.value,
            ORDER_SIDE: self.convert_snake_to_camel(OrderSideParser.to_str(order.side)),
            ORDER_TYPE: self.convert_snake_to_camel(OrderTypeParser.to_str(order.type)),
            QUANTITY: str(order.quantity),
            TIME_IN_FORCE: TimeInForceParser.to_str(order.time_in_force),
            INIT_ID: order.init_id.value,
            TIMESTAMP: ObjectParser.datetime_to_str(order.timestamp),
        }

        if isinstance(order, PassiveOrder):
            package[PRICE] = str(order.price)
            if order.expire_time is not None:
                package[EXPIRE_TIME] = ObjectParser.datetime_to_str(order.expire_time)

        if isinstance(order, LimitOrder):
            package[POST_ONLY] = order.is_post_only
            package[REDUCE_ONLY] = order.is_reduce_only
            package[HIDDEN] = order.is_hidden
        elif isinstance(order, StopMarketOrder):
            package[REDUCE_ONLY] = order.is_reduce_only

        return MsgPackSerializer.serialize(package)

    cpdef Order deserialize(self, bytes order_bytes):
        """
        Return the `Order` deserialized from the given MessagePack specification bytes.

        Parameters
        ----------
        order_bytes : bytes
            The bytes to deserialize.

        Returns
        -------
        Order

        Raises
        ------
        ValueError
            If order_bytes is empty.

        """
        Condition.not_empty(order_bytes, "order_bytes")

        cdef dict unpacked = MsgPackSerializer.deserialize(order_bytes)

        if not unpacked:
            return None  # Null order

        cdef ClientOrderId cl_ord_id = ClientOrderId(unpacked[ID])
        cdef StrategyId strategy_id = StrategyId.from_str_c(unpacked[STRATEGY_ID])
        cdef Symbol symbol = self.symbol_cache.get(unpacked[SYMBOL])
        cdef OrderSide order_side = OrderSideParser.from_str(self.convert_camel_to_snake(unpacked[ORDER_SIDE]))
        cdef OrderType order_type = OrderTypeParser.from_str(self.convert_camel_to_snake(unpacked[ORDER_TYPE]))
        cdef Quantity quantity = Quantity(unpacked[QUANTITY])
        cdef TimeInForce time_in_force = TimeInForceParser.from_str(unpacked[TIME_IN_FORCE])
        cdef UUID init_id = UUID.from_str_c(unpacked[INIT_ID])
        cdef datetime timestamp = ObjectParser.string_to_datetime(unpacked[TIMESTAMP])

        if order_type == OrderType.MARKET:
            return MarketOrder(
                cl_ord_id=cl_ord_id,
                strategy_id=strategy_id,
                symbol=symbol,
                order_side=order_side,
                quantity=quantity,
                time_in_force=time_in_force,
                init_id=init_id,
                timestamp=timestamp,
            )

        cdef str expire_time_str = unpacked.get(EXPIRE_TIME)
        cdef datetime expire_time = None
        if expire_time_str is not None:
            expire_time = ObjectParser.string_to_datetime(expire_time_str)
        if order_type == OrderType.LIMIT:
            return LimitOrder(
                cl_ord_id=cl_ord_id,
                strategy_id=strategy_id,
                symbol=symbol,
                order_side=order_side,
                quantity=quantity,
                price=Price(unpacked[PRICE]),
                time_in_force=time_in_force,
                expire_time=expire_time,
                init_id=init_id,
                timestamp=timestamp,
                post_only=unpacked[POST_ONLY],
                reduce_only=unpacked[REDUCE_ONLY],
                hidden=unpacked[HIDDEN],
            )

        if order_type == OrderType.STOP_MARKET:
            return StopMarketOrder(
                cl_ord_id=cl_ord_id,
                strategy_id=strategy_id,
                symbol=symbol,
                order_side=order_side,
                quantity=quantity,
                price=Price(unpacked[PRICE]),
                time_in_force=time_in_force,
                expire_time=expire_time,
                init_id=init_id,
                timestamp=timestamp,
                reduce_only=unpacked[REDUCE_ONLY],
            )

        raise ValueError(f"Invalid order_type, was {OrderTypeParser.to_str(order_type)}")


cdef class MsgPackCommandSerializer(CommandSerializer):
    """
    Provides a `Command` serializer for the MessagePack specification.

    """

    def __init__(self):
        """
        Initialize a new instance of the `MsgPackCommandSerializer` class.

        """
        super().__init__()

        self.identifier_cache = IdentifierCache()
        self.order_serializer = MsgPackOrderSerializer()

    cpdef bytes serialize(self, Command command):
        """
        Return the serialized `MessagePack` specification bytes from the given command.

        Parameters
        ----------
        command : Command
            The command to serialize.

        Returns
        -------
        bytes

        Raises
        ------
        RuntimeError
            If the command cannot be serialized.

        """
        Condition.not_none(command, "command")

        cdef dict package = {
            TYPE: type(command).__name__,
            ID: command.id.value,
            TIMESTAMP: ObjectParser.datetime_to_str(command.timestamp),
        }

        if isinstance(command, SubmitOrder):
            package[VENUE] = command.venue.value
            package[TRADER_ID] = command.trader_id.value
            package[ACCOUNT_ID] = command.account_id.value
            package[STRATEGY_ID] = command.strategy_id.value
            package[POSITION_ID] = command.position_id.value
            package[ORDER] = self.order_serializer.serialize(command.order)
        elif isinstance(command, SubmitBracketOrder):
            package[VENUE] = command.venue.value
            package[TRADER_ID] = command.trader_id.value
            package[ACCOUNT_ID] = command.account_id.value
            package[STRATEGY_ID] = command.strategy_id.value
            package[ENTRY] = self.order_serializer.serialize(command.bracket_order.entry)
            package[STOP_LOSS] = self.order_serializer.serialize(command.bracket_order.stop_loss)
            package[TAKE_PROFIT] = self.order_serializer.serialize(command.bracket_order.take_profit)
        elif isinstance(command, AmendOrder):
            package[VENUE] = command.venue.value
            package[TRADER_ID] = command.trader_id.value
            package[ACCOUNT_ID] = command.account_id.value
            package[CLIENT_ORDER_ID] = command.cl_ord_id.value
            package[QUANTITY] = str(command.quantity)
            package[PRICE] = str(command.price)
        elif isinstance(command, CancelOrder):
            package[VENUE] = command.venue.value
            package[TRADER_ID] = command.trader_id.value
            package[ACCOUNT_ID] = command.account_id.value
            package[CLIENT_ORDER_ID] = command.cl_ord_id.value
            package[ORDER_ID] = command.order_id.value
        else:
            raise RuntimeError("Cannot serialize command, unrecognized command")

        return MsgPackSerializer.serialize(package)

    cpdef Command deserialize(self, bytes command_bytes):
        """
        Return the command deserialize from the given MessagePack specification command_bytes.

        Parameters
        ----------
        command_bytes : bytes
            The command to deserialize.

        Returns
        -------
        Command

        Raises
        ------
        ValueError
            If command_bytes is empty.
        RuntimeError
            If command cannot be deserialized.

        """
        Condition.not_empty(command_bytes, "command_bytes")

        cdef dict unpacked = MsgPackSerializer.deserialize(command_bytes)  # type: dict[str, bytes]

        cdef str command_type = unpacked[TYPE]
        cdef UUID command_id = UUID.from_str_c(unpacked[ID])
        cdef datetime command_timestamp = ObjectParser.string_to_datetime(unpacked[TIMESTAMP])

        if command_type == SubmitOrder.__name__:
            return SubmitOrder(
                Venue(unpacked[VENUE]),
                self.identifier_cache.get_trader_id(unpacked[TRADER_ID]),
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                self.identifier_cache.get_strategy_id(unpacked[STRATEGY_ID]),
                PositionId(unpacked[POSITION_ID]),
                self.order_serializer.deserialize(unpacked[ORDER]),
                command_id,
                command_timestamp,
            )
        elif command_type == SubmitBracketOrder.__name__:
            return SubmitBracketOrder(
                Venue(unpacked[VENUE]),
                self.identifier_cache.get_trader_id(unpacked[TRADER_ID]),
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                self.identifier_cache.get_strategy_id(unpacked[STRATEGY_ID]),
                BracketOrder(self.order_serializer.deserialize(unpacked[ENTRY]),
                             self.order_serializer.deserialize(unpacked[STOP_LOSS]),
                             self.order_serializer.deserialize(unpacked[TAKE_PROFIT])),
                command_id,
                command_timestamp,
            )
        elif command_type == AmendOrder.__name__:
            return AmendOrder(
                Venue(unpacked[VENUE]),
                self.identifier_cache.get_trader_id(unpacked[TRADER_ID]),
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                Quantity(unpacked[QUANTITY]),
                Price(unpacked[PRICE]),
                command_id,
                command_timestamp,
            )
        elif command_type == CancelOrder.__name__:
            return CancelOrder(
                Venue(unpacked[VENUE]),
                self.identifier_cache.get_trader_id(unpacked[TRADER_ID]),
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                OrderId(unpacked[ORDER_ID]),
                command_id,
                command_timestamp,
            )
        else:
            raise RuntimeError("Cannot deserialize command, unrecognized bytes pattern")


cdef class MsgPackEventSerializer(EventSerializer):
    """
    Provides an `Event` serializer for the `MessagePack` specification.

    """

    def __init__(self):
        """
        Initialize a new instance of the `MsgPackEventSerializer` class.

        """
        super().__init__()

        self.identifier_cache = IdentifierCache()

    cpdef bytes serialize(self, Event event):
        """
        Return the MessagePack specification bytes serialized from the given event.

        Parameters
        ----------
        event : Event
            The event to serialize.

        Returns
        -------
        bytes

        Raises
        ------
        RuntimeError
            If the event cannot be serialized.

        """
        Condition.not_none(event, "event")

        cdef dict package = {
            TYPE: type(event).__name__,
            ID: event.id.value,
            TIMESTAMP: ObjectParser.datetime_to_str(event.timestamp),
        }

        if isinstance(event, AccountState):
            package[ACCOUNT_ID] = event.account_id.value
            package[BALANCES] = {b.currency.code: str(b) for b in event.balances}
            package[BALANCES_FREE] = {b.currency.code: str(b) for b in event.balances_free}
            package[BALANCES_LOCKED] = {b.currency.code: str(b) for b in event.balances_locked}
            package[INFO] = event.info
        elif isinstance(event, OrderInitialized):
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[STRATEGY_ID] = event.strategy_id.value
            package[SYMBOL] = event.symbol.value
            package[ORDER_SIDE] = self.convert_snake_to_camel(OrderSideParser.to_str(event.order_side))
            package[ORDER_TYPE] = self.convert_snake_to_camel(OrderTypeParser.to_str(event.order_type))
            package[QUANTITY] = str(event.quantity)
            package[TIME_IN_FORCE] = TimeInForceParser.to_str(event.time_in_force)

            if event.order_type == OrderType.LIMIT:
                package[PRICE] = str(event.options[PRICE])
                package[EXPIRE_TIME] = event.options.get(EXPIRE_TIME)  # Can be None
                package[POST_ONLY] = event.options[POST_ONLY]
                package[REDUCE_ONLY] = event.options[REDUCE_ONLY]
                package[HIDDEN] = event.options[HIDDEN]
            elif event.order_type == OrderType.STOP_MARKET:
                package[PRICE] = str(event.options[PRICE])
                package[EXPIRE_TIME] = event.options.get(EXPIRE_TIME)  # Can be None
                package[REDUCE_ONLY] = event.options[REDUCE_ONLY]

        elif isinstance(event, OrderSubmitted):
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[SUBMITTED_TIME] = ObjectParser.datetime_to_str(event.submitted_time)
        elif isinstance(event, OrderInvalid):
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[REASON] = event.reason
        elif isinstance(event, OrderDenied):
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[REASON] = event.reason
        elif isinstance(event, OrderAccepted):
            package[ACCOUNT_ID] = event.account_id.value
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[ORDER_ID] = event.order_id.value
            package[ACCEPTED_TIME] = ObjectParser.datetime_to_str(event.accepted_time)
        elif isinstance(event, OrderRejected):
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[REJECTED_TIME] = ObjectParser.datetime_to_str(event.rejected_time)
            package[REASON] = event.reason
        elif isinstance(event, OrderCancelReject):
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[REJECTED_TIME] = ObjectParser.datetime_to_str(event.rejected_time)
            package[RESPONSE_TO] = event.response_to
            package[REASON] = event.reason
        elif isinstance(event, OrderCancelled):
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[CANCELLED_TIME] = ObjectParser.datetime_to_str(event.cancelled_time)
        elif isinstance(event, OrderAmended):
            package[ACCOUNT_ID] = event.account_id.value
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[ORDER_ID] = event.order_id.value
            package[AMENDED_TIME] = ObjectParser.datetime_to_str(event.amended_time)
            package[QUANTITY] = str(event.quantity)
            package[PRICE] = str(event.price)
        elif isinstance(event, OrderExpired):
            package[ACCOUNT_ID] = event.account_id.value
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[ORDER_ID] = event.order_id.value
            package[EXPIRED_TIME] = ObjectParser.datetime_to_str(event.expired_time)
        elif isinstance(event, OrderFilled):
            package[ACCOUNT_ID] = event.account_id.value
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[ORDER_ID] = event.order_id.value
            package[EXECUTION_ID] = event.execution_id.value
            package[POSITION_ID] = event.position_id.value
            package[STRATEGY_ID] = event.strategy_id.value
            package[SYMBOL] = event.symbol.value
            package[ORDER_SIDE] = self.convert_snake_to_camel(OrderSideParser.to_str(event.order_side))
            package[FILL_QTY] = str(event.fill_qty)
            package[FILL_PRICE] = str(event.fill_price)
            package[CUM_QTY] = str(event.cum_qty)
            package[LEAVES_QTY] = str(event.leaves_qty)
            package[CURRENCY] = event.currency.code
            package[IS_INVERSE] = event.is_inverse
            package[COMMISSION] = str(event.commission)
            package[COMMISSION_CURRENCY] = event.commission.currency.code
            package[LIQUIDITY_SIDE] = LiquiditySideParser.to_str(event.liquidity_side)
            package[EXECUTION_TIME] = ObjectParser.datetime_to_str(event.execution_time)
        else:
            raise RuntimeError("Cannot serialize event, unrecognized event")

        return MsgPackSerializer.serialize(package)

    cpdef Event deserialize(self, bytes event_bytes):
        """
        Return the event deserialized from the given MessagePack specification event_bytes.

        Parameters
        ----------
        event_bytes
            The bytes to deserialize.

        Returns
        -------
        Event

        Raises
        ------
        ValueError
            If event_bytes is empty.
        RuntimeError
            If event cannot be deserialized.

        """
        Condition.not_empty(event_bytes, "event_bytes")

        cdef dict unpacked = MsgPackSerializer.deserialize(event_bytes)  # type: dict[str, bytes]

        cdef str event_type = unpacked[TYPE]
        cdef UUID event_id = UUID.from_str_c(unpacked[ID])
        cdef datetime event_timestamp = ObjectParser.string_to_datetime(unpacked[TIMESTAMP])

        cdef dict options          # typing for OrderInitialized
        cdef OrderType order_type  # typing for OrderInitialized
        if event_type == AccountState.__name__:
            return AccountState(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                [Money(v, Currency.from_str_c(k)) for k, v in unpacked[BALANCES].items()],
                [Money(v, Currency.from_str_c(k)) for k, v in unpacked[BALANCES_FREE].items()],
                [Money(v, Currency.from_str_c(k)) for k, v in unpacked[BALANCES_LOCKED].items()],
                unpacked[INFO],
                event_id,
                event_timestamp,
            )
        elif event_type == OrderInitialized.__name__:
            options = {}
            order_type = OrderTypeParser.from_str(self.convert_camel_to_snake(unpacked[ORDER_TYPE]))
            if order_type == OrderType.LIMIT:
                options[PRICE] = Price(unpacked[PRICE])
                options[EXPIRE_TIME] = unpacked[EXPIRE_TIME]
                options[POST_ONLY] = unpacked[POST_ONLY]
                options[REDUCE_ONLY] = unpacked[REDUCE_ONLY]
                options[HIDDEN] = unpacked[HIDDEN]
            elif order_type == OrderType.STOP_MARKET:
                options[PRICE] = Price(unpacked[PRICE])
                options[EXPIRE_TIME] = unpacked[EXPIRE_TIME]
                options[REDUCE_ONLY] = unpacked[REDUCE_ONLY]

            return OrderInitialized(
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                self.identifier_cache.get_strategy_id(unpacked[STRATEGY_ID]),
                self.identifier_cache.get_symbol(unpacked[SYMBOL]),
                OrderSideParser.from_str(self.convert_camel_to_snake(unpacked[ORDER_SIDE])),
                order_type,
                Quantity(unpacked[QUANTITY]),
                TimeInForceParser.from_str(unpacked[TIME_IN_FORCE]),
                event_id,
                event_timestamp,
                options,
            )
        elif event_type == OrderSubmitted.__name__:
            return OrderSubmitted(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                ObjectParser.string_to_datetime(unpacked[SUBMITTED_TIME]),
                event_id,
                event_timestamp,
            )
        elif event_type == OrderInvalid.__name__:
            return OrderInvalid(
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                unpacked[REASON],
                event_id,
                event_timestamp,
            )
        elif event_type == OrderDenied.__name__:
            return OrderDenied(
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                unpacked[REASON],
                event_id,
                event_timestamp,
            )
        elif event_type == OrderAccepted.__name__:
            return OrderAccepted(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                OrderId(unpacked[ORDER_ID]),
                ObjectParser.string_to_datetime(unpacked[ACCEPTED_TIME]),
                event_id,
                event_timestamp,
            )
        elif event_type == OrderRejected.__name__:
            return OrderRejected(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                ObjectParser.string_to_datetime(unpacked[REJECTED_TIME]),
                unpacked[REASON],
                event_id,
                event_timestamp,
            )
        elif event_type == OrderCancelled.__name__:
            return OrderCancelled(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                OrderId(unpacked[ORDER_ID]),
                ObjectParser.string_to_datetime(unpacked[CANCELLED_TIME]),
                event_id,
                event_timestamp,
            )
        elif event_type == OrderCancelReject.__name__:
            return OrderCancelReject(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                OrderId(unpacked[ORDER_ID]),
                ObjectParser.string_to_datetime(unpacked[REJECTED_TIME]),
                unpacked[RESPONSE_TO],
                unpacked[REASON],
                event_id,
                event_timestamp,
            )
        elif event_type == OrderAmended.__name__:
            return OrderAmended(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                OrderId(unpacked[ORDER_ID]),
                Quantity(unpacked[QUANTITY]),
                Price(unpacked[PRICE]),
                ObjectParser.string_to_datetime(unpacked[AMENDED_TIME]),
                event_id,
                event_timestamp,
            )
        elif event_type == OrderExpired.__name__:
            return OrderExpired(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                OrderId(unpacked[ORDER_ID]),
                ObjectParser.string_to_datetime(unpacked[EXPIRED_TIME]),
                event_id,
                event_timestamp,
            )
        elif event_type == OrderFilled.__name__:
            commission_currency = Currency.from_str_c(unpacked[COMMISSION_CURRENCY])
            return OrderFilled(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                OrderId(unpacked[ORDER_ID]),
                ExecutionId(unpacked[EXECUTION_ID]),
                PositionId(unpacked[POSITION_ID]),
                self.identifier_cache.get_strategy_id(unpacked[STRATEGY_ID]),
                self.identifier_cache.get_symbol(unpacked[SYMBOL]),
                OrderSideParser.from_str(self.convert_camel_to_snake(unpacked[ORDER_SIDE])),
                Quantity(unpacked[FILL_QTY]),
                Quantity(unpacked[CUM_QTY]),
                Quantity(unpacked[LEAVES_QTY]),
                Price(unpacked[FILL_PRICE]),
                Currency.from_str_c(unpacked[CURRENCY]),
                unpacked[IS_INVERSE],
                Money(unpacked[COMMISSION], commission_currency),
                LiquiditySideParser.from_str(unpacked[LIQUIDITY_SIDE]),
                ObjectParser.string_to_datetime(unpacked[EXECUTION_TIME]),
                event_id,
                event_timestamp,
            )
        else:
            raise RuntimeError(f"Cannot deserialize event, unrecognized event {event_type}")
