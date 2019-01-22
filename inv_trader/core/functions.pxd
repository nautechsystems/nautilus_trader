#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="functions.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from inv_trader.model.identifiers cimport Identifier
from inv_trader.model.objects cimport Symbol


cdef inline bint is_in(Identifier identifier, list collection):
    """
    Return a value indicating whether the identifier is contained in the given collection.

    :param identifier: The identifier to check.
    :param collection: The collection to check.
    :return: True if identifier contained, otherwise False.
    """
    for i in range(len(collection)):
        if identifier.value == collection[i].value:
            return True

    return False


cdef inline bint symbol_is_in(Symbol symbol, list collection):
    """
    Return a value indicating whether the symbol is contained in the given collection.

    :param symbol: The symbol to check.
    :param collection: The collection to check.
    :return: True if symbol contained, otherwise False.
    """
    for i in range(len(collection)):
        if symbol.code == collection[i].code and symbol.venue == collection[i].venue:
            return True

    return False
