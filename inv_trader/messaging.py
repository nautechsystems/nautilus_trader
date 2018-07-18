#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="messaging.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import abc
import msgpack
import iso8601
import uuid

from datetime import datetime
from decimal import Decimal
from typing import Optional

from inv_trader.model.enums import Venue, OrderSide, OrderType, TimeInForce
from inv_trader.model.objects import Symbol
from inv_trader.model.events import OrderEvent
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled

UTF8 = 'utf-8'
EVENT_TYPE = 'event_type'.encode(UTF8)
SYMBOL = 'symbol'.encode(UTF8)
ORDER_ID = 'order_id'.encode(UTF8)
ORDER_ID_BROKER = 'order_id_broker'.encode(UTF8)
EVENT_ID = 'event_id'.encode(UTF8)
EVENT_TIMESTAMP = 'event_timestamp'.encode(UTF8)
LABEL = 'label'.encode(UTF8)
ORDER_SUBMITTED = 'order_submitted'.encode(UTF8)
ORDER_ACCEPTED = 'order_accepted'.encode(UTF8)
ORDER_REJECTED = 'order_rejected'.encode(UTF8)
ORDER_WORKING = 'order_working'.encode(UTF8)
ORDER_CANCELLED = 'order_cancelled'.encode(UTF8)
ORDER_CANCEL_REJECT = 'order_cancel_reject'.encode(UTF8)
ORDER_MODIFIED = 'order_modified'.encode(UTF8)
ORDER_EXPIRED = 'order_expired'.encode(UTF8)
ORDER_PARTIALLY_FILLED = 'order_partially_filled'.encode(UTF8)
ORDER_FILLED = 'order_filled'.encode(UTF8)
SUBMITTED_TIME = 'submitted_time'.encode(UTF8)
ACCEPTED_TIME = 'accepted_time'.encode(UTF8)
REJECTED_TIME = 'rejected_time'.encode(UTF8)
REJECTED_RESPONSE = 'rejected_response'.encode(UTF8)
REJECTED_REASON = 'rejected_reason'.encode(UTF8)
WORKING_TIME = 'working_time'.encode(UTF8)
CANCELLED_TIME = 'cancelled_time'.encode(UTF8)
MODIFIED_TIME = 'modified_time'.encode(UTF8)
EXPIRE_TIME = 'expire_time'.encode(UTF8)
EXPIRED_TIME = 'expired_time'.encode(UTF8)
MODIFIED_PRICE = 'modified_price'
EXECUTION_TIME = 'execution_time'.encode(UTF8)
EXECUTION_ID = 'execution_id'.encode(UTF8)
EXECUTION_TICKET = 'execution_ticket'.encode(UTF8)
ORDER_SIDE = 'order_side'.encode(UTF8)
ORDER_TYPE = 'order_type'.encode(UTF8)
FILLED_QUANTITY = 'filled_quantity'.encode(UTF8)
LEAVES_QUANTITY = 'leaves_quantity'.encode(UTF8)
QUANTITY = 'quantity'.encode(UTF8)
AVERAGE_PRICE = 'average_price'.encode(UTF8)
PRICE = 'price'.encode(UTF8)
TIME_IN_FORCE = 'time_in_force'.encode(UTF8)


