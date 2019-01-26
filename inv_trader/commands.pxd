#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="commands.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from cpython.datetime cimport datetime

from inv_trader.model.identifiers cimport GUID
from inv_trader.model.order cimport Order


cdef class Command:
    """
    The abstract base class for all commands.
    """
    cdef readonly GUID id
    cdef readonly datetime timestamp


cdef class OrderCommand(Command):
    """
    The abstract base class for all order commands.
    """
    cdef readonly Order order


cdef class SubmitOrder(OrderCommand):
    """
    Represents a command to submit the given order.
    """


cdef class CancelOrder(OrderCommand):
    """
    Represents a command to cancel the given order.
    """
    cdef readonly str cancel_reason


cdef class ModifyOrder(OrderCommand):
    """
    Represents a command to modify the given order with the given modified price.
    """
    cdef readonly object modified_price


cdef class CollateralInquiry(Command):
    """
    Represents a request for a FIX collateral inquiry of all connected accounts.
    """
