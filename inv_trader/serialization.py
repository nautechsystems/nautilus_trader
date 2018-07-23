#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="serialization.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import abc
import msgpack
import iso8601

from uuid import UUID
from datetime import datetime
from decimal import Decimal
from typing import Dict, Optional

from inv_trader.core.checks import typechecking
from inv_trader.model.enums import Venue, OrderSide, OrderType, TimeInForce
from inv_trader.model.objects import Symbol
from inv_trader.model.order import Order
from inv_trader.model.events import Event, OrderEvent
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled
from inv_trader.model.commands import Command, OrderCommand, SubmitOrder, CancelOrder, ModifyOrder

# Constants
UTF8 = 'utf-8'
NONE = 'NONE'
COMMAND_TYPE = 'command_type'
COMMAND_ID = 'command_id'
COMMAND_TIMESTAMP = 'command_timestamp'
ORDER_COMMAND = 'order_command'
SUBMIT_ORDER = 'submit_order'
CANCEL_ORDER = 'cancel_order'
MODIFY_ORDER = 'modify_order'
ORDER = 'order'
TIMESTAMP = 'timestamp'
EVENT_TYPE = 'event_type'
ORDER_EVENT = 'order_event'
ACCOUNT_EVENT = 'account_event'
SYMBOL = 'symbol'
ORDER_ID = 'order_id'
ORDER_ID_BROKER = 'order_id_broker'
EVENT_ID = 'event_id'
EVENT_TIMESTAMP = 'event_timestamp'
LABEL = 'label'
ORDER_SUBMITTED = 'order_submitted'
ORDER_ACCEPTED = 'order_accepted'
ORDER_REJECTED = 'order_rejected'
ORDER_WORKING = 'order_working'
ORDER_CANCELLED = 'order_cancelled'
ORDER_CANCEL_REJECT = 'order_cancel_reject'
ORDER_MODIFIED = 'order_modified'
ORDER_EXPIRED = 'order_expired'
ORDER_PARTIALLY_FILLED = 'order_partially_filled'
ORDER_FILLED = 'order_filled'
SUBMITTED_TIME = 'submitted_time'
ACCEPTED_TIME = 'accepted_time'
REJECTED_TIME = 'rejected_time'
REJECTED_RESPONSE = 'rejected_response'
REJECTED_REASON = 'rejected_reason'
WORKING_TIME = 'working_time'
CANCELLED_TIME = 'cancelled_time'
MODIFIED_TIME = 'modified_time'
MODIFIED_PRICE = 'modified_price'
EXPIRE_TIME = 'expire_time'
EXPIRED_TIME = 'expired_time'
EXECUTION_TIME = 'execution_time'
EXECUTION_ID = 'execution_id'
EXECUTION_TICKET = 'execution_ticket'
ORDER_SIDE = 'order_side'
ORDER_TYPE = 'order_type'
FILLED_QUANTITY = 'filled_quantity'
LEAVES_QUANTITY = 'leaves_quantity'
QUANTITY = 'quantity'
AVERAGE_PRICE = 'average_price'
PRICE = 'price'
TIME_IN_FORCE = 'time_in_force'


@typechecking
def _parse_symbol(symbol_string: str) -> Symbol:
    """
    Parse the given string to a Symbol.

    :param symbol_string: The symbol string to parse.
    :return: The parsed symbol.
    """
    split_symbol = symbol_string.split('.')
    return Symbol(split_symbol[0], Venue[split_symbol[1].upper()])


def _convert_price_to_string(price: Optional[Decimal]) -> str:
    """
    Convert the given object to a decimal or 'NONE' string.

    :param price: The price to convert.
    :return: The converted string.
    """
    return NONE if price is None else str(price)


def _convert_string_to_price(price_string: str) -> Optional[Decimal]:
    """
    Convert the given price string to a Decimal or None.

    :param price_string: The price string to convert.
    :return: The converted price, or None.
    """
    return None if price_string == NONE else Decimal(price_string)


def _convert_datetime_to_string(expire_time: Optional[datetime]) -> str:
    """
    Convert the given object to a valid ISO8601 string, or 'NONE'.

    :param expire_time: The datetime string to convert
    :return: The converted string.
    """
    return (NONE if expire_time is None
            else expire_time.isoformat(timespec='milliseconds').replace('+00:00', 'Z'))


