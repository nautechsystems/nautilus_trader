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

import msgpack

from cpython.datetime cimport datetime

from nautilus_trader.common.cache cimport IdentifierCache
from nautilus_trader.core.cache cimport ObjectCache
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.currency cimport currency_from_string
from nautilus_trader.model.c_enums.currency cimport currency_to_string
from nautilus_trader.model.c_enums.liquidity_side cimport liquidity_side_from_string
from nautilus_trader.model.c_enums.liquidity_side cimport liquidity_side_to_string
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport order_side_from_string
from nautilus_trader.model.c_enums.order_side cimport order_side_to_string
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_type cimport order_type_from_string
from nautilus_trader.model.c_enums.order_type cimport order_type_to_string
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport time_in_force_from_string
from nautilus_trader.model.c_enums.time_in_force cimport time_in_force_to_string
from nautilus_trader.model.commands cimport AccountInquiry
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport ModifyOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.events cimport OrderAccepted
from nautilus_trader.model.events cimport OrderCancelReject
from nautilus_trader.model.events cimport OrderCancelled
from nautilus_trader.model.events cimport OrderDenied
from nautilus_trader.model.events cimport OrderExpired
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderInitialized
from nautilus_trader.model.events cimport OrderInvalid
from nautilus_trader.model.events cimport OrderModified
from nautilus_trader.model.events cimport OrderRejected
from nautilus_trader.model.events cimport OrderSubmitted
from nautilus_trader.model.events cimport OrderWorking
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Decimal
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order cimport BracketOrder
from nautilus_trader.model.order cimport LimitOrder
from nautilus_trader.model.order cimport MarketOrder
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.order cimport PassiveOrder
from nautilus_trader.model.order cimport StopOrder
from nautilus_trader.serialization.base cimport CommandSerializer
from nautilus_trader.serialization.base cimport EventSerializer
from nautilus_trader.serialization.base cimport OrderSerializer
from nautilus_trader.serialization.common cimport convert_datetime_to_string
from nautilus_trader.serialization.common cimport convert_price_to_string
from nautilus_trader.serialization.common cimport convert_string_to_datetime
from nautilus_trader.serialization.common cimport convert_string_to_price
from nautilus_trader.serialization.constants cimport *


cdef class MsgPackSerializer:
    """
    Provides a serializer for the MessagePack specification.
    """
    @staticmethod
    cdef bytes serialize(dict message):
        """
        Serialize the given message to MessagePack specification bytes.

        :param message: The message to serialize.

        :return bytes.
        """
        Condition.not_none(message, "message")

        return msgpack.packb(message, use_bin_type=False)

    @staticmethod
    cdef dict deserialize(bytes message_bytes, bint raw_values=True):
        """
        Deserialize the given MessagePack specification bytes to a dictionary.

        :param message_bytes: The message bytes to deserialize.
        :param raw_values: If the values should be deserialized as raw bytes.
        :return Dict.
        """
        Condition.not_none(message_bytes, "message_bytes")

        cdef dict raw_unpacked = msgpack.unpackb(message_bytes, raw=True)

        cdef bytes k, v
        if raw_values:
            return {k.decode(UTF8): v for k, v in raw_unpacked.items()}
        return {k.decode(UTF8): v.decode(UTF8) for k, v in raw_unpacked.items()}


cdef class MsgPackDictionarySerializer(DictionarySerializer):
    """
    Provides a serializer for dictionaries for the MsgPack specification.
    """

    def __init__(self):
        """
        Initialize a new instance of the MsgPackDictionarySerializer class.
        """
        super().__init__()

    cpdef bytes serialize(self, dict dictionary):
        """
        Serialize the given dictionary with string keys and values to bytes.

        :param dictionary: The dictionary to serialize.
        :return bytes.
        """
        Condition.not_none(dictionary, "dictionary")

        return MsgPackSerializer.serialize(dictionary)

    cpdef dict deserialize(self, bytes dictionary_bytes):
        """
        Deserialize the given bytes to a dictionary with string keys and values.

        :param dictionary_bytes: The dictionary bytes to deserialize.
        :return Dict.
        """
        Condition.not_none(dictionary_bytes, "dictionary_bytes")

        return MsgPackSerializer.deserialize(dictionary_bytes, raw_values=False)


