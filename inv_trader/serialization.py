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
import uuid

from decimal import Decimal

from inv_trader.model.enums import Venue, OrderSide, OrderType, TimeInForce
from inv_trader.model.objects import Symbol
from inv_trader.model.events import OrderEvent
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled
from inv_trader.model.order import Order
from inv_trader.model.commands import OrderCommand, SubmitOrder

# Constants
UTF8 = 'utf-8'
COMMAND_TYPE = 'command_type'
SUBMIT_ORDER = 'submit_order'
CANCEL_ORDER = 'cancel_order'
MODIFY_ORDER = 'modify_order'
ORDER = 'order'
TIMESTAMP = 'timestamp'
EVENT_TYPE = 'event_type'
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


class EventSerializer:
    """
    The abstract base class for all event serializers.
    """

    __metaclass__ = abc.ABCMeta

    @staticmethod
    @abc.abstractmethod
    def deserialize(event_bytes: bytearray) -> OrderEvent:
        """
        Deserialize the given bytes to an order event.

        :param: event_bytes: The byte array to deserialize.
        :return: The deserialized order event.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")


class MsgPackEventSerializer(EventSerializer):
    """
    Provides an event serializer for the Message Pack specification
    """

    @staticmethod
    def deserialize(event_bytes: bytearray) -> OrderEvent:
        """
        Deserialize the given Message Pack bytes to an order event.

        :param event_bytes: The byte array to deserialize.
        :return: The deserialized order event.
        """
        unpacked = msgpack.unpackb(event_bytes, encoding=UTF8)

        event_type = unpacked[EVENT_TYPE]
        split_symbol = unpacked[SYMBOL].split('.')
        symbol = Symbol(split_symbol[0], Venue[split_symbol[1].upper()])
        order_id = unpacked[ORDER_ID]
        event_id = unpacked[EVENT_ID]
        event_timestamp = iso8601.parse_date(unpacked[EVENT_TIMESTAMP])

        if event_type == ORDER_SUBMITTED:
            return OrderSubmitted(
                symbol,
                order_id,
                iso8601.parse_date(unpacked[SUBMITTED_TIME]),
                uuid.UUID(event_id),
                event_timestamp)

        elif event_type == ORDER_ACCEPTED:
            return OrderAccepted(
                symbol,
                order_id,
                iso8601.parse_date(unpacked[ACCEPTED_TIME]),
                uuid.UUID(event_id),
                event_timestamp)

        elif event_type == ORDER_REJECTED:
            return OrderRejected(
                symbol,
                order_id,
                iso8601.parse_date(unpacked[REJECTED_TIME]),
                unpacked[REJECTED_REASON],
                uuid.UUID(event_id),
                event_timestamp)

        elif event_type == ORDER_WORKING:
            expire_time_string = unpacked[EXPIRE_TIME]
            if expire_time_string == 'none':
                expire_time = None
            else:
                expire_time = iso8601.parse_date(expire_time_string),

            return OrderWorking(
                symbol,
                order_id,
                unpacked[ORDER_ID_BROKER],
                unpacked[LABEL],
                OrderSide[unpacked[ORDER_SIDE]],
                OrderType[unpacked[ORDER_TYPE]],
                unpacked[QUANTITY],
                Decimal(unpacked[PRICE]),
                TimeInForce[unpacked[TIME_IN_FORCE]],
                iso8601.parse_date(unpacked[WORKING_TIME]),
                uuid.UUID(event_id),
                event_timestamp,
                expire_time)

        elif event_type == ORDER_CANCELLED:
            return OrderCancelled(
                symbol,
                order_id,
                iso8601.parse_date(unpacked[CANCELLED_TIME]),
                uuid.UUID(event_id),
                event_timestamp)

        elif event_type == ORDER_CANCEL_REJECT:
            return OrderCancelReject(
                symbol,
                order_id,
                iso8601.parse_date(unpacked[REJECTED_TIME]),
                unpacked[REJECTED_RESPONSE],
                unpacked[REJECTED_REASON],
                uuid.UUID(event_id),
                event_timestamp)

        elif event_type == ORDER_MODIFIED:
            return OrderModified(
                symbol,
                order_id,
                unpacked[ORDER_ID_BROKER],
                Decimal(unpacked[MODIFIED_PRICE]),
                iso8601.parse_date(unpacked[MODIFIED_TIME]),
                uuid.UUID(event_id),
                event_timestamp)

        elif event_type == ORDER_EXPIRED:
            return OrderExpired(
                symbol,
                order_id,
                iso8601.parse_date(unpacked[EXPIRED_TIME]),
                uuid.UUID(event_id),
                event_timestamp)

        elif event_type == ORDER_PARTIALLY_FILLED:
            return OrderPartiallyFilled(
                symbol,
                order_id,
                unpacked[EXECUTION_ID],
                unpacked[EXECUTION_TICKET],
                OrderSide[unpacked[ORDER_SIDE].upper()],
                int(unpacked[FILLED_QUANTITY]),
                int(unpacked[LEAVES_QUANTITY]),
                Decimal(unpacked[AVERAGE_PRICE]),
                iso8601.parse_date(unpacked[EXECUTION_TIME]),
                uuid.UUID(event_id),
                event_timestamp)

        elif event_type == ORDER_FILLED:
            return OrderFilled(
                symbol,
                order_id,
                unpacked[EXECUTION_ID],
                unpacked[EXECUTION_TICKET],
                OrderSide[unpacked[ORDER_SIDE].upper()],
                int(unpacked[FILLED_QUANTITY]),
                Decimal(unpacked[AVERAGE_PRICE]),
                iso8601.parse_date(unpacked[EXECUTION_TIME]),
                uuid.UUID(event_id),
                event_timestamp)

        else:
            raise ValueError("The order event is invalid and cannot be deserialized.")


class CommandSerializer:
    """
    The abstract base class for all command serializers.
    """

    __metaclass__ = abc.ABCMeta

    @staticmethod
    @abc.abstractmethod
    def serialize(order_command: OrderCommand) -> bytearray:
        """
        Serialize the given order command to a bytes array to be sent.

        :param: order_command: The order command to serialize.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")


