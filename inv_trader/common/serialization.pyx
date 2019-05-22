#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="serialization.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

import ast
import cython
import iso8601
import json

from decimal import Decimal
from cpython.datetime cimport datetime

# Do not reorder imports (enums need to be in below order)
from inv_trader.model.enums import Venue, SecurityType
from inv_trader.enums.venue cimport Venue
from inv_trader.enums.security_type cimport SecurityType
from inv_trader.enums.currency cimport Currency
from inv_trader.model.identifiers cimport GUID, Label
from inv_trader.model.objects cimport Symbol, Price, Instrument
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport Event
from inv_trader.commands cimport Command


cdef str UTF8 = 'utf-8'
cdef str NONE = 'NONE'
cdef str COMMAND_TYPE = 'CommandType'
cdef str COMMAND_ID = 'CommandId'
cdef str COMMAND_TIMESTAMP = 'CommandTimestamp'
cdef str COLLATERAL_INQUIRY = 'CollateralInquiry'
cdef str ORDER_COMMAND = 'OrderCommand'
cdef str SUBMIT_ORDER = 'SubmitOrder'
cdef str SUBMIT_ATOMIC_ORDER = 'SubmitAtomicOrder'
cdef str CANCEL_ORDER = 'CancelOrder'
cdef str MODIFY_ORDER = 'ModifyOrder'
cdef str CANCEL_REASON = 'CancelReason'
cdef str ORDER = 'Order'
cdef str TIMESTAMP = 'Timestamp'
cdef str EVENT_TYPE = 'EventType'
cdef str ORDER_EVENT = 'OrderEvent'
cdef str ACCOUNT_EVENT = 'AccountEvent'
cdef str SYMBOL = 'Symbol'
cdef str ORDER_ID = 'OrderId'
cdef str ORDER_ID_BROKER = 'OrderIdBroker'
cdef str EVENT_ID = 'EventId'
cdef str TRADER_ID = 'TraderId'
cdef str STRATEGY_ID = 'StrategyId'
cdef str POSITION_ID = 'PositionId'
cdef str EVENT_TIMESTAMP = 'EventTimestamp'
cdef str LABEL = 'Label'
cdef str ORDER_SUBMITTED = 'OrderSubmitted'
cdef str ORDER_ACCEPTED = 'OrderAccepted'
cdef str ORDER_REJECTED = 'OrderRejected'
cdef str ORDER_WORKING = 'OrderWorking'
cdef str ORDER_CANCELLED = 'OrderCancelled'
cdef str ORDER_CANCEL_REJECT = 'OrderCancelReject'
cdef str ORDER_MODIFIED = 'OrderModified'
cdef str ORDER_EXPIRED = 'OrderExpired'
cdef str ORDER_PARTIALLY_FILLED = 'OrderPartiallyFilled'
cdef str ORDER_FILLED = 'OrderFilled'
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


cpdef Symbol parse_symbol(str symbol_string):
    """
    Return the parsed symbol from the given string.

    :param symbol_string: The symbol string to parse.
    :return: Symbol.
    """
    cdef tuple split_symbol = symbol_string.partition('.')
    return Symbol(split_symbol[0], Venue[split_symbol[2].upper()])


cpdef str convert_price_to_string(Price price):
    """
    Return the converted string from the given price, can return a 'NONE' string..

    :param price: The price to convert.
    :return: str.
    """
    return NONE if price is None else str(price)


cpdef Price convert_string_to_price(str price_string):
    """
    Return the converted price (or None) from the given price string.

    :param price_string: The price string to convert.
    :return: Price or None.
    """
    return None if price_string == NONE else Price(price_string)

cpdef str convert_label_to_string(Label label):
    """
    Return the converted string from the given label, can return a 'NONE' string.

    :param label: The label to convert.
    :return: str.
    """
    return NONE if label is None else label.value

cpdef Label convert_string_to_label(str label):
    """
    Return the converted label (or None) from the given label string.

    :param label: The label string to convert.
    :return: Label or None.
    """
    return None if label == NONE else Label(label)

