#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="serialization.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

import iso8601

from cpython.datetime cimport datetime

# Do not reorder imports (enums need to be in below order)
from nautilus_trader.core.message cimport Command, Event, Request, Response
from nautilus_trader.model.c_enums.venue cimport Venue
from nautilus_trader.model.c_enums.resolution cimport Resolution
from nautilus_trader.model.c_enums.quote_type cimport QuoteType
from nautilus_trader.model.identifiers cimport Label
from nautilus_trader.model.objects cimport Symbol, Price, BarSpecification, Bar, Tick, Instrument, Quantity
from nautilus_trader.model.order cimport Order

cdef str UTF8 = 'utf-8'
cdef str NONE = 'NONE'


cpdef Symbol parse_symbol(str symbol_string):
    """
    Return the parsed symbol from the given string.

    Note: String format example is 'AUDUSD.FXCM'.
    :param symbol_string: The symbol string to parse.
    :return: Symbol.
    """
    cdef tuple split_symbol = symbol_string.partition('.')
    return Symbol(split_symbol[0], Venue[split_symbol[2].upper()])

cpdef BarSpecification parse_bar_spec(str bar_spec_string):
    """
    Return the parsed bar specification from the given string.
    
    Note: String format example is '1-MINUTE-[BID]'.
    :param bar_spec_string: The bar specification string to parse.
    :return: BarSpecification.
    """
    cdef list split1 = bar_spec_string.split('-')
    cdef list split2 = split1[1].split('[')
    cdef str resolution = split2[0]
    cdef str quote_type = split2[1].strip(']')

    return BarSpecification(
        int(split1[0]),
        Resolution[resolution.upper()],
        QuoteType[quote_type.upper()])

cpdef Tick deserialize_tick(Symbol symbol, bytes tick_bytes):
    """
    Return a parsed a tick from the given UTF-8 string.

    :param symbol: The ticks symbol.
    :param tick_bytes: The tick bytes to deserialize.
    :return: Tick.
    """
    cdef list values = tick_bytes.decode(UTF8).split(',')

    return Tick(
        symbol,
        Price(values[0]),
        Price(values[1]),
        iso8601.parse_date(values[2]))

cpdef Bar deserialize_bar(bytes bar_bytes):
    """
    Return the deserialized bar from the give bytes.
    
    :param bar_bytes: The bar bytes to deserialize.
    :return: Bar.
    """
    cdef list values = bar_bytes.decode(UTF8).split(',')

    return Bar(
        Price(values[0]),
        Price(values[1]),
        Price(values[2]),
        Price(values[3]),
        Quantity(values[4]),
        iso8601.parse_date(values[5]))

# cpdef bytes serialize_ticks(Tick[:] ticks):
#     """
#     TBD.
#     :param ticks:
#     :return:
#     """
#     cdef int ticks_length = len(ticks)
#     cdef bytearray tick_bytes = bytearray(ticks_length)
#     cdef int i
#     for i in range(ticks_length):
#         tick_bytes[i] = str(ticks[i].values_str()).encode(UTF8)
#
#     return bytes(tick_bytes)
#
# cpdef Tick[:] deserialize_ticks(Symbol symbol, bytes tick_bytes):
#     """
#     Return a list of deserialized ticks from the given symbol and tick bytes.
#
#     :param symbol: The tick symbol.
#     :param tick_bytes: The tick bytes to deserialize.
#     :return: Tick[:].
#     """
#     cdef list ticks = []
#     cdef int i
#     cdef int array_length = len(tick_bytes)
#     for i in range(array_length):
#         ticks.append(deserialize_tick(symbol, tick_bytes[i]))
#
#     return ticks

cpdef list deserialize_bars(bytes[:] bar_bytes_array):
    """
    Return a list of deserialized bars from the given bars bytes.
    
    :param bar_bytes_array: The bar bytes to deserialize.
    :return: List[Tick].
    """
    cdef list bars = []
    cdef int i
    cdef int array_length = len(bar_bytes_array)
    for i in range(array_length):
        bars.append(deserialize_bar(bar_bytes_array[i]))

    return bars

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


cdef class InstrumentSerializer:
    """
    The abstract base class for all instrument serializers.
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
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")


cdef class RequestSerializer:
    """
    The abstract base class for all request serializers.
    """

    cpdef bytes serialize(self, Request request):
        """
        Serialize the given request to bytes.

        :param request: The event to serialize.
        :return: bytes.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef Request deserialize(self, bytes request_bytes):
        """
        Deserialize the given bytes to a request.

        :param request_bytes: The bytes to deserialize.
        :return: Request.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")


cdef class ResponseSerializer:
    """
    The abstract base class for all response serializers.
    """

    cpdef bytes serialize(self, Response response):
        """
        Serialize the given response to bytes.

        :param response: The event to serialize.
        :return: bytes.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef Response deserialize(self, bytes response_bytes):
        """
        Deserialize the given bytes to a response.

        :param response_bytes: The bytes to deserialize.
        :return: Response.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")
