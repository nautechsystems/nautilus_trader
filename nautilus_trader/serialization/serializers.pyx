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
from uuid import UUID

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.cache cimport ObjectCache
from nautilus_trader.core.types cimport ValidString, GUID
from nautilus_trader.core.message cimport Command, Event, Request, Response
from nautilus_trader.model.c_enums.time_in_force cimport time_in_force_to_string, time_in_force_from_string
from nautilus_trader.model.c_enums.order_side cimport  order_side_to_string, order_side_from_string
from nautilus_trader.model.c_enums.order_type cimport order_type_to_string, order_type_from_string
from nautilus_trader.model.c_enums.order_purpose cimport order_purpose_to_string, order_purpose_from_string
from nautilus_trader.model.c_enums.currency cimport Currency, currency_to_string, currency_from_string
from nautilus_trader.model.identifiers cimport Symbol, OrderId, OrderIdBroker, ExecutionId
from nautilus_trader.model.identifiers cimport PositionId, PositionIdBroker
from nautilus_trader.model.objects cimport Quantity, Decimal, Money
from nautilus_trader.model.commands cimport AccountInquiry, SubmitOrder, SubmitAtomicOrder
from nautilus_trader.model.commands cimport ModifyOrder, CancelOrder
from nautilus_trader.model.events cimport AccountStateEvent, OrderInitialized, OrderInvalid
from nautilus_trader.model.events cimport OrderDenied, OrderSubmitted, OrderAccepted, OrderRejected
from nautilus_trader.model.events cimport OrderWorking, OrderExpired, OrderModified, OrderCancelled
from nautilus_trader.model.events cimport OrderCancelReject, OrderPartiallyFilled, OrderFilled
from nautilus_trader.model.order cimport Order, AtomicOrder
from nautilus_trader.common.cache cimport IdentifierCache
from nautilus_trader.common.logging cimport LogMessage, log_level_from_string
from nautilus_trader.serialization.constants cimport *
from nautilus_trader.serialization.base cimport OrderSerializer, CommandSerializer, EventSerializer
from nautilus_trader.serialization.base cimport RequestSerializer, ResponseSerializer, LogSerializer
from nautilus_trader.serialization.common cimport convert_string_to_price, convert_price_to_string
from nautilus_trader.serialization.common cimport convert_string_to_label, convert_label_to_string
from nautilus_trader.serialization.common cimport convert_string_to_datetime, convert_datetime_to_string
from nautilus_trader.network.identifiers cimport ClientId, ServerId, SessionId
from nautilus_trader.network.messages cimport Connect, Connected, Disconnect, Disconnected
from nautilus_trader.network.messages cimport MessageReceived, MessageRejected, QueryFailure
from nautilus_trader.network.messages cimport DataRequest, DataResponse


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
        Condition.not_none(message, 'message')

        return msgpack.packb(message)

    @staticmethod
    cdef dict deserialize(bytes message_bytes, bint raw_values=True):
        """
        Deserialize the given MessagePack specification bytes to a dictionary.

        :param message_bytes: The message bytes to deserialize.
        :param raw_values: If the values should be deserialized as raw bytes.
        :return Dict.
        """
        Condition.not_none(message_bytes, 'message_bytes')

        cdef dict raw_unpacked = msgpack.unpackb(message_bytes, raw=True)

        cdef bytes k, v
        if raw_values:
            return { k.decode(UTF8): v for k, v in raw_unpacked.items()}
        return { k.decode(UTF8): v.decode(UTF8) for k, v in raw_unpacked.items()}


cdef class MsgPackDictionarySerializer(DictionarySerializer):
    """
    Provides a serializer for dictionaries for the MsgPack specification.
    """

    def __init__(self):
        """
        Initializes a new instance of the MsgPackDictionarySerializer class.
        """
        super().__init__()

    cpdef bytes serialize(self, dict dictionary):
        """
        Serialize the given dictionary with string keys and values to bytes.

        :param dictionary: The dictionary to serialize.
        :return bytes.
        """
        Condition.not_none(dictionary, 'dictionary')

        return MsgPackSerializer.serialize(dictionary)

    cpdef dict deserialize(self, bytes dictionary_bytes):
        """
        Deserialize the given bytes to a dictionary with string keys and values.

        :param dictionary_bytes: The dictionary bytes to deserialize.
        :return Dict.
        """
        Condition.not_none(dictionary_bytes, 'dictionary_bytes')

        return MsgPackSerializer.deserialize(dictionary_bytes, raw_values=False)