def _convert_string_to_datetime(expire_time_string: str) -> Optional[datetime]:
    """
    Convert the given string to a datetime object, or None.

    :param expire_time_string: The string to convert.
    :return: The converted datetime, or None.
    """
    return None if expire_time_string == NONE else iso8601.parse_date(expire_time_string)


class OrderSerializer:
    """
    The abstract base class for all order serializers.
    """

    __metaclass__ = abc.ABCMeta

    @staticmethod
    @typechecking
    @abc.abstractmethod
    def serialize(order: Order) -> bytes:
        """
        Serialize the given order to bytes.

        :param: order: The order to serialize.
        :return: The serialized order.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")

    @staticmethod
    @typechecking
    @abc.abstractmethod
    def deserialize(order_bytes: bytes) -> Order:
        """
        Deserialize the given bytes to an Order.

        :param: order_bytes: The bytes to deserialize.
        :return: The deserialized order.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")


class MsgPackOrderSerializer(OrderSerializer):
    """
    Provides a command serializer for the Message Pack specification
    """

    @staticmethod
    @typechecking
    def serialize(order: Order) -> bytes:
        """
        Serialize the given Order to Message Pack specification bytes.

        :param: order: The order to serialize.
        :return: The serialized order.
        """
        return msgpack.packb({
            SYMBOL: str(order.symbol),
            ORDER_ID: order.id,
            LABEL: order.label,
            ORDER_SIDE: order.side.name,
            ORDER_TYPE: order.type.name,
            QUANTITY: order.quantity,
            TIMESTAMP: _convert_datetime_to_string(order.timestamp),
            PRICE: _convert_price_to_string(order.price),
            TIME_IN_FORCE: order.time_in_force.name,
            EXPIRE_TIME: _convert_datetime_to_string(order.expire_time)
            })

    @staticmethod
    @typechecking
    def deserialize(order_bytes: bytes) -> Order:
        """
        Deserialize the given bytes to an Order.

        :param: order_bytes: The bytes to deserialize.
        :return: The deserialized order.
        """
        unpacked = msgpack.unpackb(order_bytes, encoding=UTF8)

        # Deserialize expire_time (could be 'none').
        expire_time = unpacked[EXPIRE_TIME]
        if expire_time == NONE:
            expire_time = None
        else:
            expire_time = iso8601.parse_date(unpacked[EXPIRE_TIME])

        return Order(symbol=_parse_symbol(unpacked[SYMBOL]),
                     order_id=unpacked[ORDER_ID],
                     label=unpacked[LABEL],
                     order_side=OrderSide[unpacked[ORDER_SIDE]],
                     order_type=OrderType[unpacked[ORDER_TYPE]],
                     quantity=unpacked[QUANTITY],
                     timestamp=iso8601.parse_date(unpacked[TIMESTAMP]),
                     price=_convert_string_to_price(unpacked[PRICE]),
                     time_in_force=TimeInForce[unpacked[TIME_IN_FORCE]],
                     expire_time=expire_time)


class CommandSerializer:
    """
    The abstract base class for all command serializers.
    """

    __metaclass__ = abc.ABCMeta

    @staticmethod
    @typechecking
    @abc.abstractmethod
    def serialize(command: Command) -> bytes:
        """
        Serialize the given command to bytes to be sent.

        :param: command: The command to serialize.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")

    @staticmethod
    @typechecking
    @abc.abstractmethod
    def deserialize(command_bytes: bytes) -> Command:
        """
        Deserialize the given command bytes to a Command.

        :param: command_bytes: The command bytes to deserialize.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")


