#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="serialization.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

import msgpack

from cpython.datetime cimport datetime
from uuid import UUID
from datetime import datetime

from inv_trader.core.precondition cimport Precondition
from inv_trader.core.decimal cimport Decimal
from inv_trader.commands cimport Command, OrderCommand, SubmitOrder, CancelOrder, ModifyOrder
from inv_trader.commands cimport CollateralInquiry
from inv_trader.model.enums import Broker, OrderSide, OrderType, TimeInForce, CurrencyCode
from inv_trader.enums.brokerage cimport Broker
from inv_trader.enums.time_in_force cimport TimeInForce, time_in_force_string
from inv_trader.enums.order_side cimport OrderSide, order_side_string
from inv_trader.enums.order_type cimport OrderType, order_type_string
from inv_trader.enums.currency_code cimport CurrencyCode
from inv_trader.model.identifiers cimport GUID, Label, OrderId, ExecutionId, ExecutionTicket, AccountId, AccountNumber
from inv_trader.model.objects cimport Symbol
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport Event, OrderEvent, AccountEvent
from inv_trader.model.events cimport OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events cimport OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events cimport OrderPartiallyFilled, OrderFilled
from inv_trader.common.serialization import (
    _parse_symbol, _convert_price_to_string, _convert_string_to_price,
    _convert_datetime_to_string, _convert_string_to_datetime)
from inv_trader.common.serialization cimport OrderSerializer, EventSerializer, CommandSerializer


cdef str UTF8 = 'utf-8'
cdef str NONE = 'NONE'
cdef str COMMAND_TYPE = 'command_type'
cdef str COMMAND_ID = 'command_id'
cdef str COMMAND_TIMESTAMP = 'command_timestamp'
cdef str COLLATERAL_INQUIRY = 'collateral_inquiry'
cdef str ORDER_COMMAND = 'order_command'
cdef str SUBMIT_ORDER = 'submit_order'
cdef str CANCEL_ORDER = 'cancel_order'
cdef str MODIFY_ORDER = 'modify_order'
cdef str CANCEL_REASON = 'cancel_reason'
cdef str ORDER = 'order'
cdef str TIMESTAMP = 'timestamp'
cdef str EVENT_TYPE = 'event_type'
cdef str ORDER_EVENT = 'order_event'
cdef str ACCOUNT_EVENT = 'account_event'
cdef str SYMBOL = 'symbol'
cdef str ORDER_ID = 'order_id'
cdef str ORDER_ID_BROKER = 'order_id_broker'
cdef str EVENT_ID = 'event_id'
cdef str EVENT_TIMESTAMP = 'event_timestamp'
cdef str LABEL = 'label'
cdef str ORDER_SUBMITTED = 'order_submitted'
cdef str ORDER_ACCEPTED = 'order_accepted'
cdef str ORDER_REJECTED = 'order_rejected'
cdef str ORDER_WORKING = 'order_working'
cdef str ORDER_CANCELLED = 'order_cancelled'
cdef str ORDER_CANCEL_REJECT = 'order_cancel_reject'
cdef str ORDER_MODIFIED = 'order_modified'
cdef str ORDER_EXPIRED = 'order_expired'
cdef str ORDER_PARTIALLY_FILLED = 'order_partially_filled'
cdef str ORDER_FILLED = 'order_filled'
cdef str SUBMITTED_TIME = 'submitted_time'
cdef str ACCEPTED_TIME = 'accepted_time'
cdef str REJECTED_TIME = 'rejected_time'
cdef str REJECTED_RESPONSE = 'rejected_response'
cdef str REJECTED_REASON = 'rejected_reason'
cdef str WORKING_TIME = 'working_time'
cdef str CANCELLED_TIME = 'cancelled_time'
cdef str MODIFIED_TIME = 'modified_time'
cdef str MODIFIED_PRICE = 'modified_price'
cdef str EXPIRE_TIME = 'expire_time'
cdef str EXPIRED_TIME = 'expired_time'
cdef str EXECUTION_TIME = 'execution_time'
cdef str EXECUTION_ID = 'execution_id'
cdef str EXECUTION_TICKET = 'execution_ticket'
cdef str ORDER_SIDE = 'order_side'
cdef str ORDER_TYPE = 'order_type'
cdef str FILLED_QUANTITY = 'filled_quantity'
cdef str LEAVES_QUANTITY = 'leaves_quantity'
cdef str QUANTITY = 'quantity'
cdef str AVERAGE_PRICE = 'average_price'
cdef str PRICE = 'price'
cdef str TIME_IN_FORCE = 'time_in_force'
cdef str ACCOUNT_ID = 'account_id'
cdef str ACCOUNT_NUMBER = 'account_number'
cdef str BROKER = 'broker'
cdef str CURRENCY = 'currency'
cdef str CASH_BALANCE = 'cash_balance'
cdef str CASH_START_DAY = 'cash_start_day'
cdef str CASH_ACTIVITY_DAY = 'cash_activity_day'
cdef str MARGIN_USED_LIQUIDATION = 'margin_used_liquidation'
cdef str MARGIN_USED_MAINTENANCE = 'margin_used_maintenance'
cdef str MARGIN_RATIO = 'margin_ratio'
cdef str MARGIN_CALL_STATUS = 'margin_call_status'