cdef class MsgPackOrderSerializer(OrderSerializer):
    """
    Provides a command serializer for the MessagePack specification.
    """

    def __init__(self):
        """
        Initializes a new instance of the MsgPackOrderSerializer class.
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

        return MsgPackSerializer.serialize({
            ID: order.id.value,
            SYMBOL: order.symbol.value,
            ORDER_SIDE: self.convert_snake_to_camel(order_side_to_string(order.side)),
            ORDER_TYPE: self.convert_snake_to_camel(order_type_to_string(order.type)),
            QUANTITY: order.quantity.to_string(),
            PRICE: convert_price_to_string(order.price),
            LABEL: convert_label_to_string(order.label),
            ORDER_PURPOSE: self.convert_snake_to_camel(order_purpose_to_string(order.purpose)),
            TIME_IN_FORCE: time_in_force_to_string(order.time_in_force),
            EXPIRE_TIME: convert_datetime_to_string(order.expire_time),
            INIT_ID: order.init_id.value,
            TIMESTAMP: convert_datetime_to_string(order.timestamp),
        })

    cpdef Order deserialize(self, bytes order_bytes):
        """
        Return the order deserialized from the given MessagePack specification bytes.

        :param order_bytes: The bytes to deserialize.
        :return Order.
        :raises ValueError: If the event_bytes is empty.
        """
        Condition.not_empty(order_bytes, 'order_bytes')

        cdef dict unpacked = MsgPackSerializer.deserialize(order_bytes)

        if not unpacked:
            return None  # Null order

        return Order(order_id=OrderId(unpacked[ID].decode(UTF8)),
                     symbol=self.symbol_cache.get(unpacked[SYMBOL].decode(UTF8)),
                     order_side=order_side_from_string(self.convert_camel_to_snake(unpacked[ORDER_SIDE].decode(UTF8))),
                     order_type=order_type_from_string(self.convert_camel_to_snake(unpacked[ORDER_TYPE].decode(UTF8))),
                     quantity=Quantity.from_string(unpacked[QUANTITY].decode(UTF8)),
                     price=convert_string_to_price(unpacked[PRICE].decode(UTF8)),
                     label=convert_string_to_label(unpacked[LABEL].decode(UTF8)),
                     order_purpose=order_purpose_from_string(self.convert_camel_to_snake(unpacked[ORDER_PURPOSE].decode(UTF8))),
                     time_in_force=time_in_force_from_string(unpacked[TIME_IN_FORCE].decode(UTF8)),
                     expire_time=convert_string_to_datetime(unpacked[EXPIRE_TIME].decode(UTF8)),
                     init_id=GUID(UUID(unpacked[INIT_ID].decode(UTF8))),
                     timestamp=convert_string_to_datetime(unpacked[TIMESTAMP].decode(UTF8)))


cdef class MsgPackCommandSerializer(CommandSerializer):
    """
    Provides a command serializer for the MessagePack specification.
    """

    def __init__(self):
        """
        Initializes a new instance of the MsgPackCommandSerializer class.
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
        Condition.not_none(command, 'command')

        cdef dict package = {
            TYPE: command.__class__.__name__,
            ID: command.id.value,
            TIMESTAMP: convert_datetime_to_string(command.timestamp)
        }

        if isinstance(command, AccountInquiry):
            package[TRADER_ID] = command.trader_id.value
            package[ACCOUNT_ID] = command.account_id.value
        elif isinstance(command, SubmitOrder):
            package[TRADER_ID] = command.trader_id.value
            package[ACCOUNT_ID] = command.account_id.value
            package[STRATEGY_ID] = command.strategy_id.value
            package[POSITION_ID] = command.position_id.value
            package[ORDER] = self.order_serializer.serialize(command.order)
        elif isinstance(command, SubmitAtomicOrder):
            package[TRADER_ID] = command.trader_id.value
            package[ACCOUNT_ID] = command.account_id.value
            package[STRATEGY_ID] = command.strategy_id.value
            package[POSITION_ID] = command.position_id.value
            package[ENTRY] = self.order_serializer.serialize(command.atomic_order.entry)
            package[STOP_LOSS] = self.order_serializer.serialize(command.atomic_order.stop_loss)
            package[TAKE_PROFIT] = self.order_serializer.serialize(command.atomic_order.take_profit)
        elif isinstance(command, ModifyOrder):
            package[TRADER_ID] = command.trader_id.value
            package[ACCOUNT_ID] = command.account_id.value
            package[ORDER_ID] = command.order_id.value
            package[MODIFIED_QUANTITY] = command.modified_quantity.to_string()
            package[MODIFIED_PRICE] = command.modified_price.to_string()
        elif isinstance(command, CancelOrder):
            package[TRADER_ID] = command.trader_id.value
            package[ACCOUNT_ID] = command.account_id.value
            package[ORDER_ID] = command.order_id.value
            package[CANCEL_REASON] = command.cancel_reason.value
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
        Condition.not_empty(command_bytes, 'command_bytes')

        cdef dict unpacked = MsgPackSerializer.deserialize(command_bytes)  # type: {str, bytes}

        cdef str command_type = unpacked[TYPE].decode(UTF8)
        cdef GUID command_id = GUID(UUID(unpacked[ID].decode(UTF8)))
        cdef datetime command_timestamp = convert_string_to_datetime(unpacked[TIMESTAMP].decode(UTF8))

        if command_type == AccountInquiry.__name__:
            return AccountInquiry(
                self.identifier_cache.get_trader_id(unpacked[TRADER_ID].decode(UTF8)),
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                command_id,
                command_timestamp)
        elif command_type == SubmitOrder.__name__:
            return SubmitOrder(
                self.identifier_cache.get_trader_id(unpacked[TRADER_ID].decode(UTF8)),
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                self.identifier_cache.get_strategy_id(unpacked[STRATEGY_ID].decode(UTF8)),
                PositionId(unpacked[POSITION_ID].decode(UTF8)),
                self.order_serializer.deserialize(unpacked[ORDER]),
                command_id,
                command_timestamp)
        elif command_type == SubmitAtomicOrder.__name__:
            return SubmitAtomicOrder(
                self.identifier_cache.get_trader_id(unpacked[TRADER_ID].decode(UTF8)),
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                self.identifier_cache.get_strategy_id(unpacked[STRATEGY_ID].decode(UTF8)),
                PositionId(unpacked[POSITION_ID].decode(UTF8)),
                AtomicOrder(self.order_serializer.deserialize(unpacked[ENTRY]),
                            self.order_serializer.deserialize(unpacked[STOP_LOSS]),
                            self.order_serializer.deserialize(unpacked[TAKE_PROFIT])),
                command_id,
                command_timestamp)
        elif command_type == ModifyOrder.__name__:
            return ModifyOrder(
                self.identifier_cache.get_trader_id(unpacked[TRADER_ID].decode(UTF8)),
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                Quantity.from_string(unpacked[MODIFIED_QUANTITY].decode(UTF8)),
                convert_string_to_price(unpacked[MODIFIED_PRICE].decode(UTF8)),
                command_id,
                command_timestamp)
        elif command_type == CancelOrder.__name__:
            return CancelOrder(
                self.identifier_cache.get_trader_id(unpacked[TRADER_ID].decode(UTF8)),
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                ValidString(unpacked[CANCEL_REASON].decode(UTF8)),
                command_id,
                command_timestamp)
        else:
            raise RuntimeError("Cannot deserialize command (unrecognized bytes pattern).")


