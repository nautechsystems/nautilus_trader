#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="serialization.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

import msgpack

from decimal import Decimal
from uuid import UUID

from inv_trader.core.precondition cimport Precondition
from inv_trader.commands cimport *
from inv_trader.commands cimport CollateralInquiry
from inv_trader.model.enums import Broker, OrderSide, OrderType, TimeInForce, Currency
from inv_trader.enums.brokerage cimport Broker, broker_string
from inv_trader.enums.time_in_force cimport TimeInForce, time_in_force_string
from inv_trader.enums.order_side cimport OrderSide, order_side_string
from inv_trader.enums.order_type cimport OrderType, order_type_string
from inv_trader.enums.currency cimport Currency, currency_string
from inv_trader.model.identifiers cimport GUID, Label, TraderId, StrategyId, OrderId, ExecutionId, ExecutionTicket, AccountId, AccountNumber
from inv_trader.model.objects cimport ValidString, Quantity, Symbol, Price, Money, Instrument
from inv_trader.model.order cimport Order, AtomicOrder
from inv_trader.model.events cimport Event, AccountEvent
from inv_trader.model.events cimport OrderInitialized, OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events cimport OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events cimport OrderPartiallyFilled, OrderFilled
from inv_trader.common.serialization cimport (
parse_symbol,
convert_price_to_string,
convert_string_to_price,
convert_label_to_string,
convert_string_to_label,
convert_datetime_to_string,
convert_string_to_datetime)
from inv_trader.common.serialization cimport OrderSerializer, EventSerializer, CommandSerializer, InstrumentSerializer


cdef str UTF8 = 'utf-8'
cdef str NONE = 'NONE'
cdef str TYPE = 'Type'
cdef str COMMAND = 'Command'
cdef str COMMAND_ID = 'CommandId'
cdef str COMMAND_TIMESTAMP = 'CommandTimestamp'
cdef str EVENT = 'Event'
cdef str EVENT_ID = 'EventId'
cdef str EVENT_TIMESTAMP = 'EventTimestamp'
cdef str COLLATERAL_INQUIRY = 'CollateralInquiry'
cdef str SUBMIT_ORDER = 'SubmitOrder'
cdef str SUBMIT_ATOMIC_ORDER = 'SubmitAtomicOrder'
cdef str CANCEL_ORDER = 'CancelOrder'
cdef str MODIFY_ORDER = 'ModifyOrder'
cdef str CANCEL_REASON = 'CancelReason'
cdef str ORDER = 'Order'
cdef str TIMESTAMP = 'Timestamp'
cdef str SYMBOL = 'Symbol'
cdef str ORDER_ID = 'OrderId'
cdef str ORDER_ID_BROKER = 'OrderIdBroker'
cdef str TRADER_ID = 'TraderId'
cdef str STRATEGY_ID = 'StrategyId'
cdef str POSITION_ID = 'PositionId'
cdef str LABEL = 'Label'
cdef str SUBMITTED_TIME = 'SubmittedTime'
cdef str ACCEPTED_TIME = 'AcceptedTime'
cdef str REJECTED_TIME = 'RejectedTime'
cdef str REJECTED_RESPONSE = 'RejectedResponse'
cdef str REJECTED_REASON = 'RejectedReason'
cdef str WORKING_TIME = 'WorkingTime'
cdef str CANCELLED_TIME = 'CancelledTime'
cdef str MODIFIED_TIME = 'ModifiedTime'
cdef str MODIFIED_PRICE = 'ModifiedPrice'
cdef str EXPIRE_TIME = 'ExpireTime'
cdef str EXPIRED_TIME = 'ExpiredTime'
cdef str EXECUTION_TIME = 'ExecutionTime'
cdef str EXECUTION_ID = 'ExecutionId'
cdef str EXECUTION_TICKET = 'ExecutionTicket'
cdef str ORDER_SIDE = 'OrderSide'
cdef str ORDER_TYPE = 'OrderType'
cdef str ENTRY = 'Entry'
cdef str STOP_LOSS = 'StopLoss'
cdef str TAKE_PROFIT = 'TakeProfit'
cdef str FILLED_QUANTITY = 'FilledQuantity'
cdef str LEAVES_QUANTITY = 'LeavesQuantity'
cdef str QUANTITY = 'Quantity'
cdef str AVERAGE_PRICE = 'AveragePrice'
cdef str PRICE = 'Price'
cdef str TIME_IN_FORCE = 'TimeInForce'
cdef str ACCOUNT_ID = 'AccountId'
cdef str ACCOUNT_NUMBER = 'AccountNumber'
cdef str BROKER = 'Broker'
cdef str CURRENCY = 'Currency'
cdef str CASH_BALANCE = 'CashBalance'
cdef str CASH_START_DAY = 'CashStartDay'
cdef str CASH_ACTIVITY_DAY = 'CashActivityDay'
cdef str MARGIN_USED_LIQUIDATION = 'MarginUsedLiquidation'
cdef str MARGIN_USED_MAINTENANCE = 'MarginUsedMaintenance'
cdef str MARGIN_RATIO = 'MarginRatio'
cdef str MARGIN_CALL_STATUS = 'MarginCallStatus'


