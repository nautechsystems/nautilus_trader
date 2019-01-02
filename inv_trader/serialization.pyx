#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="serialization.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

import ast
import msgpack
import iso8601
import json

from cpython.datetime cimport datetime
from uuid import UUID
from datetime import datetime
from decimal import Decimal
from typing import Dict

from inv_trader.core.precondition cimport Precondition
from inv_trader.commands cimport Command, OrderCommand, SubmitOrder, CancelOrder, ModifyOrder
from inv_trader.commands cimport CollateralInquiry
from inv_trader.model.enums import Broker, Venue, OrderSide, OrderType, TimeInForce, CurrencyCode
from inv_trader.enums.brokerage cimport Broker, broker_string
from inv_trader.enums.time_in_force cimport TimeInForce, time_in_force_string
from inv_trader.enums.order_side cimport OrderSide, order_side_string
from inv_trader.enums.order_type cimport OrderType, order_type_string
from inv_trader.enums.venue cimport Venue, venue_string
from inv_trader.enums.security_type cimport SecurityType
from inv_trader.enums.currency_code cimport CurrencyCode
from inv_trader.model.identifiers cimport GUID, Label, OrderId, ExecutionId, ExecutionTicket, AccountId, AccountNumber
from inv_trader.model.objects cimport Symbol, Instrument
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport Event, OrderEvent, AccountEvent
from inv_trader.model.events cimport OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events cimport OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events cimport OrderPartiallyFilled, OrderFilled


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

cpdef object _parse_symbol(str symbol_string):
    """
    Parse the given string to a Symbol.

    :param symbol_string: The symbol string to parse.
    :return: The parsed symbol.
    """
    split_symbol = symbol_string.split('.')
    return Symbol(split_symbol[0], Venue[split_symbol[1].upper()])


cpdef str _convert_price_to_string(price: Decimal):
    """
    Convert the given object to a decimal or 'NONE' string.

    :param price: The price to convert.
    :return: The converted string.
    """
    return NONE if price is None else str(price)


cpdef object _convert_string_to_price(str price_string):
    """
    Convert the given price string to a Decimal or None.

    :param price_string: The price string to convert.
    :return: The converted price, or None.
    """
    return None if price_string == NONE else Decimal(price_string)


cpdef str _convert_datetime_to_string(datetime expire_time):
    """
    Convert the given object to a valid ISO8601 string, or 'NONE'.

    :param expire_time: The datetime string to convert
    :return: The converted string.
    """
    return (NONE if expire_time is None
            else expire_time.isoformat(timespec='milliseconds').replace('+00:00', 'Z'))


cpdef object _convert_string_to_datetime(str expire_time_string):
    """
    Convert the given string to a datetime object, or None.

    :param expire_time_string: The string to convert.
    :return: The converted datetime, or None.
    """
    return None if expire_time_string == NONE else iso8601.parse_date(expire_time_string)


cdef class OrderSerializer:
    """
    The abstract base class for all order serializers.
    """

    @staticmethod
    def serialize(Order order):
        """
        Serialize the given order to bytes.

        :param order: The order to serialize.
        :return: The serialized order.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")

    @staticmethod
    def deserialize(bytes order_bytes):
        """
        Deserialize the given bytes to an Order.

        :param order_bytes: The bytes to deserialize.
        :return: The deserialized order.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")


cdef class MsgPackOrderSerializer(OrderSerializer):
    """
    Provides a command serializer for the Message Pack specification
    """

    @staticmethod
    def serialize(Order order):
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

    @staticmethod
    def deserialize(bytes order_bytes):
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


cdef class CommandSerializer:
    """
    The abstract base class for all command serializers.
    """

    @staticmethod
    def serialize(command: Command) -> bytes:
        """
        Serialize the given command to bytes.

        :param: command: The command to serialize.
        :return: The serialized command.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")

    @staticmethod
    def deserialize(command_bytes: bytes) -> Command:
        """
        Deserialize the given bytes to a Command.

        :param: command_bytes: The command bytes to deserialize.
        :return: The deserialized command.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")


cdef class MsgPackCommandSerializer(CommandSerializer):
    """
    Provides a command serializer for the Message Pack specification.
    """

    @staticmethod
    def serialize(Command command) -> bytes:
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
    def deserialize(bytes command_bytes) -> Command:
        """
        Deserialize the given Message Pack specification bytes to a command.

        :param command_bytes: The command to deserialize.
        :return: The deserialized command.
        :raises ValueError: If the command_bytes is empty.
        :raises ValueError: If the command cannot be deserialized.
        """
        Precondition.not_empty(command_bytes, 'command_bytes')

        unpacked = msgpack.unpackb(command_bytes, raw=False)

        command_type = unpacked[COMMAND_TYPE]
        command_id = GUID(UUID(unpacked[COMMAND_ID]))
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
    def _serialize_order_command(OrderCommand order_command) -> bytes:
        """
        Serialize the given order command to Message Pack specification bytes.

        :param order_command: The order command to serialize.
        :return: The serialized order command.
        :raises ValueError: If the order command cannot be serialized.
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
            GUID command_id,
            datetime command_timestamp,
            unpacked: Dict) -> OrderCommand:
        """
        Deserialize the given parameters to an order command.

        :param command_id: The commands order id.
        :param command_timestamp: The commands timestamp.
        :param unpacked: The commands unpacked dictionary.
        :return: The deserialized order command.
        :raises ValueError: If the order command cannot be deserialized.
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


