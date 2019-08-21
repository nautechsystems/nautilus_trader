# -------------------------------------------------------------------------------------------------
# <copyright file="serializers.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import msgpack

from cpython.datetime cimport datetime
from decimal import Decimal
from uuid import UUID

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport ValidString, GUID
from nautilus_trader.core.message cimport Command, Event, Request, Response
from nautilus_trader.model.c_enums.time_in_force cimport time_in_force_to_string, time_in_force_from_string
from nautilus_trader.model.c_enums.order_side cimport  order_side_to_string, order_side_from_string
from nautilus_trader.model.c_enums.order_type cimport order_type_to_string, order_type_from_string
from nautilus_trader.model.c_enums.currency cimport currency_to_string, currency_from_string
from nautilus_trader.model.identifiers cimport (
    Symbol,
    TraderId,
    StrategyId,
    OrderId,
    PositionId,
    AccountId,
    ExecutionId,
    ExecutionTicket,
    Label)
from nautilus_trader.model.objects cimport Quantity, Money, Price
from nautilus_trader.model.order cimport Order, AtomicOrder
from nautilus_trader.serialization.constants cimport *
from nautilus_trader.serialization.base cimport (
    OrderSerializer,
    CommandSerializer,
    EventSerializer,
    RequestSerializer,
    ResponseSerializer
)
from nautilus_trader.serialization.common cimport (
    convert_price_to_string,
    convert_label_to_string,
    convert_datetime_to_string,
    convert_string_to_datetime,
    convert_string_to_price,
    convert_string_to_label
)
from nautilus_trader.model.commands cimport (
    AccountInquiry,
    SubmitOrder,
    SubmitAtomicOrder,
    ModifyOrder,
    CancelOrder
)
from nautilus_trader.model.events cimport (
    AccountEvent,
    OrderInitialized,
    OrderSubmitted,
    OrderAccepted,
    OrderRejected,
    OrderWorking,
    OrderExpired,
    OrderModified,
    OrderCancelled,
    OrderCancelReject,
    OrderPartiallyFilled,
    OrderFilled
)
from nautilus_trader.network.requests cimport DataRequest
from nautilus_trader.network.responses cimport (
    MessageReceived,
    MessageRejected,
    QueryFailure,
    DataResponse
)


cdef class MsgPackQuerySerializer(QuerySerializer):
    """
    Provides a serializer for data query objects for the MsgPack specification.
    """

    cpdef bytes serialize(self, dict query):
        """
        Serialize the given data query to bytes.

        :param: data: The data query to serialize.
        :return: bytes.
        """
        return msgpack.packb(query)

    cpdef dict deserialize(self, bytes query_bytes):
        """
        Deserialize the given bytes to a data query.

        :param: data_bytes: The data query bytes to deserialize.
        :return: Dict.
        """
        return msgpack.unpackb(query_bytes, raw=False)


cdef class MsgPackOrderSerializer(OrderSerializer):
    """
    Provides a command serializer for the MessagePack specification.
    """

    cpdef bytes serialize(self, Order order):
        """
        Return the serialized MessagePack specification bytes from the given order.

        :param order: The order to serialize.
        :return: bytes.
        """
        if order is None:
            return msgpack.packb({})  # Null order

        return msgpack.packb({
            ID: order.id.value,
            SYMBOL: order.symbol.value,
            ORDER_SIDE: order_side_to_string(order.side),
            ORDER_TYPE: order_type_to_string(order.type),
            QUANTITY: order.quantity.value,
            PRICE: convert_price_to_string(order.price),
            LABEL: convert_label_to_string(order.label),
            TIME_IN_FORCE: time_in_force_to_string(order.time_in_force),
            EXPIRE_TIME: convert_datetime_to_string(order.expire_time),
            TIMESTAMP: convert_datetime_to_string(order.timestamp),
            INIT_ID: order.init_id.value})

    cpdef Order deserialize(self, bytes order_bytes):
        """
        Return the order deserialized from the given MessagePack specification bytes.

        :param order_bytes: The bytes to deserialize.
        :return: Order.
        :raises ConditionFailed: If the event_bytes is empty.
        """
        Condition.not_empty(order_bytes, 'order_bytes')

        cdef dict unpacked = msgpack.unpackb(order_bytes, raw=False)

        if len(unpacked) == 0:
            return None  # Null order

        return Order(order_id=OrderId(unpacked[ID]),
                     symbol=Symbol.from_string(unpacked[SYMBOL]),
                     order_side=order_side_from_string(unpacked[ORDER_SIDE]),
                     order_type=order_type_from_string(unpacked[ORDER_TYPE]),
                     quantity=Quantity(unpacked[QUANTITY]),
                     timestamp=convert_string_to_datetime(unpacked[TIMESTAMP]),
                     price=convert_string_to_price(unpacked[PRICE]),
                     label=convert_string_to_label(unpacked[LABEL]),
                     time_in_force=time_in_force_from_string(unpacked[TIME_IN_FORCE]),
                     expire_time=convert_string_to_datetime(unpacked[EXPIRE_TIME]))