cdef class MsgPackOrderSerializer(OrderSerializer):
    """
    Provides a command serializer for the MessagePack specification.
    """

    def __init__(self):
        """
        Initialize a new instance of the MsgPackOrderSerializer class.
        """
        super().__init__()

        self.symbol_cache = ObjectCache(Symbol, Symbol.from_string)

    cpdef bytes serialize(self, Order order):  # Can be None
        """
        Return the serialized MessagePack specification bytes from the given order.

        :param order: The order to serialize.
        :return bytes.
        """
        if order is None:
            return MsgPackSerializer.serialize({})  # Null order

        cdef dict package = {
            ID: order.cl_ord_id.value,
            SYMBOL: order.symbol.value,
            ORDER_SIDE: self.convert_snake_to_camel(order_side_to_string(order.side)),
            ORDER_TYPE: self.convert_snake_to_camel(order_type_to_string(order.type)),
            QUANTITY: str(order.quantity),
            TIME_IN_FORCE: time_in_force_to_string(order.time_in_force),
            INIT_ID: order.init_id.value,
            TIMESTAMP: convert_datetime_to_string(order.timestamp),
        }

        if isinstance(order, PassiveOrder):
            package[PRICE] = convert_price_to_string(order.price)
            package[EXPIRE_TIME] = convert_datetime_to_string(order.expire_time)

        if isinstance(order, LimitOrder):
            package[POST_ONLY] = str(order.is_post_only)
            package[HIDDEN] = str(order.is_hidden)

        return MsgPackSerializer.serialize(package)

    cpdef Order deserialize(self, bytes order_bytes):
        """
        Return the order deserialized from the given MessagePack specification bytes.

        :param order_bytes: The bytes to deserialize.
        :return Order.
        :raises ValueError: If the event_bytes is empty.
        """
        Condition.not_empty(order_bytes, "order_bytes")

        cdef dict unpacked = MsgPackSerializer.deserialize(order_bytes)

        if not unpacked:
            return None  # Null order

        cdef ClientOrderId cl_ord_id = ClientOrderId(unpacked[ID].decode(UTF8))
        cdef Symbol symbol = self.symbol_cache.get(unpacked[SYMBOL].decode(UTF8))
        cdef OrderSide order_side = order_side_from_string(self.convert_camel_to_snake(unpacked[ORDER_SIDE].decode(UTF8)))
        cdef OrderType order_type = order_type_from_string(self.convert_camel_to_snake(unpacked[ORDER_TYPE].decode(UTF8)))
        cdef Quantity quantity = Quantity(unpacked[QUANTITY].decode(UTF8))
        cdef TimeInForce time_in_force = time_in_force_from_string(unpacked[TIME_IN_FORCE].decode(UTF8))
        cdef UUID init_id = UUID(unpacked[INIT_ID].decode(UTF8))
        cdef datetime timestamp = convert_string_to_datetime(unpacked[TIMESTAMP].decode(UTF8))

        if order_type == OrderType.MARKET:
            return MarketOrder(
                cl_ord_id=cl_ord_id,
                symbol=symbol,
                order_side=order_side,
                quantity=quantity,
                time_in_force=time_in_force,
                init_id=init_id,
                timestamp=timestamp,
            )

        if order_type == OrderType.LIMIT:
            return LimitOrder(
                cl_ord_id=cl_ord_id,
                symbol=symbol,
                order_side=order_side,
                quantity=quantity,
                price=convert_string_to_price(unpacked[PRICE].decode(UTF8)),
                time_in_force=time_in_force,
                expire_time=convert_string_to_datetime(unpacked[EXPIRE_TIME].decode(UTF8)),
                init_id=init_id,
                timestamp=timestamp,
                post_only=unpacked[POST_ONLY].decode(UTF8) == str(True),
                hidden=unpacked[HIDDEN].decode(UTF8) == str(True),
            )

        if order_type == OrderType.STOP:
            return StopOrder(
                cl_ord_id=cl_ord_id,
                symbol=symbol,
                order_side=order_side,
                quantity=quantity,
                price=convert_string_to_price(unpacked[PRICE].decode(UTF8)),
                time_in_force=time_in_force,
                expire_time=convert_string_to_datetime(unpacked[EXPIRE_TIME].decode(UTF8)),
                init_id=init_id,
                timestamp=timestamp,
            )

        raise ValueError(f"Invalid order_type, was {order_type_to_string(order_type)}")


