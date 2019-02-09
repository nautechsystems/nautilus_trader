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

from inv_trader.model.identifiers cimport GUID, Label, PositionId
from inv_trader.model.objects cimport ValidString, Price
from inv_trader.model.order cimport Order, AtomicOrder


cdef class Command:
    """
    The abstract base class for all commands.
    """
    cdef readonly GUID id
    cdef readonly datetime timestamp


cdef class CollateralInquiry(Command):
    """
    Represents a request for a FIX collateral inquiry of all connected accounts.
    """


cdef class OrderCommand(Command):
    """
    The abstract base class for all order commands.
    """
    cdef readonly Order order


cdef class SubmitOrder(OrderCommand):
    """
    Represents a command to submit an order.
    """
    cdef readonly PositionId position_id
    cdef readonly GUID strategy_id
    cdef readonly Label strategy_name


cdef class SubmitAtomicOrder(Command):
    """
    Represents a command to submit an atomic order.
    """
    cdef readonly AtomicOrder atomic_order
    cdef readonly PositionId position_id
    cdef readonly GUID strategy_id
    cdef readonly Label strategy_name


cdef class ModifyOrder(OrderCommand):
    """
    Represents a command to modify an order with the modified price.
    """
    cdef readonly Price modified_price


cdef class CancelOrder(OrderCommand):
    """
    Represents a command to cancel an order.
    """
    cdef readonly ValidString cancel_reason
