# -------------------------------------------------------------------------------------------------
# <copyright file="identifiers.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.model.objects cimport ValidString, Symbol


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

cdef class InstrumentId(Identifier):
    pass


cdef class IdentifierGenerator:
    """
    Provides a generator for unique order identifiers.
    """
    cdef Clock _clock

    cdef readonly str prefix
    cdef readonly str id_tag_trader
    cdef readonly str id_tag_strategy
    cdef readonly int counter

    cpdef void reset(self)

    cdef str _generate(self)
    cdef str _get_datetime_tag(self)


cdef class OrderIdGenerator(IdentifierGenerator):
    """
    Provides a generator for unique OrderId(s).
    """
    cpdef OrderId generate(self)


cdef class PositionIdGenerator(IdentifierGenerator):
    """
    Provides a generator for unique PositionId(s).
    """
    cpdef PositionId generate(self)