cdef class MsgPackEventSerializer(EventSerializer):
    """
    Provides an event serializer for the MessagePack specification
    """

    def __init__(self):
        """
        Initializes a new instance of the MsgPackCommandSerializer class.
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
        Condition.not_none(event, 'event')

        cdef dict package = {
            TYPE: event.__class__.__name__,
            ID: event.id.value,
            TIMESTAMP: convert_datetime_to_string(event.timestamp)
        }

        if isinstance(event, AccountStateEvent):
            package[ACCOUNT_ID] = event.account_id.value
            package[CURRENCY] = currency_to_string(event.currency)
            package[CASH_BALANCE] = event.cash_balance.to_string()
            package[CASH_START_DAY] = event.cash_start_day.to_string()
            package[CASH_ACTIVITY_DAY] = event.cash_activity_day.to_string()
            package[MARGIN_USED_LIQUIDATION] = event.margin_used_liquidation.to_string()
            package[MARGIN_USED_MAINTENANCE] = event.margin_used_maintenance.to_string()
            package[MARGIN_RATIO] = event.margin_ratio.to_string()
            package[MARGIN_CALL_STATUS] = event.margin_call_status.value
        elif isinstance(event, OrderInitialized):
            package[ORDER_ID] = event.order_id.value
            package[SYMBOL] = event.symbol.value
            package[ORDER_SIDE] = self.convert_snake_to_camel(order_side_to_string(event.order_side))
            package[ORDER_TYPE] = self.convert_snake_to_camel(order_type_to_string(event.order_type))
            package[QUANTITY] = event.quantity.to_string()
            package[PRICE] = convert_price_to_string(event.price)
            package[LABEL] = convert_label_to_string(event.label)
            package[ORDER_PURPOSE] = self.convert_snake_to_camel(order_purpose_to_string(event.order_purpose))
            package[TIME_IN_FORCE] = time_in_force_to_string(event.time_in_force)
            package[EXPIRE_TIME] = convert_datetime_to_string(event.expire_time)
        elif isinstance(event, OrderSubmitted):
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[SUBMITTED_TIME] = convert_datetime_to_string(event.submitted_time)
        elif isinstance(event, OrderInvalid):
            package[ORDER_ID] = event.order_id.value
            package[INVALID_REASON] = event.invalid_reason
        elif isinstance(event, OrderDenied):
            package[ORDER_ID] = event.order_id.value
            package[DENIED_REASON] = event.denied_reason
        elif isinstance(event, OrderAccepted):
            package[ACCOUNT_ID] = event.account_id.value
            package[ORDER_ID] = event.order_id.value
            package[ORDER_ID_BROKER] = event.order_id_broker.value
            package[LABEL] = convert_label_to_string(event.label)
            package[ACCEPTED_TIME] = convert_datetime_to_string(event.accepted_time)
        elif isinstance(event, OrderRejected):
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[REJECTED_TIME] = convert_datetime_to_string(event.rejected_time)
            package[REJECTED_REASON] =  event.rejected_reason.value
        elif isinstance(event, OrderWorking):
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[ORDER_ID_BROKER] = event.order_id_broker.value
            package[SYMBOL] = event.symbol.value
            package[LABEL] = convert_label_to_string(event.label)
            package[ORDER_SIDE] = self.convert_snake_to_camel(order_side_to_string(event.order_side))
            package[ORDER_TYPE] = self.convert_snake_to_camel(order_type_to_string(event.order_type))
            package[QUANTITY] = event.quantity.to_string()
            package[PRICE] = event.price.to_string()
            package[TIME_IN_FORCE] = time_in_force_to_string(event.time_in_force)
            package[EXPIRE_TIME] = convert_datetime_to_string(event.expire_time)
            package[WORKING_TIME] = convert_datetime_to_string(event.working_time)
        elif isinstance(event, OrderCancelReject):
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[REJECTED_TIME] = convert_datetime_to_string(event.rejected_time)
            package[REJECTED_RESPONSE_TO] = event.rejected_response_to.value
            package[REJECTED_REASON] = event.rejected_reason.value
        elif isinstance(event, OrderCancelled):
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[CANCELLED_TIME] = convert_datetime_to_string(event.cancelled_time)
        elif isinstance(event, OrderModified):
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[ORDER_ID_BROKER] = event.order_id_broker.value
            package[MODIFIED_TIME] = convert_datetime_to_string(event.modified_time)
            package[MODIFIED_QUANTITY] = event.modified_quantity.to_string()
            package[MODIFIED_PRICE] = event.modified_price.to_string()
        elif isinstance(event, OrderExpired):
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[EXPIRED_TIME] = convert_datetime_to_string(event.expired_time)
        elif isinstance(event, OrderPartiallyFilled):
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[EXECUTION_ID] = event.execution_id.value
            package[POSITION_ID_BROKER] = event.position_id_broker.value
            package[SYMBOL] = event.symbol.value
            package[ORDER_SIDE] = self.convert_snake_to_camel(order_side_to_string(event.order_side))
            package[FILLED_QUANTITY] = event.filled_quantity.to_string()
            package[LEAVES_QUANTITY] = event.leaves_quantity.to_string()
            package[AVERAGE_PRICE] = event.average_price.to_string()
            package[CURRENCY] = currency_to_string(event.transaction_currency)
            package[EXECUTION_TIME] = convert_datetime_to_string(event.execution_time)
        elif isinstance(event, OrderFilled):
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[EXECUTION_ID] = event.execution_id.value
            package[POSITION_ID_BROKER] = event.position_id_broker.value
            package[SYMBOL] = event.symbol.value
            package[ORDER_SIDE] = self.convert_snake_to_camel(order_side_to_string(event.order_side))
            package[FILLED_QUANTITY] = event.filled_quantity.to_string()
            package[AVERAGE_PRICE] = event.average_price.to_string()
            package[CURRENCY] = currency_to_string(event.transaction_currency)
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
        Condition.not_empty(event_bytes, 'event_bytes')

        cdef dict unpacked = MsgPackSerializer.deserialize(event_bytes)  # type: {str, bytes}

        cdef str event_type = unpacked[TYPE].decode(UTF8)
        cdef GUID event_id = GUID(UUID(unpacked[ID].decode(UTF8)))
        cdef datetime event_timestamp = convert_string_to_datetime(unpacked[TIMESTAMP].decode(UTF8))

        cdef Currency currency
        if event_type == AccountStateEvent.__name__:
            currency = currency_from_string(unpacked[CURRENCY].decode(UTF8))
            return AccountStateEvent(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                currency,
                Money.from_string(unpacked[CASH_BALANCE].decode(UTF8), currency),
                Money.from_string(unpacked[CASH_START_DAY].decode(UTF8), currency),
                Money.from_string(unpacked[CASH_ACTIVITY_DAY].decode(UTF8), currency),
                Money.from_string(unpacked[MARGIN_USED_LIQUIDATION].decode(UTF8), currency),
                Money.from_string(unpacked[MARGIN_USED_MAINTENANCE].decode(UTF8), currency),
                Decimal.from_string_to_decimal(unpacked[MARGIN_RATIO].decode(UTF8)),
                ValidString(unpacked[MARGIN_CALL_STATUS].decode(UTF8)),
                event_id,
                event_timestamp)
        elif event_type == OrderInitialized.__name__:
            return OrderInitialized(
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                self.identifier_cache.get_symbol(unpacked[SYMBOL].decode(UTF8)),
                convert_string_to_label(unpacked[LABEL].decode(UTF8)),
                order_side_from_string(self.convert_camel_to_snake(unpacked[ORDER_SIDE].decode(UTF8))),
                order_type_from_string(self.convert_camel_to_snake(unpacked[ORDER_TYPE].decode(UTF8))),
                Quantity.from_string(unpacked[QUANTITY].decode(UTF8)),
                convert_string_to_price(unpacked[PRICE].decode(UTF8)),
                order_purpose_from_string(self.convert_camel_to_snake(unpacked[ORDER_PURPOSE].decode(UTF8))),
                time_in_force_from_string(unpacked[TIME_IN_FORCE].decode(UTF8)),
                convert_string_to_datetime(unpacked[EXPIRE_TIME].decode(UTF8)),
                event_id,
                event_timestamp)
        elif event_type == OrderSubmitted.__name__:
            return OrderSubmitted(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                convert_string_to_datetime(unpacked[SUBMITTED_TIME].decode(UTF8)),
                event_id,
                event_timestamp)
        elif event_type == OrderInvalid.__name__:
            return OrderInvalid(
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                unpacked[INVALID_REASON].decode(UTF8),
                event_id,
                event_timestamp)
        elif event_type == OrderDenied.__name__:
            return OrderDenied(
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                unpacked[DENIED_REASON].decode(UTF8),
                event_id,
                event_timestamp)
        elif event_type == OrderAccepted.__name__:
            return OrderAccepted(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                OrderIdBroker(unpacked[ORDER_ID_BROKER].decode(UTF8)),
                convert_string_to_label(unpacked[LABEL].decode(UTF8)),
                convert_string_to_datetime(unpacked[ACCEPTED_TIME].decode(UTF8)),
                event_id,
                event_timestamp)
        elif event_type == OrderRejected.__name__:
            return OrderRejected(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                convert_string_to_datetime(unpacked[REJECTED_TIME].decode(UTF8)),
                ValidString(unpacked[REJECTED_REASON].decode(UTF8)),
                event_id,
                event_timestamp)
        elif event_type == OrderWorking.__name__:
            return OrderWorking(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                OrderIdBroker(unpacked[ORDER_ID_BROKER].decode(UTF8)),
                Symbol.from_string(unpacked[SYMBOL].decode(UTF8)),
                convert_string_to_label(unpacked[LABEL].decode(UTF8)),
                order_side_from_string(self.convert_camel_to_snake(unpacked[ORDER_SIDE].decode(UTF8))),
                order_type_from_string(self.convert_camel_to_snake(unpacked[ORDER_TYPE].decode(UTF8))),
                Quantity.from_string(unpacked[QUANTITY].decode(UTF8)),
                convert_string_to_price(unpacked[PRICE].decode(UTF8)),
                time_in_force_from_string(unpacked[TIME_IN_FORCE].decode(UTF8)),
                convert_string_to_datetime(unpacked[WORKING_TIME].decode(UTF8)),
                event_id,
                event_timestamp,
                convert_string_to_datetime(unpacked[EXPIRE_TIME].decode(UTF8)))
        elif event_type == OrderCancelled.__name__:
            return OrderCancelled(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                convert_string_to_datetime(unpacked[CANCELLED_TIME].decode(UTF8)),
                event_id,
                event_timestamp)
        elif event_type == OrderCancelReject.__name__:
            return OrderCancelReject(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                convert_string_to_datetime(unpacked[REJECTED_TIME].decode(UTF8)),
                ValidString(unpacked[REJECTED_RESPONSE_TO].decode(UTF8)),
                ValidString(unpacked[REJECTED_REASON].decode(UTF8)),
                event_id,
                event_timestamp)
        elif event_type == OrderModified.__name__:
            return OrderModified(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                OrderIdBroker(unpacked[ORDER_ID_BROKER].decode(UTF8)),
                Quantity.from_string(unpacked[MODIFIED_QUANTITY].decode(UTF8)),
                convert_string_to_price(unpacked[MODIFIED_PRICE].decode(UTF8)),
                convert_string_to_datetime(unpacked[MODIFIED_TIME].decode(UTF8)),
                event_id,
                event_timestamp)
        elif event_type == OrderExpired.__name__:
            return OrderExpired(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                convert_string_to_datetime(unpacked[EXPIRED_TIME].decode(UTF8)),
                event_id,
                event_timestamp)
        elif event_type == OrderPartiallyFilled.__name__:
            return OrderPartiallyFilled(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                ExecutionId(unpacked[EXECUTION_ID].decode(UTF8)),
                PositionIdBroker(unpacked[POSITION_ID_BROKER].decode(UTF8)),
                self.identifier_cache.get_symbol(unpacked[SYMBOL].decode(UTF8)),
                order_side_from_string(self.convert_camel_to_snake(unpacked[ORDER_SIDE].decode(UTF8))),
                Quantity.from_string(unpacked[FILLED_QUANTITY].decode(UTF8)),
                Quantity.from_string(unpacked[LEAVES_QUANTITY].decode(UTF8)),
                convert_string_to_price(unpacked[AVERAGE_PRICE].decode(UTF8)),
                currency_from_string(unpacked[CURRENCY].decode(UTF8)),
                convert_string_to_datetime(unpacked[EXECUTION_TIME].decode(UTF8)),
                event_id,
                event_timestamp)
        elif event_type == OrderFilled.__name__:
            return OrderFilled(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID].decode(UTF8)),
                OrderId(unpacked[ORDER_ID].decode(UTF8)),
                ExecutionId(unpacked[EXECUTION_ID].decode(UTF8)),
                PositionIdBroker(unpacked[POSITION_ID_BROKER].decode(UTF8)),
                self.identifier_cache.get_symbol(unpacked[SYMBOL].decode(UTF8)),
                order_side_from_string(self.convert_camel_to_snake(unpacked[ORDER_SIDE].decode(UTF8))),
                Quantity.from_string(unpacked[FILLED_QUANTITY].decode(UTF8)),
                convert_string_to_price(unpacked[AVERAGE_PRICE].decode(UTF8)),
                currency_from_string(unpacked[CURRENCY].decode(UTF8)),
                convert_string_to_datetime(unpacked[EXECUTION_TIME].decode(UTF8)),
                event_id,
                event_timestamp)
        else:
            raise RuntimeError("Cannot deserialize event (unrecognized event).")


cdef class MsgPackRequestSerializer(RequestSerializer):
    """
    Provides a request serializer for the MessagePack specification.
    """

    def __init__(self):
        """
        Initializes a new instance of the MsgPackRequestSerializer class.
        """
        super().__init__()

        self.dict_serializer = MsgPackDictionarySerializer()

    cpdef bytes serialize(self, Request request):
        """
        Serialize the given request to bytes.

        :param request: The request to serialize.
        :return bytes.
        :raises RuntimeError: If the request cannot be serialized.
        """
        Condition.not_none(request, 'request')

        cdef dict package = {
            TYPE: request.__class__.__name__,
            ID: request.id.value,
            TIMESTAMP: convert_datetime_to_string(request.timestamp)
        }

        if isinstance(request, Connect):
            package[CLIENT_ID] = request.client_id.value
            package[AUTHENTICATION] = request.authentication
        elif isinstance(request, Disconnect):
            package[CLIENT_ID] = request.client_id.value
            package[SESSION_ID] = request.session_id.value
        elif isinstance(request, DataRequest):
            package[QUERY] = self.dict_serializer.serialize(request.query)
        else:
            raise RuntimeError("Cannot serialize request (unrecognized request.")

        return MsgPackSerializer.serialize(package)

    cpdef Request deserialize(self, bytes request_bytes):
        """
        Deserialize the given bytes to a request.

        :param request_bytes: The bytes to deserialize.
        :return Request.
        :raises RuntimeError: If the request cannot be deserialized.
        """
        Condition.not_empty(request_bytes, 'request_bytes')

        cdef dict unpacked = MsgPackSerializer.deserialize(request_bytes)  # type: {str, bytes}

        cdef str request_type = unpacked[TYPE].decode(UTF8)
        cdef GUID request_id = GUID(UUID(unpacked[ID].decode(UTF8)))
        cdef datetime request_timestamp = convert_string_to_datetime(unpacked[TIMESTAMP].decode(UTF8))

        if request_type == Connect.__name__:
            return Connect(
                ClientId(unpacked[CLIENT_ID].decode(UTF8)),
                unpacked[AUTHENTICATION].decode(UTF8),
                request_id,
                request_timestamp)
        elif request_type == Disconnect.__name__:
            return Disconnect(
                ClientId(unpacked[CLIENT_ID].decode(UTF8)),
                SessionId(unpacked[SESSION_ID].decode(UTF8)),
                request_id,
                request_timestamp)
        elif request_type == DataRequest.__name__:
            return DataRequest(
                self.dict_serializer.deserialize(unpacked[QUERY]),
                request_id,
                request_timestamp)
        else:
            raise RuntimeError("Cannot deserialize request (unrecognized request).")


cdef class MsgPackResponseSerializer(ResponseSerializer):
    """
    Provides a response serializer for the MessagePack specification.
    """

    def __init__(self):
        """
        Initializes a new instance of the MsgPackResponseSerializer class.
        """
        super().__init__()

    cpdef bytes serialize(self, Response response):
        """
        Serialize the given response to bytes.

        :param response: The response to serialize.
        :return bytes.
        :raises RuntimeError: If the response cannot be serialized.
        """
        Condition.not_none(response, 'response')

        cdef dict package = {
            TYPE: response.__class__.__name__,
            ID: response.id.value,
            CORRELATION_ID: response.correlation_id.value,
            TIMESTAMP: convert_datetime_to_string(response.timestamp)
        }

        if isinstance(response, Connected):
            package[MESSAGE] = response.message
            package[SERVER_ID] = response.server_id.value
            package[SESSION_ID] = response.session_id.value
        elif isinstance(response, Disconnected):
            package[MESSAGE] = response.message
            package[SERVER_ID] = response.server_id.value
            package[SESSION_ID] = response.session_id.value
        elif isinstance(response, MessageReceived):
            package[RECEIVED_TYPE] = response.received_type
        elif isinstance(response, MessageRejected):
            package[MESSAGE] = response.message
        elif isinstance(response, DataResponse):
            package[DATA] = response.data
            package[DATA_TYPE] = response.data_type
            package[DATA_ENCODING] = response.data_encoding
        else:
            raise RuntimeError("Cannot serialize response (unrecognized response.")

        return MsgPackSerializer.serialize(package)

    cpdef Response deserialize(self, bytes response_bytes):
        """
        Deserialize the given bytes to a response.

        :param response_bytes: The bytes to deserialize.
        :return Response.
        :raises RuntimeError: If the response cannot be deserialized.
        """
        Condition.not_empty(response_bytes, 'response_bytes')

        cdef dict unpacked = MsgPackSerializer.deserialize(response_bytes)  # type: {str, bytes}

        cdef str response_type = unpacked[TYPE].decode(UTF8)
        cdef GUID correlation_id = GUID(UUID(unpacked[CORRELATION_ID].decode(UTF8)))
        cdef GUID response_id = GUID(UUID(unpacked[ID].decode(UTF8)))
        cdef datetime response_timestamp = convert_string_to_datetime(unpacked[TIMESTAMP].decode(UTF8))

        if response_type == Connected.__name__:
            return Connected(
                unpacked[MESSAGE].decode(UTF8),
                ServerId(unpacked[SERVER_ID].decode(UTF8)),
                SessionId(unpacked[SESSION_ID].decode(UTF8)),
                correlation_id,
                response_id,
                response_timestamp)
        elif response_type == Disconnected.__name__:
            return Disconnected(
                unpacked[MESSAGE].decode(UTF8),
                ServerId(unpacked[SERVER_ID].decode(UTF8)),
                SessionId(unpacked[SESSION_ID].decode(UTF8)),
                correlation_id,
                response_id,
                response_timestamp)
        elif response_type == MessageReceived.__name__:
            return MessageReceived(
                unpacked[RECEIVED_TYPE].decode(UTF8),
                correlation_id,
                response_id,
                response_timestamp)
        elif response_type == MessageRejected.__name__:
            return MessageRejected(
                unpacked[MESSAGE].decode(UTF8),
                correlation_id,
                response_id,
                response_timestamp)
        elif response_type == QueryFailure.__name__:
            return QueryFailure(
                unpacked[MESSAGE].decode(UTF8),
                correlation_id,
                response_id,
                response_timestamp)
        elif response_type == DataResponse.__name__:
            return DataResponse(
                unpacked[DATA],
                unpacked[DATA_TYPE].decode(UTF8),
                unpacked[DATA_ENCODING].decode(UTF8),
                correlation_id,
                response_id,
                response_timestamp)
        else:
            raise RuntimeError("Cannot deserialize response (unrecognized response).")


cdef class MsgPackLogSerializer(LogSerializer):
    """
    Provides a log message serializer for the MessagePack specification.
    """

    def __init__(self):
        """
        Initializes a new instance of the MsgPackLogSerializer class.
        """
        super().__init__()

    cpdef bytes serialize(self, LogMessage message):
        """
        Serialize the given log message to bytes.

        :param message: The message to serialize.
        :return bytes.
        """
        Condition.not_none(message, 'message')

        cdef dict package = {
            TIMESTAMP: convert_datetime_to_string(message.timestamp),
            LOG_LEVEL: message.level_string(),
            LOG_TEXT: message.text,
            THREAD_ID: str(message.thread_id),
        }

        return MsgPackSerializer.serialize(package)

    cpdef LogMessage deserialize(self, bytes message_bytes):
        """
        Deserialize the given bytes to a response.

        :param message_bytes: The bytes to deserialize.
        :return LogMessage.
        """
        Condition.not_empty(message_bytes, 'message_bytes')

        cdef dict unpacked = MsgPackSerializer.deserialize(message_bytes)

        return LogMessage(
            timestamp=convert_string_to_datetime(unpacked[TIMESTAMP].decode(UTF8)),
            level=log_level_from_string(unpacked[LOG_LEVEL].decode(UTF8)),
            text=unpacked[LOG_TEXT].decode(UTF8),
            thread_id=int(unpacked[THREAD_ID].decode(UTF8)))