cdef class MsgPackOrderSerializer(OrderSerializer):
    """
    Provides a command serializer for the Message Pack specification
    """

    cpdef bytes serialize(self, Order order):
        """
        Serialize the given Order to Message Pack specification bytes.

        :param order: The order to serialize.
        :return: The serialized order.
        """
        return msgpack.packb({
            SYMBOL: str(order.symbol),
            ORDER_ID: str(order.id),
            LABEL: str(order.label),
            ORDER_SIDE: order_side_string(order.side),
            ORDER_TYPE: order_type_string(order.type),
            QUANTITY: order.quantity,
            TIMESTAMP: _convert_datetime_to_string(order.timestamp),
            PRICE: _convert_price_to_string(order.price),
            TIME_IN_FORCE: time_in_force_string(order.time_in_force),
            EXPIRE_TIME: _convert_datetime_to_string(order.expire_time)
            }, encoding=UTF8)

    cpdef Order deserialize(self, bytes order_bytes):
        """
        Deserialize the given Message Pack specification bytes to an Order.

        :param order_bytes: The bytes to deserialize.
        :return: The deserialized order.
        :raises ValueError: If the event_bytes is empty.
        """
        Precondition.not_empty(order_bytes, 'order_bytes')

        cdef dict unpacked = msgpack.unpackb(order_bytes, raw=False)

        return Order(symbol=_parse_symbol(unpacked[SYMBOL]),
                     order_id=OrderId(unpacked[ORDER_ID]),
                     label=Label(unpacked[LABEL]),
                     order_side=OrderSide[unpacked[ORDER_SIDE]],
                     order_type=OrderType[unpacked[ORDER_TYPE]],
                     quantity=unpacked[QUANTITY],
                     timestamp=_convert_string_to_datetime(unpacked[TIMESTAMP]),
                     price=_convert_string_to_price(unpacked[PRICE]),
                     time_in_force=TimeInForce[unpacked[TIME_IN_FORCE]],
                     expire_time=_convert_string_to_datetime(unpacked[EXPIRE_TIME]))


