#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="identifiers.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from inv_trader.common.clock cimport Clock
from inv_trader.model.objects cimport ValidString, Symbol


cdef class Identifier:
    cdef readonly str value
    cpdef bint equals(self, Identifier other)

cdef class GUID(Identifier):
    pass

cdef class Label(Identifier):
    pass

cdef class TraderId(Identifier):
    pass

cdef class StrategyId(Identifier):
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


cdef class IdentifierGenerator:
    """
    Provides a generator for unique order identifiers.
    """
    cdef Clock _clock
    cdef dict _symbol_counts

    cdef readonly str id_tag_trader
    cdef readonly str id_tag_strategy

    cpdef void reset(self)

    cdef str _generate(self, Symbol symbol)


cdef class OrderIdGenerator(IdentifierGenerator):
    """
    Provides a generator for unique OrderIds.
    """
    cpdef OrderId generate(self, Symbol symbol)


cdef class PositionIdGenerator(IdentifierGenerator):
    """
    Provides a generator for unique PositionIds.
    """
    cpdef PositionId generate(self, Symbol symbol)
