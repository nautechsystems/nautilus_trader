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
import iso8601
import json

from cpython.datetime cimport datetime
from decimal import Decimal

from inv_trader.commands cimport Command
from inv_trader.model.enums import Venue, CurrencyCode
from inv_trader.enums.venue cimport Venue
from inv_trader.enums.security_type cimport SecurityType
from inv_trader.enums.currency_code cimport CurrencyCode
from inv_trader.model.identifiers cimport GUID
from inv_trader.model.objects cimport Symbol, Instrument
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport Event


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


cpdef Symbol _parse_symbol(str symbol_string):
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


cpdef datetime _convert_string_to_datetime(str expire_time_string):
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

    cpdef bytes serialize(self, Order order):
        """
        Serialize the given order to bytes.

        :param order: The order to serialize.
        :return: The serialized order.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")

    cpdef Order deserialize(self, bytes order_bytes):
        """
        Deserialize the given bytes to an Order.

        :param order_bytes: The bytes to deserialize.
        :return: The deserialized order.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")


cdef class CommandSerializer:
    """
    The abstract base class for all command serializers.
    """

    cpdef bytes serialize(self, Command command):
        """
        Serialize the given command to bytes.

        :param: command: The command to serialize.
        :return: The serialized command.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")

    cpdef Command deserialize(self, bytes command_bytes):
        """
        Deserialize the given bytes to a Command.

        :param: command_bytes: The command bytes to deserialize.
        :return: The deserialized command.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")
    cdef bytes _serialize_order_command(self, OrderCommand order_command):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")
    cdef OrderCommand _deserialize_order_command(
            self,
            GUID command_id,
            datetime command_timestamp,
            dict unpacked):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")

cdef class EventSerializer:
    """
    The abstract base class for all event serializers.
    """

    cpdef bytes serialize(self, Event event):
        """
        Serialize the given event to bytes.

        :param event: The event to serialize.
        :return: The serialized event.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")

    cpdef Event deserialize(self, bytes event_bytes):
        """
        Deserialize the given bytes to an event.

        :param event_bytes: The bytes to deserialize.
        :return: The deserialized event.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")


cdef class InstrumentSerializer:
    """
    Provides an instrument deserializer.
    """

    cpdef Instrument deserialize(self, bytes instrument_bytes):
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