cdef class MsgPackOrderSerializer(OrderSerializer):
    """
    Provides a command serializer for the MessagePack specification
    """

    cpdef bytes serialize(self, Order order):
        """
        Serialize the given order to MessagePack specification bytes.

        :param order: The order to serialize.
        :return: bytes.
        """
        if order is None:
            return msgpack.packb({})  # Null order

        return msgpack.packb({
            ORDER_ID: order.id.value,
            SYMBOL: str(order.symbol),
            ORDER_SIDE: order_side_string(order.side),
            ORDER_TYPE: order_type_string(order.type),
            QUANTITY: order.quantity.value,
            TIMESTAMP: convert_datetime_to_string(order.timestamp),
            PRICE: convert_price_to_string(order.price),
            LABEL: convert_label_to_string(order.label),
            TIME_IN_FORCE: time_in_force_string(order.time_in_force),
            EXPIRE_TIME: convert_datetime_to_string(order.expire_time)
            })

    cpdef Order deserialize(self, bytes order_bytes):
        """
        Deserialize the given MessagePack specification bytes to an order.

        :param order_bytes: The bytes to deserialize.
        :return: Order.
        :raises ValueError: If the event_bytes is empty.
        """
        Precondition.not_empty(order_bytes, 'order_bytes')

        cdef dict unpacked = msgpack.unpackb(order_bytes, raw=False)

        if len(unpacked) == 0:
            return None  # Null order

        return Order(order_id=OrderId(unpacked[ORDER_ID]),
                     symbol=parse_symbol(unpacked[SYMBOL]),
                     order_side=OrderSide[unpacked[ORDER_SIDE]],
                     order_type=OrderType[unpacked[ORDER_TYPE]],
                     quantity=Quantity(unpacked[QUANTITY]),
                     timestamp=convert_string_to_datetime(unpacked[TIMESTAMP]),
                     price=convert_string_to_price(unpacked[PRICE]),
                     label=convert_string_to_label(unpacked[LABEL]),
                     time_in_force=TimeInForce[unpacked[TIME_IN_FORCE]],
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
        Serialize the given command to MessagePackk specification bytes.

        :param: command: The command to serialize.
        :return: bytes.
        :raises: ValueError: If the command cannot be serialized.
        """
        cdef dict package = {
            TYPE: COMMAND,
            COMMAND_ID: command.id.value,
            COMMAND_TIMESTAMP: convert_datetime_to_string(command.timestamp)
        }

        if isinstance(command, CollateralInquiry):
            package[COMMAND] = COLLATERAL_INQUIRY
        elif isinstance(command, SubmitOrder):
            package[COMMAND] = SUBMIT_ORDER
            package[TRADER_ID] = command.trader_id.value
            package[STRATEGY_ID] = command.strategy_id.value
            package[POSITION_ID] = command.position_id.value
            package[ORDER] = self.order_serializer.serialize(command.order)
            return msgpack.packb(package)
        elif isinstance(command, SubmitAtomicOrder):
            package[COMMAND] = SUBMIT_ATOMIC_ORDER
            package[TRADER_ID] = command.trader_id.value
            package[STRATEGY_ID] = command.strategy_id.value
            package[POSITION_ID] = command.position_id.value
            package[ENTRY] = self.order_serializer.serialize(command.atomic_order.entry)
            package[STOP_LOSS] = self.order_serializer.serialize(command.atomic_order.stop_loss)
            package[TAKE_PROFIT] = self.order_serializer.serialize(command.atomic_order.take_profit)
            return msgpack.packb(package)
        elif isinstance(command, ModifyOrder):
            package[COMMAND] = MODIFY_ORDER
            package[TRADER_ID] = command.trader_id.value
            package[STRATEGY_ID] = command.strategy_id.value
            package[ORDER_ID] = command.order_id.value
            package[MODIFIED_PRICE] = str(command.modified_price)
            return msgpack.packb(package)
        elif isinstance(command, CancelOrder):
            package[COMMAND] = CANCEL_ORDER
            package[TRADER_ID] = command.trader_id.value
            package[STRATEGY_ID] = command.strategy_id.value
            package[ORDER_ID] = command.order_id.value
            package[CANCEL_REASON] = command.cancel_reason.value
            return msgpack.packb(package)
        else:
            raise ValueError("Cannot serialize command (unrecognized command).")


    cpdef Command deserialize(self, bytes command_bytes):
        """
        Deserialize the given MessagePack specification bytes to a command.

        :param command_bytes: The command to deserialize.
        :return: Command.
        :raises ValueError: If the command_bytes is empty.
        :raises ValueError: If the command cannot be deserialized.
        """
        Precondition.not_empty(command_bytes, 'command_bytes')

        cdef dict unpacked_raw = msgpack.unpackb(command_bytes)
        cdef dict unpacked = {}

        cdef str message_type = unpacked_raw[b'Type'].decode(UTF8)
        if message_type != COMMAND:
            raise ValueError("Cannot deserialize command (the message is not a type of command).")

        # Manually unpack and decode
        for k, v in unpacked_raw.items():
            if k not in (b'Order', b'Entry', b'StopLoss', b'TakeProfit'):
                if isinstance(v, bytes):
                    unpacked[k.decode(UTF8)] = v.decode(UTF8)
                else:
                    unpacked[k.decode(UTF8)] = v
            else:
                unpacked[k.decode(UTF8)] = v

        cdef str command = unpacked[COMMAND]
        cdef GUID command_id = GUID(UUID(unpacked[COMMAND_ID]))
        cdef datetime command_timestamp = convert_string_to_datetime(unpacked[COMMAND_TIMESTAMP])

        if command == COLLATERAL_INQUIRY:
            return CollateralInquiry(
                command_id,
                command_timestamp)
        elif command == SUBMIT_ORDER:
            return SubmitOrder(
                TraderId(unpacked[TRADER_ID]),
                StrategyId(unpacked[STRATEGY_ID]),
                PositionId(unpacked[POSITION_ID]),
                self.order_serializer.deserialize(unpacked[ORDER]),
                command_id,
                command_timestamp)
        elif command == SUBMIT_ATOMIC_ORDER:
            return SubmitAtomicOrder(
                TraderId(unpacked[TRADER_ID]),
                StrategyId(unpacked[STRATEGY_ID]),
                PositionId(unpacked[POSITION_ID]),
                AtomicOrder(self.order_serializer.deserialize(unpacked[ENTRY]),
                            self.order_serializer.deserialize(unpacked[STOP_LOSS]),
                            self.order_serializer.deserialize(unpacked[TAKE_PROFIT])),
                command_id,
                command_timestamp)
        elif command == MODIFY_ORDER:
            return ModifyOrder(
                TraderId(unpacked[TRADER_ID]),
                StrategyId(unpacked[STRATEGY_ID]),
                OrderId(unpacked[ORDER_ID]),
                Price(unpacked[MODIFIED_PRICE]),
                command_id,
                command_timestamp)
        elif command == CANCEL_ORDER:
            return CancelOrder(
                TraderId(unpacked[TRADER_ID]),
                StrategyId(unpacked[STRATEGY_ID]),
                OrderId(unpacked[ORDER_ID]),
                ValidString(unpacked[CANCEL_REASON]),
                command_id,
                command_timestamp)
        else:
            raise ValueError("Cannot deserialize command (unrecognized bytes pattern).")


cdef class MsgPackEventSerializer(EventSerializer):
    """
    Provides an event serializer for the MessagePack specification
    """

    cpdef bytes serialize(self, Event event):
        """
        Serialize the given event to MessagePack specification bytes.

        :param event: The event to serialize.
        :return: bytes.
        :raises: ValueError: If the event cannot be serialized.
        """
        cdef dict package = {
            TYPE: EVENT,
            EVENT: event.__class__.__name__,
            EVENT_ID: event.id.value,
            EVENT_TIMESTAMP: convert_datetime_to_string(event.timestamp)
        }

        if isinstance(event, AccountEvent):
            package[ACCOUNT_ID] = str(event.account_id)
            package[BROKER] = broker_string(event.broker)
            package[ACCOUNT_NUMBER] = str(event.account_number)
            package[CURRENCY] = currency_string(event.currency)
            package[CASH_BALANCE] = str(event.cash_balance)
            package[CASH_START_DAY] = str(event.cash_start_day)
            package[CASH_ACTIVITY_DAY] = str(event.cash_activity_day)
            package[MARGIN_USED_LIQUIDATION] = str(event.margin_used_liquidation)
            package[MARGIN_USED_MAINTENANCE] = str(event.margin_used_maintenance)
            package[MARGIN_RATIO] = str(event.margin_ratio)
            package[MARGIN_CALL_STATUS] = str(event.margin_call_status)
        elif isinstance(event, OrderInitialized):
            package[ORDER_ID] = str(event.order_id)
            package[SYMBOL] = str(event.symbol)
            package[LABEL] = str(event.label)
            package[ORDER_SIDE] = order_side_string(event.order_side)
            package[ORDER_TYPE] = order_type_string(event.order_type)
            package[QUANTITY] = event.quantity.value
            package[PRICE] = str(event.price)
            package[TIME_IN_FORCE] = time_in_force_string(event.time_in_force)
            package[EXPIRE_TIME] = convert_datetime_to_string(event.expire_time)
            return msgpack.packb(package)
        elif isinstance(event, OrderSubmitted):
            package[ORDER_ID] =  str(event.order_id)
            package[SYMBOL] =  str(event.symbol)
            package[SUBMITTED_TIME] = convert_datetime_to_string(event.submitted_time)
            return msgpack.packb(package)
        elif isinstance(event, OrderAccepted):
            package[ORDER_ID] =  str(event.order_id)
            package[SYMBOL] =  str(event.symbol)
            package[ACCEPTED_TIME] = convert_datetime_to_string(event.accepted_time)
            return msgpack.packb(package)
        elif isinstance(event, OrderRejected):
            package[ORDER_ID] =  str(event.order_id)
            package[SYMBOL] =  str(event.symbol)
            package[REJECTED_TIME] = convert_datetime_to_string(event.rejected_time)
            package[REJECTED_REASON] =  str(event.rejected_reason)
            return msgpack.packb(package)
        elif isinstance(event, OrderWorking):
            package[ORDER_ID] = str(event.order_id)
            package[ORDER_ID_BROKER] = str(event.order_id_broker)
            package[SYMBOL] = str(event.symbol)
            package[LABEL] = str(event.label)
            package[ORDER_SIDE] = order_side_string(event.order_side)
            package[ORDER_TYPE] = order_type_string(event.order_type)
            package[QUANTITY] = event.quantity.value
            package[PRICE] = str(event.price)
            package[TIME_IN_FORCE] = time_in_force_string(event.time_in_force)
            package[EXPIRE_TIME] = convert_datetime_to_string(event.expire_time)
            package[WORKING_TIME] = convert_datetime_to_string(event.working_time)
            return msgpack.packb(package)
        elif isinstance(event, OrderCancelReject):
            package[ORDER_ID] = str(event.order_id)
            package[SYMBOL] = str(event.symbol)
            package[REJECTED_TIME] = convert_datetime_to_string(event.cancel_reject_time)
            package[REJECTED_RESPONSE] = event.cancel_reject_response.value
            package[REJECTED_REASON] = event.cancel_reject_reason.value
            return msgpack.packb(package)
        elif isinstance(event, OrderCancelled):
            package[ORDER_ID] = str(event.order_id)
            package[SYMBOL] = str(event.symbol)
            package[CANCELLED_TIME] = convert_datetime_to_string(event.cancelled_time)
            return msgpack.packb(package)
        elif isinstance(event, OrderModified):
            package[ORDER_ID] = str(event.order_id)
            package[ORDER_ID_BROKER] = str(event.order_id_broker)
            package[SYMBOL] = str(event.symbol)
            package[MODIFIED_TIME] = convert_datetime_to_string(event.modified_time)
            package[MODIFIED_PRICE] = str(event.modified_price)
            return msgpack.packb(package)
        elif isinstance(event, OrderExpired):
            package[ORDER_ID] = str(event.order_id)
            package[SYMBOL] = str(event.symbol)
            package[EXPIRED_TIME] = convert_datetime_to_string(event.expired_time)
            return msgpack.packb(package)
        elif isinstance(event, OrderPartiallyFilled):
            package[ORDER_ID] = str(event.order_id)
            package[SYMBOL] = str(event.symbol)
            package[EXECUTION_ID] = str(event.execution_id)
            package[EXECUTION_TICKET] = str(event.execution_ticket)
            package[ORDER_SIDE] = order_side_string(event.order_side)
            package[FILLED_QUANTITY] = event.filled_quantity.value
            package[LEAVES_QUANTITY] = event.leaves_quantity.value
            package[AVERAGE_PRICE] = str(event.average_price)
            package[EXECUTION_TIME] = convert_datetime_to_string(event.execution_time)
            return msgpack.packb(package)
        elif isinstance(event, OrderFilled):
            package[ORDER_ID] = str(event.order_id)
            package[SYMBOL] = str(event.symbol)
            package[EXECUTION_ID] = event.execution_id.value
            package[EXECUTION_TICKET] = event.execution_ticket.value
            package[ORDER_SIDE] = order_side_string(event.order_side)
            package[FILLED_QUANTITY] = event.filled_quantity.value
            package[AVERAGE_PRICE] = str(event.average_price)
            package[EXECUTION_TIME] = convert_datetime_to_string(event.execution_time)
            return msgpack.packb(package)
        else:
            raise ValueError("Cannot serialize event (unrecognized event.")


    cpdef Event deserialize(self, bytes event_bytes):
        """
        Deserialize the given MessagePack specification bytes to an event.

        :param event_bytes: The bytes to deserialize.
        :return: Event.
        :raises ValueError: If the event_bytes is empty.
        :raises ValueError: If the event cannot be deserialized.
        """
        Precondition.not_empty(event_bytes, 'event_bytes')

        cdef dict unpacked = msgpack.unpackb(event_bytes, raw=False)

        cdef str message_type = unpacked[TYPE]
        if message_type != EVENT:
            raise ValueError("Cannot deserialize event (the message is not a type of event).")

        cdef str event_type = unpacked[EVENT]
        cdef GUID event_id = GUID(UUID(unpacked[EVENT_ID]))
        cdef datetime event_timestamp = convert_string_to_datetime(unpacked[EVENT_TIMESTAMP])

        if event_type == AccountEvent.__class__.__name__:
            return AccountEvent(
                AccountId(unpacked[ACCOUNT_ID]),
                Broker[unpacked[BROKER]],
                AccountNumber(unpacked[ACCOUNT_NUMBER]),
                Currency[unpacked[CURRENCY]],
                Money(unpacked[CASH_BALANCE]),
                Money(unpacked[CASH_START_DAY]),
                Money(unpacked[CASH_ACTIVITY_DAY]),
                Money(unpacked[MARGIN_USED_LIQUIDATION]),
                Money(unpacked[MARGIN_USED_MAINTENANCE]),
                Decimal(unpacked[MARGIN_RATIO]),
                ValidString('NONE'),
                event_id,
                event_timestamp)

        cdef Symbol order_symbol = parse_symbol(unpacked[SYMBOL])
        cdef OrderId order_id = OrderId(unpacked[ORDER_ID])

        if event_type == OrderSubmitted.__class__.__name__:
            return OrderSubmitted(
                order_id,
                order_symbol,
                convert_string_to_datetime(unpacked[SUBMITTED_TIME]),
                event_id,
                event_timestamp)
        elif event_type == OrderAccepted.__class__.__name__:
            return OrderAccepted(
                order_id,
                order_symbol,
                convert_string_to_datetime(unpacked[ACCEPTED_TIME]),
                event_id,
                event_timestamp)
        elif event_type == OrderRejected.__class__.__name__:
            return OrderRejected(
                order_id,
                order_symbol,
                convert_string_to_datetime(unpacked[REJECTED_TIME]),
                ValidString(unpacked[REJECTED_REASON]),
                event_id,
                event_timestamp)
        elif event_type == OrderWorking.__class__.__name__:
            return OrderWorking(
                order_id,
                order_symbol,
                OrderId(unpacked[ORDER_ID_BROKER]),
                Label(unpacked[LABEL]),
                OrderSide[unpacked[ORDER_SIDE]],
                OrderType[unpacked[ORDER_TYPE]],
                Quantity(unpacked[QUANTITY]),
                Price(unpacked[PRICE]),
                TimeInForce[unpacked[TIME_IN_FORCE]],
                convert_string_to_datetime(unpacked[WORKING_TIME]),
                event_id,
                event_timestamp,
                convert_string_to_datetime(unpacked[EXPIRE_TIME]))
        elif event_type == OrderCancelled.__class__.__name__:
            return OrderCancelled(
                order_id,
                order_symbol,
                convert_string_to_datetime(unpacked[CANCELLED_TIME]),
                event_id,
                event_timestamp)
        elif event_type == OrderCancelReject.__class__.__name__:
            return OrderCancelReject(
                order_id,
                order_symbol,
                convert_string_to_datetime(unpacked[REJECTED_TIME]),
                ValidString(unpacked[REJECTED_RESPONSE]),
                ValidString(unpacked[REJECTED_REASON]),
                event_id,
                event_timestamp)
        elif event_type == OrderModified.__class__.__name__:
            return OrderModified(
                order_id,
                order_symbol,
                OrderId(unpacked[ORDER_ID_BROKER]),
                Price(unpacked[MODIFIED_PRICE]),
                convert_string_to_datetime(unpacked[MODIFIED_TIME]),
                event_id,
                event_timestamp)
        elif event_type == OrderExpired.__class__.__name__:
            return OrderExpired(
                order_id,
                order_symbol,
                convert_string_to_datetime(unpacked[EXPIRED_TIME]),
                event_id,
                event_timestamp)
        elif event_type == OrderPartiallyFilled.__class__.__name__:
            return OrderPartiallyFilled(
                order_id,
                order_symbol,
                ExecutionId(unpacked[EXECUTION_ID]),
                ExecutionTicket(unpacked[EXECUTION_TICKET]),
                OrderSide[unpacked[ORDER_SIDE]],
                Quantity(unpacked[FILLED_QUANTITY]),
                Quantity(unpacked[LEAVES_QUANTITY]),
                Price(unpacked[AVERAGE_PRICE]),
                convert_string_to_datetime(unpacked[EXECUTION_TIME]),
                event_id,
                event_timestamp)
        elif event_type == OrderFilled.__class__.__name__:
            return OrderFilled(
                order_id,
                order_symbol,
                ExecutionId(unpacked[EXECUTION_ID]),
                ExecutionTicket(unpacked[EXECUTION_TICKET]),
                OrderSide[unpacked[ORDER_SIDE]],
                Quantity(unpacked[FILLED_QUANTITY]),
                Price(unpacked[AVERAGE_PRICE]),
                convert_string_to_datetime(unpacked[EXECUTION_TIME]),
                event_id,
                event_timestamp)
        else:
            raise ValueError("Cannot deserialize event (unrecognized event).")


cdef class MsgPackInstrumentSerializer(InstrumentSerializer):
    """
    Provides an instrument serializer for the MessagePack specification.
    """

    cpdef bytes serialize(self, Instrument instrument):
        """
        Serialize the given event to bytes.

        :param instrument: The instrument to serialize.
        :return: bytes.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef Instrument deserialize(self, bytes instrument_bytes):
        """
        Deserialize the given instrument bytes to an instrument.

        :param instrument_bytes: The bytes to deserialize.
        :return: Instrument.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")