#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="common.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

import iso8601

from cpython.datetime cimport datetime

from nautilus_trader.model.enums import Venue, Resolution, QuoteType, OrderSide
from nautilus_trader.model.c_enums.venue cimport Venue
from nautilus_trader.model.c_enums.resolution cimport Resolution
from nautilus_trader.model.c_enums.quote_type cimport QuoteType
from nautilus_trader.model.identifiers cimport Label
from nautilus_trader.model.objects cimport Symbol, Price, BarSpecification
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