cdef class MsgPackCommandSerializer(CommandSerializer):
    """
    Provides a command serializer for the MessagePack specification.
    """

    def __init__(self):
        """
        Initialize a new instance of the MsgPackCommandSerializer class.
        """
        super().__init__()

        self.identifier_cache = IdentifierCache()
        self.order_serializer = MsgPackOrderSerializer()

    cpdef bytes serialize(self, Command command):
        """
        Return the serialized MessagePack specification bytes from the given command.

        :param command: The command to serialize.
        :return bytes.
        :raises: RuntimeError: If the command cannot be serialized.
        """
        Condition.not_none(command, "command")

        cdef dict package = {
            TYPE: command.__class__.__name__,
            ID: command.id.value,
            TIMESTAMP: convert_datetime_to_string(command.timestamp),
        }

        if isinstance(command, AccountInquiry):
            package[TRADER_ID] = command.trader_id.value
            package[ACCOUNT_ID] = command.account_id.value
        elif isinstance(command, SubmitOrder):
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
        elif isinstance(command, ModifyOrder):
            package[VENUE] = command.venue.value
            package[TRADER_ID] = command.trader_id.value
            package[ACCOUNT_ID] = command.account_id.value
            package[CLIENT_ORDER_ID] = command.cl_ord_id.value
            package[QUANTITY] = command.quantity.to_string()
            package[PRICE] = command.price.to_string()
        elif isinstance(command, CancelOrder):
            package[VENUE] = command.venue.value
            package[TRADER_ID] = command.trader_id.value
            package[ACCOUNT_ID] = command.account_id.value
            package[CLIENT_ORDER_ID] = command.cl_ord_id.value
        else:
            raise RuntimeError("Cannot serialize command (unrecognized command).")

        return MsgPackSerializer.serialize(package)

    cpdef Command deserialize(self, bytes command_bytes):
        """
        Return the command deserialize from the given MessagePack specification command_bytes.

        :param command_bytes: The command to deserialize.
        :return Command.
        :raises ValueError: If the command_bytes is empty.
        :raises RuntimeError: If the command cannot be deserialized.
        """
        Condition.not_empty(command_bytes, "command_bytes")

        cdef dict unpacked = MsgPackSerializer.deserialize(command_bytes)  # type: {str, bytes}

        cdef str command_type = unpacked[TYPE].decode(UTF8)
        cdef UUID command_id = UUID(unpacked[ID].decode(UTF8))
        cdef datetime command_timestamp = convert_string_to_datetime(unpacked[TIMESTAMP].decode(UTF8))

        if command_type == AccountInquiry.__name__:
            return AccountInquiry(
                self.identifier_cache.get_trader_id(unpacked[TRADER_ID].decode(UTF8)),
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                command_id,
                command_timestamp,
            )
        elif command_type == SubmitOrder.__name__:
            return SubmitOrder(
                Venue(unpacked[VENUE].decode(UTF8)),
                self.identifier_cache.get_trader_id(unpacked[TRADER_ID].decode(UTF8)),
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                self.identifier_cache.get_strategy_id(unpacked[STRATEGY_ID].decode(UTF8)),
                PositionId(unpacked[POSITION_ID].decode(UTF8)),
                self.order_serializer.deserialize(unpacked[ORDER]),
                command_id,
                command_timestamp,
            )
        elif command_type == SubmitBracketOrder.__name__:
            return SubmitBracketOrder(
                Venue(unpacked[VENUE].decode(UTF8)),
                self.identifier_cache.get_trader_id(unpacked[TRADER_ID].decode(UTF8)),
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                self.identifier_cache.get_strategy_id(unpacked[STRATEGY_ID].decode(UTF8)),
                BracketOrder(self.order_serializer.deserialize(unpacked[ENTRY]),
                             self.order_serializer.deserialize(unpacked[STOP_LOSS]),
                             self.order_serializer.deserialize(unpacked[TAKE_PROFIT])),
                command_id,
                command_timestamp,
            )
        elif command_type == ModifyOrder.__name__:
            return ModifyOrder(
                Venue(unpacked[VENUE].decode(UTF8)),
                self.identifier_cache.get_trader_id(unpacked[TRADER_ID].decode(UTF8)),
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                ClientOrderId(unpacked[CLIENT_ORDER_ID].decode(UTF8)),
                Quantity(unpacked[QUANTITY].decode(UTF8)),
                convert_string_to_price(unpacked[PRICE].decode(UTF8)),
                command_id,
                command_timestamp,
            )
        elif command_type == CancelOrder.__name__:
            return CancelOrder(
                Venue(unpacked[VENUE].decode(UTF8)),
                self.identifier_cache.get_trader_id(unpacked[TRADER_ID].decode(UTF8)),
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                ClientOrderId(unpacked[CLIENT_ORDER_ID].decode(UTF8)),
                command_id,
                command_timestamp,
            )
        else:
            raise RuntimeError("Cannot deserialize command (unrecognized bytes pattern).")