cpdef str convert_datetime_to_string(datetime time):
    """
    Return the converted ISO8601 string from the given datetime, can return a 'NONE' string.

    :param time: The datetime to convert
    :return: str.
    """
    return NONE if time is None else time.isoformat(timespec='milliseconds').replace('+00:00', 'Z')

cpdef datetime convert_string_to_datetime(str time_string):
    """
    Return the converted datetime (or None) from the given time string.

    :param time_string: The time string to convert.
    :return: datetime or None.
    """
    return None if time_string == NONE else iso8601.parse_date(time_string)


cdef class OrderSerializer:
    """
    The abstract base class for all order serializers.
    """

    cpdef bytes serialize(self, Order order):
        """
        Serialize the given order to bytes.

        :param order: The order to serialize.
        :return: bytes.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef Order deserialize(self, bytes order_bytes):
        """
        Deserialize the given bytes to an order.

        :param order_bytes: The bytes to deserialize.
        :return: Order.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass. ")


cdef class CommandSerializer:
    """
    The abstract base class for all command serializers.
    """

    cpdef bytes serialize(self, Command command):
        """
        Serialize the given command to bytes.

        :param: command: The command to serialize.
        :return: bytes.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef Command deserialize(self, bytes command_bytes):
        """
        Deserialize the given bytes to a command.

        :param: command_bytes: The command bytes to deserialize.
        :return: Command.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef bytes _serialize_order_command(self, OrderCommand order_command):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented.")

    cdef OrderCommand _deserialize_order_command(
            self,
            GUID command_id,
            datetime command_timestamp,
            dict unpacked):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")


cdef class EventSerializer:
    """
    The abstract base class for all event serializers.
    """

    cpdef bytes serialize(self, Event event):
        """
        Serialize the given event to bytes.

        :param event: The event to serialize.
        :return: bytes.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef Event deserialize(self, bytes event_bytes):
        """
        Deserialize the given bytes to an event.

        :param event_bytes: The bytes to deserialize.
        :return: Event.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef bytes _serialize_order_event(self, OrderEvent order_event):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef OrderEvent _deserialize_order_event(
            self,
            GUID event_id,
            datetime event_timestamp,
            dict unpacked):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")


@cython.wraparound(True)
cdef class InstrumentSerializer:
    """
    Provides an instrument deserializer.
    """

    cpdef Instrument deserialize(self, bytes instrument_bytes):
        """
        Deserialize the given instrument bytes to an instrument.

        :param instrument_bytes: The string to deserialize.
        :return: Instrument.
        :raises ValueError: If the instrument_bytes is empty.
        :raises ValueError: If the instrument cannot be deserialized.
        """
        cdef dict inst_dict = ast.literal_eval((json.loads(instrument_bytes)
                                      .replace("\"", "\'")
                                      .replace("\'Timestamp\':", "\'Timestamp\':\'")[:-1] + "\'}"))

        tick_size = inst_dict['TickSize']
        rollover_interest_buy = inst_dict['RolloverInterestBuy']
        rollover_interest_sell = inst_dict['RolloverInterestSell']

        return Instrument(
            Symbol(inst_dict['Symbol']['Code'], Venue[inst_dict['Symbol']['Venue'].upper()]),
            inst_dict['BrokerSymbol']['Value'],
            Currency[inst_dict['QuoteCurrency'].upper()],
            SecurityType[inst_dict['SecurityType'].upper()],
            inst_dict['TickPrecision'],
            Decimal(f'{tick_size}'),
            inst_dict['RoundLotSize'],
            inst_dict['MinStopDistanceEntry'],
            inst_dict['MinLimitDistanceEntry'],
            inst_dict['MinStopDistance'],
            inst_dict['MinLimitDistance'],
            inst_dict['MinTradeSize'],
            inst_dict['MaxTradeSize'],
            Decimal(f'{rollover_interest_buy}'),
            Decimal(f'{rollover_interest_sell}'),
            iso8601.parse_date(inst_dict['Timestamp']))