cdef class MsgPackCommandSerializer(CommandSerializer):
    """
    Provides a command serializer for the MessagePack specification.
    """

    def __init__(self):
        """
        Initializes a new instance of the MsgPackCommandSerializer class.
        """
        self.order_serializer = MsgPackOrderSerializer()

    cpdef bytes serialize(self, Command command):
        """
        Return the serialized MessagePack specification bytes from the given command.

        :param: command: The command to serialize.
        :return: bytes.
        :raises: RuntimeError: If the command cannot be serialized.
        """
        cdef dict package = {
            TYPE: command.__class__.__name__,
            ID: command.id.value,
            TIMESTAMP: convert_datetime_to_string(command.timestamp)
        }

        if isinstance(command, AccountInquiry):
            package[ACCOUNT_ID] = command.account_id.value
        elif isinstance(command, SubmitOrder):
            package[TRADER_ID] = command.trader_id.value
            package[STRATEGY_ID] = command.strategy_id.value
            package[POSITION_ID] = command.position_id.value
            package[ACCOUNT_ID] = command.account_id.value
            package[ORDER] = self.order_serializer.serialize(command.order)
        elif isinstance(command, SubmitAtomicOrder):
            package[TRADER_ID] = command.trader_id.value
            package[STRATEGY_ID] = command.strategy_id.value
            package[POSITION_ID] = command.position_id.value
            package[ACCOUNT_ID] = command.account_id.value
            package[ENTRY] = self.order_serializer.serialize(command.atomic_order.entry)
            package[STOP_LOSS] = self.order_serializer.serialize(command.atomic_order.stop_loss)
            package[TAKE_PROFIT] = self.order_serializer.serialize(command.atomic_order.take_profit)
        elif isinstance(command, ModifyOrder):
            package[TRADER_ID] = command.trader_id.value
            package[STRATEGY_ID] = command.strategy_id.value
            package[ACCOUNT_ID] = command.account_id.value
            package[ORDER_ID] = command.order_id.value
            package[MODIFIED_PRICE] = str(command.modified_price)
        elif isinstance(command, CancelOrder):
            package[TRADER_ID] = command.trader_id.value
            package[STRATEGY_ID] = command.strategy_id.value
            package[ACCOUNT_ID] = command.account_id.value
            package[ORDER_ID] = command.order_id.value
            package[CANCEL_REASON] = command.cancel_reason.value
        else:
            raise RuntimeError("Cannot serialize command (unrecognized command).")

        return msgpack.packb(package)

    cpdef Command deserialize(self, bytes command_bytes):
        """
        Return the command deserialize from the given MessagePack specification command_bytes.

        :param command_bytes: The command to deserialize.
        :return: Command.
        :raises ConditionFailed: If the command_bytes is empty.
        :raises RuntimeError: If the command cannot be deserialized.
        """
        Condition.not_empty(command_bytes, 'command_bytes')

        cdef dict unpacked_raw = msgpack.unpackb(command_bytes)
        cdef dict unpacked = {}

        # Manually unpack and decode
        for k, v in unpacked_raw.items():
            if k not in (b'Order', b'Entry', b'StopLoss', b'TakeProfit'):
                if isinstance(v, bytes):
                    unpacked[k.decode(UTF8)] = v.decode(UTF8)
                else:
                    unpacked[k.decode(UTF8)] = v
            else:
                unpacked[k.decode(UTF8)] = v

        cdef str command_type = unpacked[TYPE]
        cdef GUID command_id = GUID(UUID(unpacked[ID]))
        cdef datetime command_timestamp = convert_string_to_datetime(unpacked[TIMESTAMP])

        if command_type == AccountInquiry.__name__:
            return AccountInquiry(
                AccountId.from_string(unpacked[ACCOUNT_ID]),
                command_id,
                command_timestamp)
        if command_type == SubmitOrder.__name__:
            return SubmitOrder(
                TraderId.from_string(unpacked[TRADER_ID]),
                StrategyId.from_string(unpacked[STRATEGY_ID]),
                PositionId(unpacked[POSITION_ID]),
                AccountId.from_string(unpacked[ACCOUNT_ID]),
                self.order_serializer.deserialize(unpacked[ORDER]),
                command_id,
                command_timestamp)
        if command_type == SubmitAtomicOrder.__name__:
            return SubmitAtomicOrder(
                TraderId.from_string(unpacked[TRADER_ID]),
                StrategyId.from_string(unpacked[STRATEGY_ID]),
                PositionId(unpacked[POSITION_ID]),
                AccountId.from_string(unpacked[ACCOUNT_ID]),
                AtomicOrder(self.order_serializer.deserialize(unpacked[ENTRY]),
                            self.order_serializer.deserialize(unpacked[STOP_LOSS]),
                            self.order_serializer.deserialize(unpacked[TAKE_PROFIT])),
                command_id,
                command_timestamp)
        if command_type == ModifyOrder.__name__:
            return ModifyOrder(
                TraderId.from_string(unpacked[TRADER_ID]),
                StrategyId.from_string(unpacked[STRATEGY_ID]),
                AccountId.from_string(unpacked[ACCOUNT_ID]),
                OrderId(unpacked[ORDER_ID]),
                Price(unpacked[MODIFIED_PRICE]),
                command_id,
                command_timestamp)
        if command_type == CancelOrder.__name__:
            return CancelOrder(
                TraderId.from_string(unpacked[TRADER_ID]),
                StrategyId.from_string(unpacked[STRATEGY_ID]),
                AccountId.from_string(unpacked[ACCOUNT_ID]),
                OrderId(unpacked[ORDER_ID]),
                ValidString(unpacked[CANCEL_REASON]),
                command_id,
                command_timestamp)
        else:
            raise RuntimeError("Cannot deserialize command (unrecognized bytes pattern).")