class MsgPackCommandSerializer(CommandSerializer):
    """
    Provides a command serializer for the Message Pack specification
    """

    @staticmethod
    @typechecking
    def serialize(command: Command) -> bytes:
        """
        Serialize the given command to Message Pack specification bytes.

        :param: command: The command to serialize.
        :return: The serialized command.
        """
        if isinstance(command, OrderCommand):
            return MsgPackCommandSerializer._serialize_order_command(command)

        else:
            raise ValueError("Cannot serialize command (unrecognized command).")

    @staticmethod
    @typechecking
    def deserialize(command_bytes: bytes) -> Command:
        """
        Deserialize the given command bytes from Message Pack specification
        bytes to a command.

        :param: command: The command to serialize.
        :return: The deserialized command.
        """
        unpacked = msgpack.unpackb(command_bytes, encoding=UTF8)

        command_type = unpacked[COMMAND_TYPE]
        command_id = UUID(unpacked[COMMAND_ID])
        command_timestamp = iso8601.parse_date(unpacked[COMMAND_TIMESTAMP])

        if command_type == ORDER_COMMAND:
            return MsgPackCommandSerializer._deserialize_order_command(
                command_id,
                command_timestamp,
                unpacked)
        else:
            raise ValueError("Cannot deserialize command (unrecognized command).")

    @staticmethod
    @typechecking
    def _serialize_order_command(order_command: OrderCommand) -> bytes:
        """
        Serialize the given order command to Message Pack specification bytes.

        :param order_command: The order command to serialize.
        :return: The serialized order command.
        """
        package = {
            COMMAND_TYPE: ORDER_COMMAND,
            SYMBOL: str(order_command.symbol),
            ORDER_ID: order_command.order_id,
            COMMAND_ID: str(order_command.command_id),
            COMMAND_TIMESTAMP: _convert_datetime_to_string(order_command.command_timestamp)
        }

        if isinstance(order_command, SubmitOrder):
            package[ORDER_COMMAND] = SUBMIT_ORDER
            package[ORDER] = MsgPackOrderSerializer.serialize(order_command.order).hex()
            return msgpack.packb(package)

        if isinstance(order_command, CancelOrder):
            package[ORDER_COMMAND] = CANCEL_ORDER
            return msgpack.packb(package)

        if isinstance(order_command, ModifyOrder):
            package[ORDER_COMMAND] = MODIFY_ORDER
            package[MODIFIED_PRICE] = str(order_command.modified_price)
            return msgpack.packb(package)

        else:
            raise ValueError("Cannot serialize order command (unrecognized command).")

    @staticmethod
    @typechecking
    def _deserialize_order_command(
            command_id: UUID,
            command_timestamp: datetime,
            unpacked: Dict) -> OrderCommand:
        """
        Deserialize the given order command.

        :param command_id: The commands order id.
        :param command_timestamp: The commands timestamp.
        :param unpacked: The commands unpacked dictionary.
        :return: The deserialized order command.
        """
        order_symbol = _parse_symbol(unpacked[SYMBOL])
        order_id = unpacked[ORDER_ID]
        order_command = unpacked[ORDER_COMMAND]

        if order_command == SUBMIT_ORDER:
            order = MsgPackOrderSerializer.deserialize(bytes.fromhex(unpacked[ORDER]))
            return SubmitOrder(
                order,
                command_id,
                command_timestamp)

        if order_command == CANCEL_ORDER:
            return CancelOrder(
                order_symbol,
                order_id,
                command_id,
                command_timestamp)

        if order_command == MODIFY_ORDER:
            return ModifyOrder(
                order_symbol,
                order_id,
                Decimal(unpacked[MODIFIED_PRICE]),
                command_id,
                command_timestamp)

        else:
            raise ValueError("Cannot deserialize order command (unrecognized bytes pattern).")