cdef class MsgPackEventSerializer(EventSerializer):
    """
    Provides an event serializer for the MessagePack specification
    """

    def __init__(self):
        """
        Initialize a new instance of the MsgPackEventSerializer class.
        """
        super().__init__()

        self.identifier_cache = IdentifierCache()

    cpdef bytes serialize(self, Event event):
        """
        Return the MessagePack specification bytes serialized from the given event.

        :param event: The event to serialize.
        :return bytes.
        :raises: RuntimeError: If the event cannot be serialized.
        """
        Condition.not_none(event, "event")

        cdef dict package = {
            TYPE: event.__class__.__name__,
            ID: event.id.value,
            TIMESTAMP: convert_datetime_to_string(event.timestamp),
        }

        if isinstance(event, AccountState):
            package[ACCOUNT_ID] = event.account_id.value
            package[QUOTE_CURRENCY] = currency_to_string(event.currency)
            package[CASH_BALANCE] = event.cash_balance.to_string()
            package[CASH_START_DAY] = event.cash_start_day.to_string()
            package[CASH_ACTIVITY_DAY] = event.cash_activity_day.to_string()
            package[MARGIN_USED_LIQUIDATION] = event.margin_used_liquidation.to_string()
            package[MARGIN_USED_MAINTENANCE] = event.margin_used_maintenance.to_string()
            package[MARGIN_RATIO] = event.margin_ratio.to_string()
            package[MARGIN_CALL_STATUS] = event.margin_call_status
        elif isinstance(event, OrderInitialized):
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[SYMBOL] = event.symbol.value
            package[ORDER_SIDE] = self.convert_snake_to_camel(order_side_to_string(event.order_side))
            package[ORDER_TYPE] = self.convert_snake_to_camel(order_type_to_string(event.order_type))
            package[QUANTITY] = event.quantity.to_string()
            package[TIME_IN_FORCE] = time_in_force_to_string(event.time_in_force)
            package[OPTIONS] = MsgPackSerializer.serialize(event.options)
        elif isinstance(event, OrderSubmitted):
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[SUBMITTED_TIME] = convert_datetime_to_string(event.submitted_time)
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
            package[ACCEPTED_TIME] = convert_datetime_to_string(event.accepted_time)
        elif isinstance(event, OrderRejected):
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[REJECTED_TIME] = convert_datetime_to_string(event.rejected_time)
            package[REASON] = event.reason
        elif isinstance(event, OrderWorking):
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[ORDER_ID] = event.order_id.value
            package[SYMBOL] = event.symbol.value
            package[ORDER_SIDE] = self.convert_snake_to_camel(order_side_to_string(event.order_side))
            package[ORDER_TYPE] = self.convert_snake_to_camel(order_type_to_string(event.order_type))
            package[QUANTITY] = event.quantity.to_string()
            package[PRICE] = event.price.to_string()
            package[TIME_IN_FORCE] = time_in_force_to_string(event.time_in_force)
            package[EXPIRE_TIME] = convert_datetime_to_string(event.expire_time)
            package[WORKING_TIME] = convert_datetime_to_string(event.working_time)
        elif isinstance(event, OrderCancelReject):
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[REJECTED_TIME] = convert_datetime_to_string(event.rejected_time)
            package[RESPONSE_TO] = event.response_to
            package[REASON] = event.reason
        elif isinstance(event, OrderCancelled):
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[CANCELLED_TIME] = convert_datetime_to_string(event.cancelled_time)
        elif isinstance(event, OrderModified):
            package[ACCOUNT_ID] = event.account_id.value
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[ORDER_ID] = event.order_id.value
            package[MODIFIED_TIME] = convert_datetime_to_string(event.modified_time)
            package[MODIFIED_QUANTITY] = event.modified_quantity.to_string()
            package[MODIFIED_PRICE] = event.modified_price.to_string()
        elif isinstance(event, OrderExpired):
            package[ACCOUNT_ID] = event.account_id.value
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[ORDER_ID] = event.order_id.value
            package[EXPIRED_TIME] = convert_datetime_to_string(event.expired_time)
        elif isinstance(event, OrderFilled):
            package[ACCOUNT_ID] = event.account_id.value
            package[CLIENT_ORDER_ID] = event.cl_ord_id.value
            package[ORDER_ID] = event.order_id.value
            package[EXECUTION_ID] = event.execution_id.value
            package[POSITION_ID] = event.position_id.value
            package[STRATEGY_ID] = event.strategy_id.value
            package[SYMBOL] = event.symbol.value
            package[ORDER_SIDE] = self.convert_snake_to_camel(order_side_to_string(event.order_side))
            package[FILLED_QUANTITY] = event.filled_qty.to_string()
            package[LEAVES_QUANTITY] = event.leaves_qty.to_string()
            package[AVERAGE_PRICE] = event.avg_price.to_string()
            package[COMMISSION] = event.commission.to_string()
            package[COMMISSION_CURRENCY] = currency_to_string(event.commission.currency)
            package[LIQUIDITY_SIDE] = liquidity_side_to_string(event.liquidity_side)
            package[BASE_CURRENCY] = currency_to_string(event.quote_currency)
            package[QUOTE_CURRENCY] = currency_to_string(event.quote_currency)
            package[EXECUTION_TIME] = convert_datetime_to_string(event.execution_time)
        else:
            raise RuntimeError("Cannot serialize event (unrecognized event.")

        return MsgPackSerializer.serialize(package)

    cpdef Event deserialize(self, bytes event_bytes):
        """
        Return the event deserialized from the given MessagePack specification event_bytes.

        :param event_bytes: The bytes to deserialize.
        :return Event.
        :raises ValueError: If the event_bytes is empty.
        :raises RuntimeError: If the event cannot be deserialized.
        """
        Condition.not_empty(event_bytes, "event_bytes")

        cdef dict unpacked = MsgPackSerializer.deserialize(event_bytes)  # type: {str, bytes}

        cdef str event_type = unpacked[TYPE].decode(UTF8)
        cdef UUID event_id = UUID(unpacked[ID].decode(UTF8))
        cdef datetime event_timestamp = convert_string_to_datetime(unpacked[TIMESTAMP].decode(UTF8))

        cdef Currency currency
        if event_type == AccountState.__name__:
            currency = currency_from_string(unpacked[QUOTE_CURRENCY].decode(UTF8))
            return AccountState(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                currency,
                Money(unpacked[CASH_BALANCE].decode(UTF8), currency),
                Money(unpacked[CASH_START_DAY].decode(UTF8), currency),
                Money(unpacked[CASH_ACTIVITY_DAY].decode(UTF8), currency),
                Money(unpacked[MARGIN_USED_LIQUIDATION].decode(UTF8), currency),
                Money(unpacked[MARGIN_USED_MAINTENANCE].decode(UTF8), currency),
                Decimal(unpacked[MARGIN_RATIO].decode(UTF8)),
                unpacked[MARGIN_CALL_STATUS].decode(UTF8),
                event_id,
                event_timestamp,
            )
        elif event_type == OrderInitialized.__name__:
            return OrderInitialized(
                ClientOrderId(unpacked[CLIENT_ORDER_ID].decode(UTF8)),
                self.identifier_cache.get_symbol(unpacked[SYMBOL].decode(UTF8)),
                order_side_from_string(self.convert_camel_to_snake(unpacked[ORDER_SIDE].decode(UTF8))),
                order_type_from_string(self.convert_camel_to_snake(unpacked[ORDER_TYPE].decode(UTF8))),
                Quantity(unpacked[QUANTITY].decode(UTF8)),
                time_in_force_from_string(unpacked[TIME_IN_FORCE].decode(UTF8)),
                event_id,
                event_timestamp,
                MsgPackSerializer.deserialize(unpacked[OPTIONS], raw_values=False),
            )
        elif event_type == OrderSubmitted.__name__:
            return OrderSubmitted(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                ClientOrderId(unpacked[CLIENT_ORDER_ID].decode(UTF8)),
                convert_string_to_datetime(unpacked[SUBMITTED_TIME].decode(UTF8)),
                event_id,
                event_timestamp,
            )
        elif event_type == OrderInvalid.__name__:
            return OrderInvalid(
                ClientOrderId(unpacked[CLIENT_ORDER_ID].decode(UTF8)),
                unpacked[REASON].decode(UTF8),
                event_id,
                event_timestamp,
            )
        elif event_type == OrderDenied.__name__:
            return OrderDenied(
                ClientOrderId(unpacked[CLIENT_ORDER_ID].decode(UTF8)),
                unpacked[REASON].decode(UTF8),
                event_id,
                event_timestamp,
            )
        elif event_type == OrderAccepted.__name__:
            return OrderAccepted(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                ClientOrderId(unpacked[CLIENT_ORDER_ID].decode(UTF8)),
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                convert_string_to_datetime(unpacked[ACCEPTED_TIME].decode(UTF8)),
                event_id,
                event_timestamp,
            )
        elif event_type == OrderRejected.__name__:
            return OrderRejected(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                ClientOrderId(unpacked[CLIENT_ORDER_ID].decode(UTF8)),
                convert_string_to_datetime(unpacked[REJECTED_TIME].decode(UTF8)),
                unpacked[REASON].decode(UTF8),
                event_id,
                event_timestamp,
            )
        elif event_type == OrderWorking.__name__:
            return OrderWorking(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                ClientOrderId(unpacked[CLIENT_ORDER_ID].decode(UTF8)),
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                Symbol.from_string(unpacked[SYMBOL].decode(UTF8)),
                order_side_from_string(self.convert_camel_to_snake(unpacked[ORDER_SIDE].decode(UTF8))),
                order_type_from_string(self.convert_camel_to_snake(unpacked[ORDER_TYPE].decode(UTF8))),
                Quantity(unpacked[QUANTITY].decode(UTF8)),
                convert_string_to_price(unpacked[PRICE].decode(UTF8)),
                time_in_force_from_string(unpacked[TIME_IN_FORCE].decode(UTF8)),
                convert_string_to_datetime(unpacked[EXPIRE_TIME].decode(UTF8)),
                convert_string_to_datetime(unpacked[WORKING_TIME].decode(UTF8)),
                event_id,
                event_timestamp,
            )
        elif event_type == OrderCancelled.__name__:
            return OrderCancelled(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                ClientOrderId(unpacked[CLIENT_ORDER_ID].decode(UTF8)),
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                convert_string_to_datetime(unpacked[CANCELLED_TIME].decode(UTF8)),
                event_id,
                event_timestamp,
            )
        elif event_type == OrderCancelReject.__name__:
            return OrderCancelReject(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                ClientOrderId(unpacked[CLIENT_ORDER_ID].decode(UTF8)),
                convert_string_to_datetime(unpacked[REJECTED_TIME].decode(UTF8)),
                unpacked[RESPONSE_TO].decode(UTF8),
                unpacked[REASON].decode(UTF8),
                event_id,
                event_timestamp,
            )
        elif event_type == OrderModified.__name__:
            return OrderModified(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                ClientOrderId(unpacked[CLIENT_ORDER_ID].decode(UTF8)),
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                Quantity(unpacked[MODIFIED_QUANTITY].decode(UTF8)),
                convert_string_to_price(unpacked[MODIFIED_PRICE].decode(UTF8)),
                convert_string_to_datetime(unpacked[MODIFIED_TIME].decode(UTF8)),
                event_id,
                event_timestamp,
            )
        elif event_type == OrderExpired.__name__:
            return OrderExpired(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                ClientOrderId(unpacked[CLIENT_ORDER_ID].decode(UTF8)),
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                convert_string_to_datetime(unpacked[EXPIRED_TIME].decode(UTF8)),
                event_id,
                event_timestamp,
            )
        elif event_type == OrderFilled.__name__:
            commission_currency = currency_from_string(unpacked[COMMISSION_CURRENCY].decode(UTF8))
            # TODO: Optimize
            strategy_id_pieces = unpacked[STRATEGY_ID].decode(UTF8).split('-')
            strategy_id = StrategyId(strategy_id_pieces[0], strategy_id_pieces[1])
            return OrderFilled(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                ClientOrderId(unpacked[CLIENT_ORDER_ID].decode(UTF8)),
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                ExecutionId(unpacked[EXECUTION_ID].decode(UTF8)),
                PositionId(unpacked[POSITION_ID].decode(UTF8)),
                strategy_id,
                self.identifier_cache.get_symbol(unpacked[SYMBOL].decode(UTF8)),
                order_side_from_string(self.convert_camel_to_snake(unpacked[ORDER_SIDE].decode(UTF8))),
                Quantity(unpacked[FILLED_QUANTITY].decode(UTF8)),
                Quantity(unpacked[LEAVES_QUANTITY].decode(UTF8)),
                convert_string_to_price(unpacked[AVERAGE_PRICE].decode(UTF8)),
                Money(unpacked[COMMISSION].decode(UTF8), commission_currency),
                liquidity_side_from_string(unpacked[LIQUIDITY_SIDE].decode(UTF8)),
                currency_from_string(unpacked[BASE_CURRENCY].decode(UTF8)),
                currency_from_string(unpacked[QUOTE_CURRENCY].decode(UTF8)),
                convert_string_to_datetime(unpacked[EXECUTION_TIME].decode(UTF8)),
                event_id,
                event_timestamp,
            )
        else:
            raise RuntimeError(f"Cannot deserialize event (unrecognized event {event_type}).")