class MsgPackCommandSerializer(CommandSerializer):
    """
    Provides a command serializer for the Message Pack specification
    """

    @staticmethod
    def serialize(order_command: OrderCommand) -> bytearray:
        """
        Serialize the given OrderCommand to Message Pack bytes.

        :param: order_command: The order command to serialize.
        """
        if isinstance(order_command, SubmitOrder):
            return msgpack.packb({
                COMMAND_TYPE: SUBMIT_ORDER,
                ORDER_ID: order_command.order_id,
                ORDER: MsgPackOrderSerializer.serialize(order_command.order)
            })

        else:
            raise ValueError("The order command is invalid and cannot be serialized.")


class OrderSerializer:
    """
    The abstract base class for all order serializers.
    """

    __metaclass__ = abc.ABCMeta

    @staticmethod
    @abc.abstractmethod
    def serialize(order_command: OrderCommand) -> bytearray:
        """
        Serialize the given order to a bytes array.

        :param: order: The order to serialize.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")

    @staticmethod
    @abc.abstractmethod
    def deserialize(order_bytes: bytearray) -> Order:
        """
        Deserialize the given byte array to an Order.

        :param: order_bytes: The byte array to deserialize.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")


class MsgPackOrderSerializer(CommandSerializer):
    """
    Provides a command serializer for the Message Pack specification
    """

    @staticmethod
    def serialize(order: Order) -> bytearray:
        """
        Serialize the given Order to Message Pack bytes.

        :param: order: The order to serialize.
        """
        # Convert price to string (could be None).
        if order.price is None:
            price = 'none'
        else:
            price = str(order.price)

        # Convert expire time to string (could be None).
        if order.expire_time is None:
            expire_time = 'none'
        else:
            expire_time = iso8601.parse_date(order.expire_time)

        return msgpack.packb({
            SYMBOL: str(order.symbol),
            ORDER_ID: order.id,
            LABEL: order.label,
            ORDER_SIDE: order.side.name,
            ORDER_TYPE: order.type.name,
            QUANTITY: order.quantity,
            TIMESTAMP: order.timestamp.isoformat(timespec='milliseconds').replace('+00:00', 'Z'),
            PRICE: price,
            TIME_IN_FORCE: order.time_in_force.name,
            EXPIRE_TIME: expire_time
            })

    @staticmethod
    def deserialize(order_bytes: bytearray) -> Order:
        """
        Deserialize the given byte array to an Order.

        :param: order_bytes: The byte array to deserialize.
        """
        unpacked = msgpack.unpackb(order_bytes, encoding=UTF8)

        # Deserialize symbol
        split_symbol = unpacked[SYMBOL].split('.')
        symbol = Symbol(split_symbol[0], Venue[split_symbol[1].upper()])

        # Deserialize price (could be 'none').
        price = unpacked[PRICE]
        if price == 'none':
            price = None
        else:
            price = Decimal(unpacked[PRICE])

        # Deserialize expire_time (could be 'none').
        expire_time = unpacked[EXPIRE_TIME]
        if expire_time == 'none':
            expire_time = None
        else:
            expire_time = iso8601.parse_date(unpacked[EXPIRE_TIME])

        return Order(symbol=symbol,
                     order_id=unpacked[ORDER_ID],
                     label=unpacked[LABEL],
                     order_side=OrderSide[unpacked[ORDER_SIDE]],
                     order_type=OrderType[unpacked[ORDER_TYPE]],
                     quantity=unpacked[QUANTITY],
                     timestamp=iso8601.parse_date(unpacked[TIMESTAMP]),
                     price=price,
                     time_in_force=TimeInForce[unpacked[TIME_IN_FORCE]],
                     expire_time=expire_time)