class EventSerializer:
    """
    The abstract base class for all event serializers.
    """

    __metaclass__ = abc.ABCMeta

    @staticmethod
    @typechecking
    @abc.abstractmethod
    def deserialize(event_bytes: bytes) -> OrderEvent:
        """
        Deserialize the given bytes to an order event.

        :param: event_bytes: The bytes to deserialize.
        :return: The deserialized order event.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")


class MsgPackEventSerializer(EventSerializer):
    """
    Provides an event serializer for the Message Pack specification
    """

    @staticmethod
    @typechecking
    def deserialize(event_bytes: bytes) -> Event:
        """
        Deserialize the given Message Pack bytes to an order event.

        :param event_bytes: The bytes to deserialize.
        :return: The deserialized order event.
        """
        unpacked = msgpack.unpackb(event_bytes, encoding=UTF8)

        event_type = unpacked[EVENT_TYPE]
        event_id = UUID(unpacked[EVENT_ID])
        event_timestamp = _convert_string_to_datetime(unpacked[EVENT_TIMESTAMP])

        if event_type == ORDER_EVENT:
            return MsgPackEventSerializer._deserialize_order_event(
                event_id,
                event_timestamp,
                unpacked)

        else:
            raise ValueError("Cannot deserialize event (unrecognized event).")

    @staticmethod
    @typechecking
    def _deserialize_order_event(
            event_id: UUID,
            event_timestamp: datetime,
            unpacked: Dict) -> OrderEvent:
        """
        Deserialize the given order event.

        :param event_id: The events order id.
        :param event_timestamp: The events timestamp.
        :param unpacked: The events unpacked dictionary.
        :return: The deserialized order event.
        """
        order_symbol = _parse_symbol(unpacked[SYMBOL])
        order_id = unpacked[ORDER_ID]
        order_event = unpacked[ORDER_EVENT]

        if order_event == ORDER_SUBMITTED:
            return OrderSubmitted(
                order_symbol,
                order_id,
                _convert_string_to_datetime(unpacked[SUBMITTED_TIME]),
                event_id,
                event_timestamp)

        if order_event == ORDER_ACCEPTED:
            return OrderAccepted(
                order_symbol,
                order_id,
                _convert_string_to_datetime(unpacked[ACCEPTED_TIME]),
                event_id,
                event_timestamp)

        if order_event == ORDER_REJECTED:
            return OrderRejected(
                order_symbol,
                order_id,
                _convert_string_to_datetime(unpacked[REJECTED_TIME]),
                unpacked[REJECTED_REASON],
                event_id,
                event_timestamp)

        if order_event == ORDER_WORKING:
            return OrderWorking(
                order_symbol,
                order_id,
                unpacked[ORDER_ID_BROKER],
                unpacked[LABEL],
                OrderSide[unpacked[ORDER_SIDE]],
                OrderType[unpacked[ORDER_TYPE]],
                unpacked[QUANTITY],
                Decimal(unpacked[PRICE]),
                TimeInForce[unpacked[TIME_IN_FORCE]],
                _convert_string_to_datetime(unpacked[WORKING_TIME]),
                event_id,
                event_timestamp,
                _convert_string_to_datetime(unpacked[EXPIRE_TIME]))

        if order_event == ORDER_CANCELLED:
            return OrderCancelled(
                order_symbol,
                order_id,
                _convert_string_to_datetime(unpacked[CANCELLED_TIME]),
                event_id,
                event_timestamp)

        if order_event == ORDER_CANCEL_REJECT:
            return OrderCancelReject(
                order_symbol,
                order_id,
                _convert_string_to_datetime(unpacked[REJECTED_TIME]),
                unpacked[REJECTED_RESPONSE],
                unpacked[REJECTED_REASON],
                event_id,
                event_timestamp)

        if order_event == ORDER_MODIFIED:
            return OrderModified(
                order_symbol,
                order_id,
                unpacked[ORDER_ID_BROKER],
                Decimal(unpacked[MODIFIED_PRICE]),
                _convert_string_to_datetime(unpacked[MODIFIED_TIME]),
                event_id,
                event_timestamp)

        if order_event == ORDER_EXPIRED:
            return OrderExpired(
                order_symbol,
                order_id,
                _convert_string_to_datetime(unpacked[EXPIRED_TIME]),
                event_id,
                event_timestamp)

        if order_event == ORDER_PARTIALLY_FILLED:
            return OrderPartiallyFilled(
                order_symbol,
                order_id,
                unpacked[EXECUTION_ID],
                unpacked[EXECUTION_TICKET],
                OrderSide[unpacked[ORDER_SIDE]],
                int(unpacked[FILLED_QUANTITY]),
                int(unpacked[LEAVES_QUANTITY]),
                Decimal(unpacked[AVERAGE_PRICE]),
                _convert_string_to_datetime(unpacked[EXECUTION_TIME]),
                event_id,
                event_timestamp)

        if order_event == ORDER_FILLED:
            return OrderFilled(
                order_symbol,
                order_id,
                unpacked[EXECUTION_ID],
                unpacked[EXECUTION_TICKET],
                OrderSide[unpacked[ORDER_SIDE]],
                int(unpacked[FILLED_QUANTITY]),
                Decimal(unpacked[AVERAGE_PRICE]),
                _convert_string_to_datetime(unpacked[EXECUTION_TIME]),
                event_id,
                event_timestamp)

        else:
            raise ValueError("Cannot deserialize event_bytes (unrecognized bytes pattern.")