class EventSerializer:
    """
    The abstract base class for all event serializers.
    """

    __metaclass__ = abc.ABCMeta

    @staticmethod
    @abc.abstractmethod
    def deserialize_order_event(event_bytes: bytearray) -> OrderEvent:
        """
        Deserialize the given bytes to an order event.

        :param: event_bytes: The byte array to deserialize.
        :return: The deserialized order event.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")


class MsgPackEventSerializer(EventSerializer):
    """
    Provides a serializer for the Message Pack specification
    """

    @staticmethod
    def deserialize_order_event(event_bytes: bytearray) -> OrderEvent:
        """
        Deserialize the given Message Pack bytes to an order event.

        :param event_bytes: The byte array to deserialize.
        :return: The deserialized order event.
        """
        unpacked = msgpack.unpackb(event_bytes)

        event_type = unpacked[EVENT_TYPE]
        split_symbol = unpacked[SYMBOL].decode(UTF8).split('.')
        symbol = Symbol(split_symbol[0], Venue[split_symbol[1].upper()])
        order_id = unpacked[ORDER_ID].decode(UTF8)
        event_id = unpacked[EVENT_ID].decode(UTF8)
        event_timestamp = iso8601.parse_date(unpacked[EVENT_TIMESTAMP].decode(UTF8))

        if event_type == ORDER_SUBMITTED:
            return OrderSubmitted(
                symbol,
                order_id,
                iso8601.parse_date(unpacked[SUBMITTED_TIME].decode(UTF8)),
                uuid.UUID(event_id),
                event_timestamp)

        elif event_type == ORDER_ACCEPTED:
            return OrderAccepted(
                symbol,
                order_id,
                iso8601.parse_date(unpacked[ACCEPTED_TIME].decode(UTF8)),
                uuid.UUID(event_id),
                event_timestamp)

        elif event_type == ORDER_REJECTED:
            return OrderRejected(
                symbol,
                order_id,
                iso8601.parse_date(unpacked[REJECTED_TIME].decode(UTF8)),
                unpacked[REJECTED_REASON].decode(UTF8),
                uuid.UUID(event_id),
                event_timestamp)

        elif event_type == ORDER_WORKING:
            expire_time_string = unpacked[EXPIRE_TIME].decode(UTF8)
            if expire_time_string == 'none':
                expire_time = None
            else:
                expire_time = iso8601.parse_date(expire_time_string),

            return OrderWorking(
                symbol,
                order_id,
                unpacked[ORDER_ID_BROKER].decode(UTF8),
                unpacked[LABEL].decode(UTF8),
                OrderSide[unpacked[ORDER_SIDE].decode(UTF8)],
                OrderType[unpacked[ORDER_TYPE].decode(UTF8)],
                unpacked[QUANTITY],
                Decimal(unpacked[PRICE].decode(UTF8)),
                TimeInForce[unpacked[TIME_IN_FORCE].decode(UTF8)],
                iso8601.parse_date(unpacked[WORKING_TIME].decode(UTF8)),
                uuid.UUID(event_id),
                event_timestamp,
                expire_time)

        elif event_type == ORDER_CANCELLED:
            return OrderCancelled(
                symbol,
                order_id,
                iso8601.parse_date(unpacked[CANCELLED_TIME].decode(UTF8)),
                uuid.UUID(event_id),
                event_timestamp)

        elif event_type == ORDER_CANCEL_REJECT:
            return OrderCancelReject(
                symbol,
                order_id,
                iso8601.parse_date(unpacked[REJECTED_TIME].decode(UTF8)),
                unpacked[REJECTED_RESPONSE].decode(UTF8),
                unpacked[REJECTED_REASON].decode(UTF8),
                uuid.UUID(event_id),
                event_timestamp)

        elif event_type == ORDER_MODIFIED:
            return OrderModified(
                symbol,
                order_id,
                unpacked[ORDER_ID_BROKER].decode(UTF8),
                Decimal(unpacked[ORDER_ID_BROKER].decode(UTF8)),
                iso8601.parse_date(unpacked[MODIFIED_TIME].decode(UTF8)),
                uuid.UUID(event_id),
                event_timestamp)

        elif event_type == ORDER_EXPIRED:
            return OrderExpired(
                symbol,
                order_id,
                iso8601.parse_date(unpacked[EXPIRED_TIME].decode(UTF8)),
                uuid.UUID(event_id),
                event_timestamp)

        elif event_type == ORDER_PARTIALLY_FILLED:
            return OrderPartiallyFilled(
                symbol,
                order_id,
                unpacked[EXECUTION_ID].decode(UTF8),
                unpacked[EXECUTION_TICKET].decode(UTF8),
                OrderSide[unpacked[ORDER_SIDE].decode(UTF8).upper()],
                int(unpacked[FILLED_QUANTITY]),
                int(unpacked[LEAVES_QUANTITY]),
                Decimal(unpacked[AVERAGE_PRICE]),
                iso8601.parse_date(unpacked[EXECUTION_TIME].decode(UTF8)),
                uuid.UUID(event_id),
                event_timestamp)

        elif event_type == ORDER_FILLED:
            return OrderFilled(
                symbol,
                order_id,
                unpacked[EXECUTION_ID].decode(UTF8),
                unpacked[EXECUTION_TICKET].decode(UTF8),
                OrderSide[unpacked[ORDER_SIDE].decode(UTF8).upper()],
                int(unpacked[FILLED_QUANTITY]),
                Decimal(unpacked[AVERAGE_PRICE]),
                iso8601.parse_date(unpacked[EXECUTION_TIME].decode(UTF8)),
                uuid.UUID(event_id),
                event_timestamp)

        else:
            raise ValueError("The order event is invalid and cannot be parsed.")
