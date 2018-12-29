#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="identifiers.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False


cdef class Identifier:
    cdef readonly str value

cdef class Label(Identifier):
    pass

cdef class AccountId(Identifier):
    pass

cdef class AccountNumber(Identifier):
    pass

cdef class OrderId(Identifier):
    pass

cdef class PositionId(Identifier):
    pass

cdef class ExecutionId(Identifier):
    pass

cdef class ExecutionTicket(Identifier):
    pass
