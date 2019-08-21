# -------------------------------------------------------------------------------------------------
# <copyright file="common.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import re
import iso8601

from cpython.datetime cimport datetime

from nautilus_trader.model.enums import Resolution, QuoteType
from nautilus_trader.model.identifiers cimport Symbol, Venue, Label
from nautilus_trader.model.c_enums.resolution cimport Resolution
from nautilus_trader.model.c_enums.quote_type cimport QuoteType
from nautilus_trader.model.objects cimport Price, BarSpecification, BarType, Tick, Bar
from nautilus_trader.serialization.constants cimport *


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

cpdef Tick parse_tick(Symbol symbol, str tick_string):
    """
    Return a parsed a tick from the given UTF-8 string.

    :param symbol: The tick symbol.
    :param tick_string: The tick string.
    :return: Tick.
    """
    cdef list split_tick = tick_string.split(',')

    return Tick(
        symbol,
        Price(split_tick[0]),
        Price(split_tick[1]),
        iso8601.parse_date(split_tick[2]))

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
        Resolution[resolution],
        QuoteType[quote_type])

cpdef BarType parse_bar_type(str bar_type_string):
    """
    Return a parsed a bar type from the given UTF-8 string.

    :param bar_type_string: The bar type string to parse.
    :return: BarType.
    """
    cdef list split_string = re.split(r'[.-]+', bar_type_string)
    cdef str resolution = split_string[3].split('[')[0]
    cdef str quote_type = split_string[3].split('[')[1].strip(']')
    cdef Symbol symbol = Symbol(split_string[0], Venue(split_string[1].upper()))
    cdef BarSpecification bar_spec = BarSpecification(int(split_string[2]),
                                                      Resolution[resolution.upper()],
                                                      QuoteType[quote_type.upper()])
    return BarType(symbol, bar_spec)

cpdef Bar parse_bar(str bar_string):
    """
    Return a parsed bar from the given UTF-8 string.

    :param bar_string: The bar string to parse.
    :return: Bar.
    """
    cdef list split_bar = bar_string.split(',')

    return Bar(Price(split_bar[0]),
               Price(split_bar[1]),
               Price(split_bar[2]),
               Price(split_bar[3]),
               int(split_bar[4]),
               iso8601.parse_date(split_bar[5]))