cdef class MsgPackCommandSerializer(CommandSerializer):
    """
    Provides a command serializer for the Message Pack specification.
    """

    def __init__(self):
        """
        Initializes a new instance of the MsgPackCommandSerializer class.
        """
        self.order_serializer = MsgPackOrderSerializer()

    cpdef bytes serialize(self, Command command):
        """
        Serialize the given command to Message Pack specification bytes.

        :param: command: The command to serialize.
        :return: The serialized command.
        :raises: ValueError: If the command cannot be serialized.
        """
        if isinstance(command, OrderCommand):
            return self._serialize_order_command(command)

        cdef dict package = {
            COMMAND_ID: str(command.id),
            COMMAND_TIMESTAMP: _convert_datetime_to_string(command.timestamp)
        }

        if isinstance(command, CollateralInquiry):
            package[COMMAND_TYPE] = COLLATERAL_INQUIRY
            return msgpack.packb(package)
        else:
            raise ValueError("Cannot serialize command (unrecognized command).")

    cpdef Command deserialize(self, bytes command_bytes):
        """
        Deserialize the given Message Pack specification bytes to a command.

        :param command_bytes: The command to deserialize.
        :return: The deserialized command.
        :raises ValueError: If the command_bytes is empty.
        :raises ValueError: If the command cannot be deserialized.
        """
        Precondition.not_empty(command_bytes, 'command_bytes')

        cdef dict unpacked = msgpack.unpackb(command_bytes, raw=False)
        cdef str command_type = unpacked[COMMAND_TYPE]
        cdef GUID command_id = GUID(UUID(unpacked[COMMAND_ID]))
        cdef datetime command_timestamp = _convert_string_to_datetime(unpacked[COMMAND_TIMESTAMP])

        if command_type == ORDER_COMMAND:
            return self._deserialize_order_command(
                command_id,
                command_timestamp,
                unpacked)
        elif command_type == COLLATERAL_INQUIRY:
            return CollateralInquiry(
                command_id,
                command_timestamp)
        else:
            raise ValueError("Cannot deserialize command (unrecognized command).")

    cdef bytes _serialize_order_command(self, OrderCommand order_command):
        """
        Serialize the given order command to Message Pack specification bytes.

        :param order_command: The order command to serialize.
        :return: The serialized order command.
        :raises ValueError: If the order command cannot be serialized.
        """
        cdef dict package = {
            COMMAND_ID: str(order_command.id),
            COMMAND_TIMESTAMP: _convert_datetime_to_string(order_command.timestamp)
        }

        package[COMMAND_TYPE] = ORDER_COMMAND
        package[ORDER] = self.order_serializer.serialize(order_command.order).hex()

        if isinstance(order_command, SubmitOrder):
            package[ORDER_COMMAND] = SUBMIT_ORDER
            return msgpack.packb(package)
        elif isinstance(order_command, CancelOrder):
            package[ORDER_COMMAND] = CANCEL_ORDER
            package[CANCEL_REASON] = order_command.cancel_reason
            return msgpack.packb(package)
        elif isinstance(order_command, ModifyOrder):
            package[ORDER_COMMAND] = MODIFY_ORDER
            package[MODIFIED_PRICE] = str(order_command.modified_price)
            return msgpack.packb(package)
        else:
            raise ValueError("Cannot serialize order command (unrecognized command).")

    cdef OrderCommand _deserialize_order_command(
            self,
            GUID command_id,
            datetime command_timestamp,
            dict unpacked):
        """
        Deserialize the given parameters to an order command.

        :param command_id: The commands order id.
        :param command_timestamp: The commands timestamp.
        :param unpacked: The commands unpacked dictionary.
        :return: The deserialized order command.
        :raises ValueError: If the order command cannot be deserialized.
        """
        cdef str order_command = unpacked[ORDER_COMMAND]
        cdef Order order = self.order_serializer.deserialize(bytes.fromhex(unpacked[ORDER]))

        if order_command == SUBMIT_ORDER:
            return SubmitOrder(
                order,
                command_id,
                command_timestamp)
        elif order_command == CANCEL_ORDER:
            return CancelOrder(
                order,
                unpacked[CANCEL_REASON],
                command_id,
                command_timestamp)
        elif order_command == MODIFY_ORDER:
            return ModifyOrder(
                order,
                Decimal(unpacked[MODIFIED_PRICE]),
                command_id,
                command_timestamp)
        else:
            raise ValueError("Cannot deserialize order command (unrecognized bytes pattern).")


