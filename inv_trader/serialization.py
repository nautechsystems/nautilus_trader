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
from typing import Dict

from inv_trader.core.precondition import Precondition
from inv_trader.commands import Command, OrderCommand, SubmitOrder, CancelOrder, ModifyOrder
from inv_trader.commands import CollateralInquiry
from inv_trader.model.enums import Venue, OrderSide, OrderType, TimeInForce, CurrencyCode, Broker
from inv_trader.model.objects import Symbol
from inv_trader.model.order import Order
from inv_trader.model.events import Event, OrderEvent, AccountEvent
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled


# Constants
UTF8 = 'utf-8'
NONE = 'NONE'
COMMAND_TYPE = 'command_type'
COMMAND_ID = 'command_id'
COMMAND_TIMESTAMP = 'command_timestamp'
COLLATERAL_INQUIRY = 'collateral_inquiry'
ORDER_COMMAND = 'order_command'
SUBMIT_ORDER = 'submit_order'
CANCEL_ORDER = 'cancel_order'
MODIFY_ORDER = 'modify_order'
CANCEL_REASON = 'cancel_reason'
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
ACCOUNT_ID = 'account_id'
ACCOUNT_NUMBER = 'account_number'
BROKER = 'broker'
CURRENCY = 'currency'
CASH_BALANCE = 'cash_balance'
CASH_START_DAY = 'cash_start_day'
CASH_ACTIVITY_DAY = 'cash_activity_day'
MARGIN_USED_LIQUIDATION = 'margin_used_liquidation'
MARGIN_USED_MAINTENANCE = 'margin_used_maintenance'
MARGIN_RATIO = 'margin_ratio'
MARGIN_CALL_STATUS = 'margin_call_status'


def _parse_symbol(symbol_string: str) -> Symbol:
    """
    Parse the given string to a Symbol.

    :param symbol_string: The symbol string to parse.
    :return: The parsed symbol.
    """
    split_symbol = symbol_string.split('.')
    return Symbol(split_symbol[0], Venue[split_symbol[1].upper()])


def _convert_price_to_string(price: Decimal or None) -> str:
    """
    Convert the given object to a decimal or 'NONE' string.

    :param price: The price to convert.
    :return: The converted string.
    """
    return NONE if price is None else str(price)


def _convert_string_to_price(price_string: str) -> Decimal or None:
    """
    Convert the given price string to a Decimal or None.

    :param price_string: The price string to convert.
    :return: The converted price, or None.
    """
    return None if price_string == NONE else Decimal(price_string)


def _convert_datetime_to_string(expire_time: datetime or None) -> str:
    """
    Convert the given object to a valid ISO8601 string, or 'NONE'.

    :param expire_time: The datetime string to convert
    :return: The converted string.
    """
    return (NONE if expire_time is None
            else expire_time.isoformat(timespec='milliseconds').replace('+00:00', 'Z'))


def _convert_string_to_datetime(expire_time_string: str) -> datetime or None:
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
            }, encoding=UTF8)

    @staticmethod
    def deserialize(order_bytes: bytes) -> Order:
        """
        Deserialize the given Message Pack specification bytes to an Order.

        :param: order_bytes: The bytes to deserialize.
        :return: The deserialized order.
        """
        Precondition.not_empty(order_bytes, 'order_bytes')

        unpacked = msgpack.unpackb(order_bytes, encoding=UTF8)

        return Order(symbol=_parse_symbol(unpacked[SYMBOL]),
                     order_id=unpacked[ORDER_ID],
                     label=unpacked[LABEL],
                     order_side=OrderSide[unpacked[ORDER_SIDE]],
                     order_type=OrderType[unpacked[ORDER_TYPE]],
                     quantity=unpacked[QUANTITY],
                     timestamp=_convert_string_to_datetime(unpacked[TIMESTAMP]),
                     price=_convert_string_to_price(unpacked[PRICE]),
                     time_in_force=TimeInForce[unpacked[TIME_IN_FORCE]],
                     expire_time=_convert_string_to_datetime(unpacked[EXPIRE_TIME]))


class CommandSerializer:
    """
    The abstract base class for all command serializers.
    """

    __metaclass__ = abc.ABCMeta

    @staticmethod
    @abc.abstractmethod
    def serialize(command: Command) -> bytes:
        """
        Serialize the given command to bytes.

        :param: command: The command to serialize.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")

    @staticmethod
    @abc.abstractmethod
    def deserialize(command_bytes: bytes) -> Command:
        """
        Deserialize the given bytes to a Command.

        :param: command_bytes: The command bytes to deserialize.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")