cdef class MsgPackEventSerializer(EventSerializer):
    """
    Provides an event serializer for the MessagePack specification
    """

    cpdef bytes serialize(self, Event event):
        """
        Return the MessagePack specification bytes serialized from the given event.

        :param event: The event to serialize.
        :return: bytes.
        :raises: RuntimeError: If the event cannot be serialized.
        """
        cdef dict package = {
            TYPE: event.__class__.__name__,
            ID: event.id.value,
            TIMESTAMP: convert_datetime_to_string(event.timestamp)
        }

        if isinstance(event, AccountEvent):
            package[ACCOUNT_ID] = event.account_id.value
            package[CURRENCY] = currency_to_string(event.currency)
            package[CASH_BALANCE] = str(event.cash_balance)
            package[CASH_START_DAY] = str(event.cash_start_day)
            package[CASH_ACTIVITY_DAY] = str(event.cash_activity_day)
            package[MARGIN_USED_LIQUIDATION] = str(event.margin_used_liquidation)
            package[MARGIN_USED_MAINTENANCE] = str(event.margin_used_maintenance)
            package[MARGIN_RATIO] = str(event.margin_ratio)
            package[MARGIN_CALL_STATUS] = event.margin_call_status.value
        elif isinstance(event, OrderInitialized):
            package[ORDER_ID] = event.order_id.value
            package[SYMBOL] = event.symbol.value
            package[LABEL] = event.label.value
            package[ORDER_SIDE] = order_side_to_string(event.order_side)
            package[ORDER_TYPE] = order_type_to_string(event.order_type)
            package[QUANTITY] = event.quantity.value
            package[PRICE] = str(event.price)
            package[TIME_IN_FORCE] = time_in_force_to_string(event.time_in_force)
            package[EXPIRE_TIME] = convert_datetime_to_string(event.expire_time)
        elif isinstance(event, OrderSubmitted):
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[SUBMITTED_TIME] = convert_datetime_to_string(event.submitted_time)
        elif isinstance(event, OrderAccepted):
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[ACCEPTED_TIME] = convert_datetime_to_string(event.accepted_time)
        elif isinstance(event, OrderRejected):
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[REJECTED_TIME] = convert_datetime_to_string(event.rejected_time)
            package[REJECTED_REASON] =  str(event.rejected_reason)
        elif isinstance(event, OrderWorking):
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[ORDER_ID_BROKER] = event.order_id_broker.value
            package[SYMBOL] = event.symbol.value
            package[LABEL] = event.label.value
            package[ORDER_SIDE] = order_side_to_string(event.order_side)
            package[ORDER_TYPE] = order_type_to_string(event.order_type)
            package[QUANTITY] = event.quantity.value
            package[PRICE] = str(event.price)
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
            package[MODIFIED_PRICE] = str(event.modified_price)
        elif isinstance(event, OrderExpired):
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[EXPIRED_TIME] = convert_datetime_to_string(event.expired_time)
        elif isinstance(event, OrderPartiallyFilled):
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[EXECUTION_ID] = event.execution_id.value
            package[EXECUTION_TICKET] = event.execution_ticket.value
            package[SYMBOL] = event.symbol.value
            package[ORDER_SIDE] = order_side_to_string(event.order_side)
            package[FILLED_QUANTITY] = event.filled_quantity.value
            package[LEAVES_QUANTITY] = event.leaves_quantity.value
            package[AVERAGE_PRICE] = str(event.average_price)
            package[EXECUTION_TIME] = convert_datetime_to_string(event.execution_time)
        elif isinstance(event, OrderFilled):
            package[ORDER_ID] = event.order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[EXECUTION_ID] = event.execution_id.value
            package[EXECUTION_TICKET] = event.execution_ticket.value
            package[SYMBOL] = event.symbol.value
            package[ORDER_SIDE] = order_side_to_string(event.order_side)
            package[FILLED_QUANTITY] = event.filled_quantity.value
            package[AVERAGE_PRICE] = str(event.average_price)
            package[EXECUTION_TIME] = convert_datetime_to_string(event.execution_time)
        else:
            raise RuntimeError("Cannot serialize event (unrecognized event.")

        return msgpack.packb(package)

    cpdef Event deserialize(self, bytes event_bytes):
        """
        Return the event deserialized from the given MessagePack specification event_bytes.

        :param event_bytes: The bytes to deserialize.
        :return: Event.
        :raises ConditionFailed: If the event_bytes is empty.
        :raises RuntimeError: If the event cannot be deserialized.
        """
        Condition.not_empty(event_bytes, 'event_bytes')

        cdef dict unpacked = msgpack.unpackb(event_bytes, raw=False)

        cdef str event_type = unpacked[TYPE]
        cdef GUID event_id = GUID(UUID(unpacked[ID]))
        cdef datetime event_timestamp = convert_string_to_datetime(unpacked[TIMESTAMP])

        if event_type == AccountEvent.__name__:
            return AccountEvent(
                AccountId.from_string(unpacked[ACCOUNT_ID]),
                currency_from_string(unpacked[CURRENCY]),
                Money(unpacked[CASH_BALANCE]),
                Money(unpacked[CASH_START_DAY]),
                Money(unpacked[CASH_ACTIVITY_DAY]),
                Money(unpacked[MARGIN_USED_LIQUIDATION]),
                Money(unpacked[MARGIN_USED_MAINTENANCE]),
                Decimal(unpacked[MARGIN_RATIO]),
                ValidString('NONE'),
                event_id,
                event_timestamp)

        if event_type == OrderSubmitted.__name__:
            return OrderSubmitted(
                OrderId(unpacked[ORDER_ID]),
                AccountId.from_string(unpacked[ACCOUNT_ID]),
                convert_string_to_datetime(unpacked[SUBMITTED_TIME]),
                event_id,
                event_timestamp)
        if event_type == OrderAccepted.__name__:
            return OrderAccepted(
                OrderId(unpacked[ORDER_ID]),
                AccountId.from_string(unpacked[ACCOUNT_ID]),
                convert_string_to_datetime(unpacked[ACCEPTED_TIME]),
                event_id,
                event_timestamp)
        if event_type == OrderRejected.__name__:
            return OrderRejected(
                OrderId(unpacked[ORDER_ID]),
                AccountId.from_string(unpacked[ACCOUNT_ID]),
                convert_string_to_datetime(unpacked[REJECTED_TIME]),
                ValidString(unpacked[REJECTED_REASON]),
                event_id,
                event_timestamp)
        if event_type == OrderWorking.__name__:
            return OrderWorking(
                OrderId(unpacked[ORDER_ID]),
                OrderId(unpacked[ORDER_ID_BROKER]),
                AccountId.from_string(unpacked[ACCOUNT_ID]),
                Symbol.from_string(unpacked[SYMBOL]),
                Label(unpacked[LABEL]),
                order_side_from_string(unpacked[ORDER_SIDE]),
                order_type_from_string(unpacked[ORDER_TYPE]),
                Quantity(unpacked[QUANTITY]),
                Price(unpacked[PRICE]),
                time_in_force_from_string(unpacked[TIME_IN_FORCE]),
                convert_string_to_datetime(unpacked[WORKING_TIME]),
                event_id,
                event_timestamp,
                convert_string_to_datetime(unpacked[EXPIRE_TIME]))
        if event_type == OrderCancelled.__name__:
            return OrderCancelled(
                OrderId(unpacked[ORDER_ID]),
                AccountId.from_string(unpacked[ACCOUNT_ID]),
                convert_string_to_datetime(unpacked[CANCELLED_TIME]),
                event_id,
                event_timestamp)
        if event_type == OrderCancelReject.__name__:
            return OrderCancelReject(
                OrderId(unpacked[ORDER_ID]),
                AccountId.from_string(unpacked[ACCOUNT_ID]),
                convert_string_to_datetime(unpacked[REJECTED_TIME]),
                ValidString(unpacked[REJECTED_RESPONSE_TO]),
                ValidString(unpacked[REJECTED_REASON]),
                event_id,
                event_timestamp)
        if event_type == OrderModified.__name__:
            return OrderModified(
                OrderId(unpacked[ORDER_ID]),
                OrderId(unpacked[ORDER_ID_BROKER]),
                AccountId.from_string(unpacked[ACCOUNT_ID]),
                Price(unpacked[MODIFIED_PRICE]),
                convert_string_to_datetime(unpacked[MODIFIED_TIME]),
                event_id,
                event_timestamp)
        if event_type == OrderExpired.__name__:
            return OrderExpired(
                OrderId(unpacked[ORDER_ID]),
                AccountId.from_string(unpacked[ACCOUNT_ID]),
                convert_string_to_datetime(unpacked[EXPIRED_TIME]),
                event_id,
                event_timestamp)
        if event_type == OrderPartiallyFilled.__name__:
            return OrderPartiallyFilled(
                OrderId(unpacked[ORDER_ID]),
                AccountId.from_string(unpacked[ACCOUNT_ID]),
                ExecutionId(unpacked[EXECUTION_ID]),
                ExecutionTicket(unpacked[EXECUTION_TICKET]),
                Symbol.from_string(unpacked[SYMBOL]),
                order_side_from_string(unpacked[ORDER_SIDE]),
                Quantity(unpacked[FILLED_QUANTITY]),
                Quantity(unpacked[LEAVES_QUANTITY]),
                Price(unpacked[AVERAGE_PRICE]),
                convert_string_to_datetime(unpacked[EXECUTION_TIME]),
                event_id,
                event_timestamp)
        if event_type == OrderFilled.__name__:
            return OrderFilled(
                OrderId(unpacked[ORDER_ID]),
                AccountId.from_string(unpacked[ACCOUNT_ID]),
                ExecutionId(unpacked[EXECUTION_ID]),
                ExecutionTicket(unpacked[EXECUTION_TICKET]),
                Symbol.from_string(unpacked[SYMBOL]),
                order_side_from_string(unpacked[ORDER_SIDE]),
                Quantity(unpacked[FILLED_QUANTITY]),
                Price(unpacked[AVERAGE_PRICE]),
                convert_string_to_datetime(unpacked[EXECUTION_TIME]),
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
        self.query_serializer = MsgPackQuerySerializer()

    cpdef bytes serialize(self, Request request):
        """
        Serialize the given request to bytes.

        :param request: The request to serialize.
        :return: bytes.
        :raises RuntimeError: If the request cannot be serialized.
        """
        cdef dict package = {
            TYPE: request.__class__.__name__,
            ID: request.id.value,
            TIMESTAMP: convert_datetime_to_string(request.timestamp)
        }

        if isinstance(request, DataRequest):
            package[QUERY] = self.query_serializer.serialize(request.query)
        else:
            raise RuntimeError("Cannot serialize request (unrecognized request.")

        return msgpack.packb(package)

    cpdef Request deserialize(self, bytes request_bytes):
        """
        Deserialize the given bytes to a request.

        :param request_bytes: The bytes to deserialize.
        :return: Request.
        :raises RuntimeError: If the request cannot be deserialized.
        """
        Condition.not_empty(request_bytes, 'request_bytes')

        cdef dict unpacked_raw = msgpack.unpackb(request_bytes)
        cdef dict unpacked = {}

        # Manually unpack and decode
        for k, v in unpacked_raw.items():
            if k not in b'Query':
                if isinstance(v, bytes):
                    unpacked[k.decode(UTF8)] = v.decode(UTF8)
                else:
                    unpacked[k.decode(UTF8)] = v
            else:
                unpacked[k.decode(UTF8)] = v

        cdef str request_type = unpacked[TYPE]
        cdef GUID request_id = GUID(UUID(unpacked[ID]))
        cdef datetime request_timestamp = convert_string_to_datetime(unpacked[TIMESTAMP])

        if request_type == DataRequest.__name__:
            return DataRequest(
                self.query_serializer.deserialize(unpacked[QUERY]),
                request_id,
                request_timestamp)
        else:
            raise RuntimeError("Cannot deserialize request (unrecognized request).")


cdef class MsgPackResponseSerializer(ResponseSerializer):
    """
    Provides a response serializer for the MessagePack specification.
    """

    cpdef bytes serialize(self, Response response):
        """
        Serialize the given response to bytes.

        :param response: The response to serialize.
        :return: bytes.
        :raises RuntimeError: If the response cannot be serialized.
        """
        cdef dict package = {
            TYPE: response.__class__.__name__,
            ID: response.id.value,
            CORRELATION_ID: response.correlation_id.value,
            TIMESTAMP: convert_datetime_to_string(response.timestamp)
        }

        if isinstance(response, MessageReceived):
            package[RECEIVED_TYPE] = response.received_type
        elif isinstance(response, MessageRejected):
            package[MESSAGE] = response.received_type
        elif isinstance(response, DataResponse):
            package[DATA] = response.data
            package[DATA_ENCODING] = response.data_encoding
        else:
            raise RuntimeError("Cannot serialize response (unrecognized response.")

        return msgpack.packb(package)

    cpdef Response deserialize(self, bytes response_bytes):
        """
        Deserialize the given bytes to a response.

        :param response_bytes: The bytes to deserialize.
        :return: Response.
        :raises RuntimeError: If the response cannot be deserialized.
        """
        Condition.not_empty(response_bytes, 'response_bytes')

        cdef dict unpacked_raw = msgpack.unpackb(response_bytes)
        cdef dict unpacked = {}

        # Manually unpack and decode
        for k, v in unpacked_raw.items():
            if k not in b'Data':
                if isinstance(v, bytes):
                    unpacked[k.decode(UTF8)] = v.decode(UTF8)
                else:
                    unpacked[k.decode(UTF8)] = v
            else:
                unpacked[k.decode(UTF8)] = v

        cdef str response_type = unpacked[TYPE]
        cdef GUID correlation_id = GUID(UUID(unpacked[CORRELATION_ID]))
        cdef GUID response_id = GUID(UUID(unpacked[ID]))
        cdef datetime response_timestamp = convert_string_to_datetime(unpacked[TIMESTAMP])

        if response_type == MessageReceived.__name__:
            return MessageReceived(
                unpacked[RECEIVED_TYPE],
                correlation_id,
                response_id,
                response_timestamp)
        if response_type == MessageRejected.__name__:
            return MessageRejected(
                unpacked[MESSAGE],
                correlation_id,
                response_id,
                response_timestamp)
        if response_type == QueryFailure.__name__:
            return QueryFailure(
                unpacked[MESSAGE],
                correlation_id,
                response_id,
                response_timestamp)
        if response_type == DataResponse.__name__:
            return DataResponse(
                bytes(unpacked[DATA]),
                unpacked[DATA_ENCODING],
                correlation_id,
                response_id,
                response_timestamp)
        else:
            raise RuntimeError("Cannot deserialize response (unrecognized response).")