cdef class MsgPackEventSerializer(EventSerializer):
    """
    Provides an event serializer for the Message Pack specification
    """

    cpdef bytes serialize(self, Event event):
        """
        Serialize the event to Message Pack specification bytes.

        :param event: The event to serialize.
        :return: The serialized event.
        :raises: ValueError: If the event cannot be serialized.
        """
        if isinstance(event, OrderEvent):
            return self._serialize_order_event(event)
        else:
            raise ValueError("Cannot serialize event (unrecognized event).")

    cpdef Event deserialize(self, bytes event_bytes):
        """
        Deserialize the given Message Pack specification bytes to an event.

        :param event_bytes: The bytes to deserialize.
        :return: The deserialized event.
        :raises ValueError: If the event_bytes is empty.
        :raises ValueError: If the event cannot be deserialized.
        """
        Precondition.not_empty(event_bytes, 'event_bytes')

        cdef dict unpacked = msgpack.unpackb(event_bytes, raw=False)

        cdef str event_type = unpacked[EVENT_TYPE]
        cdef GUID event_id = GUID(UUID(unpacked[EVENT_ID]))
        cdef datetime event_timestamp = _convert_string_to_datetime(unpacked[EVENT_TIMESTAMP])

        if event_type == ORDER_EVENT:
            return self._deserialize_order_event(
                event_id,
                event_timestamp,
                unpacked)
        elif event_type == ACCOUNT_EVENT:
            return AccountEvent(
                AccountId(unpacked[ACCOUNT_ID]),
                Broker[unpacked[BROKER]],
                AccountNumber(unpacked[ACCOUNT_NUMBER]),
                CurrencyCode[unpacked[CURRENCY]],
                Decimal(unpacked[CASH_BALANCE]),
                Decimal(unpacked[CASH_START_DAY]),
                Decimal(unpacked[CASH_ACTIVITY_DAY]),
                Decimal(unpacked[MARGIN_USED_LIQUIDATION]),
                Decimal(unpacked[MARGIN_USED_MAINTENANCE]),
                Decimal(unpacked[MARGIN_RATIO]),
                unpacked[MARGIN_CALL_STATUS],
                event_id,
                event_timestamp)
        else:
            raise ValueError("Cannot deserialize event (unrecognized event).")

    cdef bytes _serialize_order_event(self, OrderEvent order_event):
        """
        Serialize the given order event to Message Pack specification bytes.

        :param order_event: The order event to serialize.
        :return: The serialized order event.
        :raises ValueError: If the order event cannot be serialized.
        """
        cdef dict package = {
            EVENT_TYPE: ORDER_EVENT,
            SYMBOL: str(order_event.symbol),
            ORDER_ID: str(order_event.order_id),
            EVENT_ID: str(order_event.id),
            EVENT_TIMESTAMP: _convert_datetime_to_string(order_event.timestamp)
        }

        if isinstance(order_event, OrderSubmitted):
            package[ORDER_EVENT] = ORDER_SUBMITTED
            package[SUBMITTED_TIME] = _convert_datetime_to_string(order_event.submitted_time)
            return msgpack.packb(package)

        if isinstance(order_event, OrderAccepted):
            package[ORDER_EVENT] = ORDER_ACCEPTED
            package[ACCEPTED_TIME] = _convert_datetime_to_string(order_event.accepted_time)
            return msgpack.packb(package)

        if isinstance(order_event, OrderRejected):
            package[ORDER_EVENT] = ORDER_REJECTED
            package[REJECTED_TIME] = _convert_datetime_to_string(order_event.rejected_time)
            package[REJECTED_REASON] = order_event.rejected_reason
            return msgpack.packb(package)

        if isinstance(order_event, OrderWorking):
            package[ORDER_EVENT] = ORDER_WORKING
            package[ORDER_ID_BROKER] = str(order_event.broker_order_id)
            package[LABEL] = str(order_event.label)
            package[ORDER_SIDE] = order_side_string(order_event.order_side)
            package[ORDER_TYPE] = order_type_string(order_event.order_type)
            package[QUANTITY] = order_event.quantity
            package[PRICE] = str(order_event.price)
            package[TIME_IN_FORCE] = time_in_force_string(order_event.time_in_force)
            package[WORKING_TIME] = _convert_datetime_to_string(order_event.working_time)
            package[EXPIRE_TIME] = _convert_datetime_to_string(order_event.expire_time)
            return msgpack.packb(package)

        if isinstance(order_event, OrderCancelled):
            package[ORDER_EVENT] = ORDER_CANCELLED
            package[CANCELLED_TIME] = _convert_datetime_to_string(order_event.cancelled_time)
            return msgpack.packb(package)

        if isinstance(order_event, OrderCancelReject):
            package[ORDER_EVENT] = ORDER_CANCEL_REJECT
            package[REJECTED_TIME] = _convert_datetime_to_string(order_event.cancel_reject_time)
            package[REJECTED_RESPONSE] = order_event.cancel_reject_response
            package[REJECTED_REASON] = order_event.cancel_reject_reason
            return msgpack.packb(package)

        if isinstance(order_event, OrderModified):
            package[ORDER_EVENT] = ORDER_MODIFIED
            package[ORDER_ID_BROKER] = str(order_event.broker_order_id)
            package[MODIFIED_TIME] = _convert_datetime_to_string(order_event.modified_time)
            package[MODIFIED_PRICE] = str(order_event.modified_price)
            return msgpack.packb(package)

        if isinstance(order_event, OrderExpired):
            package[ORDER_EVENT] = ORDER_EXPIRED
            package[EXPIRED_TIME] = _convert_datetime_to_string(order_event.expired_time)
            return msgpack.packb(package)

        if isinstance(order_event, OrderPartiallyFilled):
            package[ORDER_EVENT] = ORDER_PARTIALLY_FILLED
            package[EXECUTION_ID] = str(order_event.execution_id)
            package[EXECUTION_TICKET] = str(order_event.execution_ticket)
            package[ORDER_SIDE] = order_side_string(order_event.order_side)
            package[FILLED_QUANTITY] = order_event.filled_quantity
            package[LEAVES_QUANTITY] = order_event.leaves_quantity
            package[AVERAGE_PRICE] = str(order_event.average_price)
            package[EXECUTION_TIME] = _convert_datetime_to_string(order_event.execution_time)
            return msgpack.packb(package)

        if isinstance(order_event, OrderFilled):
            package[ORDER_EVENT] = ORDER_FILLED
            package[EXECUTION_ID] = str(order_event.execution_id)
            package[EXECUTION_TICKET] = str(order_event.execution_ticket)
            package[ORDER_SIDE] = order_side_string(order_event.order_side)
            package[FILLED_QUANTITY] = order_event.filled_quantity
            package[AVERAGE_PRICE] = str(order_event.average_price)
            package[EXECUTION_TIME] = _convert_datetime_to_string(order_event.execution_time)
            return msgpack.packb(package)

        else:
            raise ValueError("Cannot serialize event (unrecognized event.")

    cdef OrderEvent _deserialize_order_event(
            self,
            GUID event_id,
            datetime event_timestamp,
            dict unpacked):
        """
        Deserialize the given parameters to an order event.

        :param event_id: The events order id.
        :param event_timestamp: The events timestamp.
        :param unpacked: The events unpacked dictionary.
        :return: The deserialized order event.
        :raises ValueError: If the order event cannot be deserialized.
        """
        cdef Symbol order_symbol = _parse_symbol(unpacked[SYMBOL])
        cdef OrderId order_id = OrderId(unpacked[ORDER_ID])
        cdef str order_event = unpacked[ORDER_EVENT]

        if order_event == ORDER_SUBMITTED:
            return OrderSubmitted(
                order_symbol,
                order_id,
                _convert_string_to_datetime(unpacked[SUBMITTED_TIME]),
                event_id,
                event_timestamp)
        elif order_event == ORDER_ACCEPTED:
            return OrderAccepted(
                order_symbol,
                order_id,
                _convert_string_to_datetime(unpacked[ACCEPTED_TIME]),
                event_id,
                event_timestamp)
        elif order_event == ORDER_REJECTED:
            return OrderRejected(
                order_symbol,
                order_id,
                _convert_string_to_datetime(unpacked[REJECTED_TIME]),
                unpacked[REJECTED_REASON],
                event_id,
                event_timestamp)
        elif order_event == ORDER_WORKING:
            return OrderWorking(
                order_symbol,
                order_id,
                OrderId(unpacked[ORDER_ID_BROKER]),
                Label(unpacked[LABEL]),
                OrderSide[unpacked[ORDER_SIDE]],
                OrderType[unpacked[ORDER_TYPE]],
                unpacked[QUANTITY],
                Decimal(unpacked[PRICE]),
                TimeInForce[unpacked[TIME_IN_FORCE]],
                _convert_string_to_datetime(unpacked[WORKING_TIME]),
                event_id,
                event_timestamp,
                _convert_string_to_datetime(unpacked[EXPIRE_TIME]))
        elif order_event == ORDER_CANCELLED:
            return OrderCancelled(
                order_symbol,
                order_id,
                _convert_string_to_datetime(unpacked[CANCELLED_TIME]),
                event_id,
                event_timestamp)
        elif order_event == ORDER_CANCEL_REJECT:
            return OrderCancelReject(
                order_symbol,
                order_id,
                _convert_string_to_datetime(unpacked[REJECTED_TIME]),
                unpacked[REJECTED_RESPONSE],
                unpacked[REJECTED_REASON],
                event_id,
                event_timestamp)
        elif order_event == ORDER_MODIFIED:
            return OrderModified(
                order_symbol,
                order_id,
                OrderId(unpacked[ORDER_ID_BROKER]),
                Decimal(unpacked[MODIFIED_PRICE]),
                _convert_string_to_datetime(unpacked[MODIFIED_TIME]),
                event_id,
                event_timestamp)
        elif order_event == ORDER_EXPIRED:
            return OrderExpired(
                order_symbol,
                order_id,
                _convert_string_to_datetime(unpacked[EXPIRED_TIME]),
                event_id,
                event_timestamp)
        elif order_event == ORDER_PARTIALLY_FILLED:
            return OrderPartiallyFilled(
                order_symbol,
                order_id,
                ExecutionId(unpacked[EXECUTION_ID]),
                ExecutionTicket(unpacked[EXECUTION_TICKET]),
                OrderSide[unpacked[ORDER_SIDE]],
                int(unpacked[FILLED_QUANTITY]),
                int(unpacked[LEAVES_QUANTITY]),
                Decimal(unpacked[AVERAGE_PRICE]),
                _convert_string_to_datetime(unpacked[EXECUTION_TIME]),
                event_id,
                event_timestamp)
        elif order_event == ORDER_FILLED:
            return OrderFilled(
                order_symbol,
                order_id,
                ExecutionId(unpacked[EXECUTION_ID]),
                ExecutionTicket(unpacked[EXECUTION_TICKET]),
                OrderSide[unpacked[ORDER_SIDE]],
                int(unpacked[FILLED_QUANTITY]),
                Decimal(unpacked[AVERAGE_PRICE]),
                _convert_string_to_datetime(unpacked[EXECUTION_TIME]),
                event_id,
                event_timestamp)
        else:
            raise ValueError("Cannot deserialize event_bytes (unrecognized bytes pattern.")