class MsgPackCommandSerializer(CommandSerializer):
    """
    Provides a command serializer for the Message Pack specification.
    """

    @staticmethod
    def serialize(command: Command) -> bytes:
        """
        Serialize the given command to Message Pack specification bytes.

        :param: command: The command to serialize.
        :return: The serialized command.
        :raises: ValueError: If the command cannot be serialized.
        """
        if isinstance(command, OrderCommand):
            return MsgPackCommandSerializer._serialize_order_command(command)

        package = {
            COMMAND_ID: str(command.id),
            COMMAND_TIMESTAMP: _convert_datetime_to_string(command.timestamp)
        }

        if isinstance(command, CollateralInquiry):
            package[COMMAND_TYPE] = COLLATERAL_INQUIRY
            return msgpack.packb(package)

        else:
            raise ValueError("Cannot serialize command (unrecognized command).")

    @staticmethod
    def deserialize(command_bytes: bytes) -> Command:
        """
        Deserialize the given Message Pack specification bytes to a command.

        :param: command: The command to deserialize.
        :return: The deserialized command.
        :raises: ValueError: If the command cannot be deserialized.
        """
        Precondition.not_empty(command_bytes, 'command_bytes')

        unpacked = msgpack.unpackb(command_bytes, encoding=UTF8)

        command_type = unpacked[COMMAND_TYPE]
        command_id = UUID(unpacked[COMMAND_ID])
        command_timestamp = _convert_string_to_datetime(unpacked[COMMAND_TIMESTAMP])

        if command_type == ORDER_COMMAND:
            return MsgPackCommandSerializer._deserialize_order_command(
                command_id,
                command_timestamp,
                unpacked)

        if command_type == COLLATERAL_INQUIRY:
            return CollateralInquiry(
                command_id,
                command_timestamp)

        else:
            raise ValueError("Cannot deserialize command (unrecognized command).")

    @staticmethod
    def _serialize_order_command(order_command: OrderCommand) -> bytes:
        """
        Serialize the given order command to Message Pack specification bytes.

        :param order_command: The order command to serialize.
        :return: The serialized order command.
        :raises: ValueError: If the order command cannot be serialized.
        """
        package = {
            COMMAND_TYPE: ORDER_COMMAND,
            ORDER: MsgPackOrderSerializer.serialize(order_command.order).hex(),
            COMMAND_ID: str(order_command.id),
            COMMAND_TIMESTAMP: _convert_datetime_to_string(order_command.timestamp)
        }

        if isinstance(order_command, SubmitOrder):
            package[ORDER_COMMAND] = SUBMIT_ORDER
            return msgpack.packb(package)

        if isinstance(order_command, CancelOrder):
            package[ORDER_COMMAND] = CANCEL_ORDER
            package[CANCEL_REASON] = order_command.cancel_reason
            return msgpack.packb(package)

        if isinstance(order_command, ModifyOrder):
            package[ORDER_COMMAND] = MODIFY_ORDER
            package[MODIFIED_PRICE] = str(order_command.modified_price)
            return msgpack.packb(package)

        else:
            raise ValueError("Cannot serialize order command (unrecognized command).")

    @staticmethod
    def _deserialize_order_command(
            command_id: UUID,
            command_timestamp: datetime,
            unpacked: Dict) -> OrderCommand:
        """
        Deserialize the given parameters to an order command.

        :param command_id: The commands order id.
        :param command_timestamp: The commands timestamp.
        :param unpacked: The commands unpacked dictionary.
        :return: The deserialized order command.
        :raises: ValueError: If the order command cannot be deserialized.
        """
        order_command = unpacked[ORDER_COMMAND]
        order = MsgPackOrderSerializer.deserialize(bytes.fromhex(unpacked[ORDER]))

        if order_command == SUBMIT_ORDER:
            return SubmitOrder(
                order,
                command_id,
                command_timestamp)

        if order_command == CANCEL_ORDER:
            return CancelOrder(
                order,
                unpacked[CANCEL_REASON],
                command_id,
                command_timestamp)

        if order_command == MODIFY_ORDER:
            return ModifyOrder(
                order,
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
    @abc.abstractmethod
    def serialize(event: Event) -> bytes:
        """
        Serialize the given event to bytes.

        :param: event_bytes: The bytes to deserialize.
        :return: The serialized event.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")

    @staticmethod
    @abc.abstractmethod
    def deserialize(event_bytes: bytes) -> Event:
        """
        Deserialize the given bytes to an event.

        :param: event_bytes: The bytes to deserialize.
        :return: The deserialized event.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")


class MsgPackEventSerializer(EventSerializer):
    """
    Provides an event serializer for the Message Pack specification
    """

    @staticmethod
    @abc.abstractmethod
    def serialize(event: Event) -> bytes:
        """
        Serialize the event to Message Pack specification bytes.

        :param: event_bytes: The bytes to serialize.
        :return: The serialized event.
        :raises: ValueError: If the event cannot be serialized.
        """
        if isinstance(event, OrderEvent):
            return MsgPackEventSerializer._serialize_order_event(event)

        else:
            raise ValueError("Cannot serialize event (unrecognized event).")

    @staticmethod
    def deserialize(event_bytes: bytes) -> Event:
        """
        Deserialize the given Message Pack specification bytes to an event.

        :param event_bytes: The bytes to deserialize.
        :return: The deserialized event.
        :raises: ValueError: If the event cannot be deserialized.
        """
        Precondition.not_empty(event_bytes, 'event_bytes')

        unpacked = msgpack.unpackb(event_bytes, encoding=UTF8)

        event_type = unpacked[EVENT_TYPE]
        event_id = UUID(unpacked[EVENT_ID])
        event_timestamp = _convert_string_to_datetime(unpacked[EVENT_TIMESTAMP])

        if event_type == ORDER_EVENT:
            return MsgPackEventSerializer._deserialize_order_event(
                event_id,
                event_timestamp,
                unpacked)

        if event_type == ACCOUNT_EVENT:
            return AccountEvent(
                unpacked[ACCOUNT_ID],
                Broker[unpacked[BROKER]],
                unpacked[ACCOUNT_NUMBER],
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

    @staticmethod
    def _serialize_order_event(order_event: OrderEvent) -> bytes:
        """
        Serialize the given order event to Message Pack specification bytes.

        :param order_event: The order event to serialize.
        :return: The serialized order event.
        :raises: ValueError: If the order event cannot be serialized.
        """
        package = {
            EVENT_TYPE: ORDER_EVENT,
            SYMBOL: str(order_event.symbol),
            ORDER_ID: order_event.order_id,
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
            package[ORDER_ID_BROKER] = order_event.broker_order_id
            package[LABEL] = order_event.label
            package[ORDER_SIDE] = order_event.order_side.name
            package[ORDER_TYPE] = order_event.order_type.name
            package[QUANTITY] = order_event.quantity
            package[PRICE] = str(order_event.price)
            package[TIME_IN_FORCE] = order_event.time_in_force.name
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
            package[ORDER_ID_BROKER] = order_event.broker_order_id
            package[MODIFIED_TIME] = _convert_datetime_to_string(order_event.modified_time)
            package[MODIFIED_PRICE] = str(order_event.modified_price)
            return msgpack.packb(package)

        if isinstance(order_event, OrderExpired):
            package[ORDER_EVENT] = ORDER_EXPIRED
            package[EXPIRED_TIME] = _convert_datetime_to_string(order_event.expired_time)
            return msgpack.packb(package)

        if isinstance(order_event, OrderPartiallyFilled):
            package[ORDER_EVENT] = ORDER_PARTIALLY_FILLED
            package[EXECUTION_ID] = order_event.execution_id
            package[EXECUTION_TICKET] = order_event.execution_ticket
            package[ORDER_SIDE] = order_event.order_side.name
            package[FILLED_QUANTITY] = order_event.filled_quantity
            package[LEAVES_QUANTITY] = order_event.leaves_quantity
            package[AVERAGE_PRICE] = str(order_event.average_price)
            package[EXECUTION_TIME] = _convert_datetime_to_string(order_event.execution_time)
            return msgpack.packb(package)

        if isinstance(order_event, OrderFilled):
            package[ORDER_EVENT] = ORDER_FILLED
            package[EXECUTION_ID] = order_event.execution_id
            package[EXECUTION_TICKET] = order_event.execution_ticket
            package[ORDER_SIDE] = order_event.order_side.name
            package[FILLED_QUANTITY] = order_event.filled_quantity
            package[AVERAGE_PRICE] = str(order_event.average_price)
            package[EXECUTION_TIME] = _convert_datetime_to_string(order_event.execution_time)
            return msgpack.packb(package)

        else:
            raise ValueError("Cannot serialize event (unrecognized event.")

    @staticmethod
    def _deserialize_order_event(
            event_id: UUID,
            event_timestamp: datetime,
            unpacked: Dict) -> OrderEvent:
        """
        Deserialize the given parameters to an order event.

        :param event_id: The events order id.
        :param event_timestamp: The events timestamp.
        :param unpacked: The events unpacked dictionary.
        :return: The deserialized order event.
        :raises: ValueError: If the order event cannot be deserialized.
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