cdef class EventSerializer:
    """
    The abstract base class for all event serializers.
    """

    @staticmethod
    def serialize(Event event) -> bytes:
        """
        Serialize the given event to bytes.

        :param event: The event to serialize.
        :return: The serialized event.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")

    @staticmethod
    def deserialize(bytes event_bytes) -> Event:
        """
        Deserialize the given bytes to an event.

        :param event_bytes: The bytes to deserialize.
        :return: The deserialized event.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")


cdef class MsgPackEventSerializer(EventSerializer):
    """
    Provides an event serializer for the Message Pack specification
    """

    @staticmethod
    def serialize(Event event) -> bytes:
        """
        Serialize the event to Message Pack specification bytes.

        :param event: The event to serialize.
        :return: The serialized event.
        :raises: ValueError: If the event cannot be serialized.
        """
        if isinstance(event, OrderEvent):
            return MsgPackEventSerializer._serialize_order_event(event)

        else:
            raise ValueError("Cannot serialize event (unrecognized event).")

    @staticmethod
    def deserialize(bytes event_bytes) -> Event:
        """
        Deserialize the given Message Pack specification bytes to an event.

        :param event_bytes: The bytes to deserialize.
        :return: The deserialized event.
        :raises ValueError: If the event_bytes is empty.
        :raises ValueError: If the event cannot be deserialized.
        """
        Precondition.not_empty(event_bytes, 'event_bytes')

        unpacked = msgpack.unpackb(event_bytes, raw=False)

        event_type = unpacked[EVENT_TYPE]
        event_id = GUID(UUID(unpacked[EVENT_ID]))
        event_timestamp = _convert_string_to_datetime(unpacked[EVENT_TIMESTAMP])

        if event_type == ORDER_EVENT:
            return MsgPackEventSerializer._deserialize_order_event(
                event_id,
                event_timestamp,
                unpacked)

        if event_type == ACCOUNT_EVENT:
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

    @staticmethod
    def _serialize_order_event(OrderEvent order_event) -> bytes:
        """
        Serialize the given order event to Message Pack specification bytes.

        :param order_event: The order event to serialize.
        :return: The serialized order event.
        :raises ValueError: If the order event cannot be serialized.
        """
        package = {
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

    @staticmethod
    cdef object _deserialize_order_event(
            GUID event_id,
            datetime event_timestamp,
            unpacked: Dict):
        """
        Deserialize the given parameters to an order event.

        :param event_id: The events order id.
        :param event_timestamp: The events timestamp.
        :param unpacked: The events unpacked dictionary.
        :return: The deserialized order event.
        :raises ValueError: If the order event cannot be deserialized.
        """
        order_symbol = _parse_symbol(unpacked[SYMBOL])
        order_id = OrderId(unpacked[ORDER_ID])
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
                OrderId(unpacked[ORDER_ID_BROKER]),
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
                ExecutionId(unpacked[EXECUTION_ID]),
                ExecutionTicket(unpacked[EXECUTION_TICKET]),
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


cdef class InstrumentSerializer:
    """
    Provides an instrument deserializer.
    """

    @staticmethod
    def deserialize(bytes instrument_bytes) -> Instrument:
        """
        Deserialize the given instrument bytes to an instrument.

        :param instrument_bytes: The string to deserialize.
        :return: The deserialized instrument.
        :raises ValueError: If the instrument_bytes is empty.
        :raises ValueError: If the instrument cannot be deserialized.
        """
        inst_json = (json.loads(instrument_bytes)
                     .replace("\"", "\'")
                     .replace("\'Timestamp\':", "\'Timestamp\':\'")[:-1] + "\'}")
        inst_dict = ast.literal_eval(inst_json)

        tick_size = inst_dict['TickSize']
        tick_value = inst_dict['TickValue']
        target_direct_spread = inst_dict['TargetDirectSpread']
        margin_requirement = inst_dict['MarginRequirement']
        rollover_interest_buy = inst_dict['RolloverInterestBuy']
        rollover_interest_sell = inst_dict['RolloverInterestSell']

        return Instrument(
            Symbol(inst_dict['Symbol']['Code'], Venue[inst_dict['Symbol']['Venue'].upper()]),
            inst_dict['BrokerSymbol']['Value'],
            CurrencyCode[inst_dict['QuoteCurrency'].upper()],
            SecurityType[inst_dict['SecurityType'].upper()],
            inst_dict['TickDecimals'],
            Decimal(f'{tick_size}'),
            Decimal(f'{tick_value}'),
            Decimal(f'{target_direct_spread}'),
            inst_dict['RoundLotSize'],
            inst_dict['ContractSize'],
            inst_dict['MinStopDistanceEntry'],
            inst_dict['MinLimitDistanceEntry'],
            inst_dict['MinStopDistance'],
            inst_dict['MinLimitDistance'],
            inst_dict['MinTradeSize'],
            inst_dict['MaxTradeSize'],
            Decimal(f'{margin_requirement}'),
            Decimal(f'{rollover_interest_buy}'),
            Decimal(f'{rollover_interest_sell}'),
            iso8601.parse_date(inst_dict['Timestamp']))
